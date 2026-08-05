use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::models::{BuildResult, DebugInfo, JobSpec, NixEvalJobsResult, SystemType};
use crate::op::Operation;
use crate::process::{get_config_attr, run, run_json};

/// Evaluate the flake's `blzrd.nodes` and return enriched `JobSpec`s.
pub async fn eval_deployments(cfg: &str) -> Result<(HashMap<String, JobSpec>, Vec<DebugInfo>)> {
    let (text, mut debug_infos) = run_eval_jobs(cfg).await?;
    let results = parse_json_lines(&text)?;
    let basics = collect_basics(&results)?;
    let jobs = build_job_specs(cfg, &results, &basics, &mut debug_infos).await?;
    Ok((jobs, debug_infos))
}

/// Run `nix-eval-jobs` on the flake's `blzrd.nodes` and return it + `DebugInfo`.
async fn run_eval_jobs(cfg: &str) -> Result<(String, Vec<DebugInfo>)> {
    let flake_reference = format!("{cfg}#blzrd.nodes");

    let cache_home = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let home =
                std::env::var("HOME").expect("HOME must be set if XDG_CACHE_HOME is not set");
            format!("{home}/.cache")
        });
    let gc_roots_dir: PathBuf = PathBuf::from(cache_home).join("blzrd");

    std::fs::create_dir_all(&gc_roots_dir)
        .with_context(|| format!("failed to create gc-roots dir {}", gc_roots_dir.display()))?;

    let (out, debug) = run(
        "nix-eval-jobs",
        &[
            "--gc-roots-dir",
            &gc_roots_dir.to_string_lossy(),
            "--force-recurse",
            "--flake",
            &flake_reference,
        ],
    )
    .await?;

    Ok((String::from_utf8_lossy(&out).into_owned(), vec![debug]))
}

/// Parse the JSON-Lines text emitted by `nix-eval-jobs` into typed records.
fn parse_json_lines(text: &str) -> Result<Vec<NixEvalJobsResult>> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse JSON from nix-eval-jobs")
}

/// First pass: extract the basic `(output, drv_path)` pair for each job.
fn collect_basics(results: &[NixEvalJobsResult]) -> Result<HashMap<String, (String, String)>> {
    let mut basics: HashMap<String, (String, String)> = HashMap::with_capacity(results.len());
    for result in results {
        // Error records from nix-eval-jobs carry `attr` and `error`.
        if let Some(err) = &result.error {
            anyhow::bail!("job {}: {err}", result.attr);
        }

        let attr_path = result
            .attr_path
            .as_ref()
            .with_context(|| format!("job {}: missing attr_path", result.attr))?;
        if attr_path.len() < 2 {
            anyhow::bail!("job {}: malformed attr_path: {:?}", result.attr, attr_path);
        }
        let job_name = attr_path[0].clone();

        let outputs = result
            .outputs
            .as_ref()
            .with_context(|| format!("job {job_name}: missing outputs"))?;
        let output_path = outputs
            .get("out")
            .with_context(|| format!("job {job_name}: missing 'out' output"))?;

        let drv_path = result.drv_path.clone().unwrap_or_default();

        basics.insert(job_name, (output_path.clone(), drv_path));
    }
    Ok(basics)
}

/// Second pass: enrich each job's basic data with hostname, user, and system type.
async fn build_job_specs(
    cfg: &str,
    results: &[NixEvalJobsResult],
    basics: &HashMap<String, (String, String)>,
    debug_infos: &mut Vec<DebugInfo>,
) -> Result<HashMap<String, JobSpec>> {
    let mut jobs: HashMap<String, JobSpec> = HashMap::with_capacity(basics.len());

    for name in basics.keys() {
        let (output, drv_path) = basics.get(name).cloned().unwrap_or_default();

        // hostname: defaults to the job name, override if flake provides one.
        let mut hostname = name.clone();
        if let Ok((h, debug)) = get_config_attr(cfg, name, "hostname").await {
            debug_infos.push(debug);
            if !h.is_empty() {
                hostname = h;
            }
        }

        // user
        let mut user = String::new();
        if let Ok((u, debug)) = get_config_attr(cfg, name, "user").await {
            debug_infos.push(debug);
            user = u;
        }

        // system type: from flake, or inferred from the build system string.
        let mut type_str = String::new();
        if let Ok((t, debug)) = get_config_attr(cfg, name, "type").await {
            debug_infos.push(debug);
            type_str = t;
        }

        if type_str.is_empty() {
            let system = results
                .iter()
                .find(|r| r.attr_path.as_deref().and_then(|p| p.first()) == Some(name))
                .and_then(|r| r.system.as_deref())
                .unwrap_or_default();

            type_str = if system.contains("darwin") {
                "darwin".to_string()
            } else if system.contains("linux") {
                "nixos".to_string()
            } else {
                anyhow::bail!("job {name}: unknown system type: {system}");
            };
        }

        let system =
            SystemType::parse(&type_str).map_err(|e| anyhow::anyhow!("job {name}: {e}"))?;

        if output.is_empty() {
            anyhow::bail!("job {name}: missing output path");
        }
        if user.is_empty() {
            anyhow::bail!("job {name}: missing user");
        }

        jobs.insert(
            name.clone(),
            JobSpec {
                hostname,
                system,
                user,
                drv_path,
            },
        );
    }

    Ok(jobs)
}

/// Build a job's derivation and return the `out` store path.
pub async fn build_closure(spec: &JobSpec, build_host: &str) -> Result<(String, DebugInfo)> {
    let drv = format!("{}^*", spec.drv_path);

    if build_host == "localhost" {
        let (results, debug): (Vec<BuildResult>, _) =
            run_json("nix", &["build", "--no-link", "--json", &drv]).await?;
        let out = results
            .into_iter()
            .next()
            .and_then(|r| r.outputs.get("out").cloned())
            .context("build result missing 'out' output")?;
        return Ok((out, debug));
    }

    // Remote builder branch.
    let store = format!("ssh-ng://{build_host}");

    // 1. Copy the derivation to the builder.
    let (_out, _debug) = run("nix", &["copy", "--to", &store, &spec.drv_path])
        .await
        .with_context(|| format!("copy to {build_host}"))?;

    // 2. Build on the builder.
    let (results, debug): (Vec<BuildResult>, _) = run_json(
        "nix",
        &["build", "--no-link", "--json", "--store", &store, &drv],
    )
    .await
    .with_context(|| format!("build on {build_host}"))?;

    let out = results
        .into_iter()
        .next()
        .and_then(|r| r.outputs.get("out").cloned())
        .context("build result missing 'out' output")?;

    // 3. Copy the out path back.
    let (_out, _debug2) = run("nix", &["copy", "--from", &store, &out, "--no-check-sigs"])
        .await
        .with_context(|| format!("copy from {build_host}"))?;

    Ok((out, debug))
}

/// Poll a remote `blzrd-activate-*` transient systemd unit until it reaches a
/// terminal state, then return. Exponential backoff 5 -> 60 seconds, 5-minute deadline.
async fn poll_activation(target: &str, unit: &str) -> Result<()> {
    let mut sleep_val = Duration::from_secs(5);
    let deadline = Instant::now() + Duration::from_secs(300);

    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("activation on {target}: timed out waiting for {unit} unit");
        }

        let args: Vec<&str> = vec![
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            target,
            "systemctl",
            "show",
            unit,
        ];
        let result = run("ssh", &args).await.ok();

        let mut sub_state: Option<String> = None;
        let mut exec_status: Option<i32> = None;
        if let Some((out, debug_info)) = result {
            let text = String::from_utf8_lossy(&out);
            log::debug!(
                "{unit} on {target}:\n{}stderr: {}",
                text.trim(),
                debug_info.std_err.trim()
            );
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("SubState=") {
                    sub_state = Some(v.trim().to_string());
                } else if let Some(v) = line.strip_prefix("ExecMainStatus=") {
                    exec_status = v.trim().parse::<i32>().ok();
                }
            }
        }

        // With --remain-after-exit, the unit stays active but transitions to
        // SubState=exited once the main process terminates.
        match (sub_state.as_deref(), exec_status.unwrap_or(0)) {
            (Some("exited"), 0) => break,
            (Some("exited"), code) => {
                anyhow::bail!(
                    "activation on {target}: switch-to-configuration exited with code {code}"
                );
            }
            (Some("failed"), _) => {
                anyhow::bail!(
                    "activation on {target}: unit failed (ExecMainStatus={})",
                    exec_status.unwrap_or(-1)
                );
            }
            _ => {
                sleep(sleep_val).await;
                sleep_val = (sleep_val * 2).min(Duration::from_secs(60));
            }
        }

        if sleep_val > Duration::from_secs(60) {
            anyhow::bail!("activation on {target}: gave up after backoff cap exceeded");
        }
    }

    Ok(())
}

pub async fn deploy_closure(spec: &JobSpec, out_path: &str, op: Operation) -> Result<DebugInfo> {
    let target = spec.target();
    let path = out_path.to_string();

    let unit = format!("blzrd-activate-{}", Uuid::new_v4().simple());

    let mut cmds: Vec<Vec<String>> = Vec::new();

    let sys = spec.system;

    match (sys, op) {
        (SystemType::Darwin, Operation::Switch) => {
            cmds.push(vec![
                "ssh".into(),
                target.clone(),
                "PATH=/run/current-system/sw/bin:$PATH".into(),
                "sudo".into(),
                "nix-env".into(),
                "-p".into(),
                "/nix/var/nix/profiles/system".into(),
                "--set".into(),
                path.clone(),
            ]);
            cmds.push(vec![
                "ssh".into(),
                target.clone(),
                "PATH=/run/current-system/sw/bin:$PATH".into(),
                "sudo".into(),
                format!("{path}/activate"),
            ]);
        }

        (SystemType::Darwin, Operation::Boot) => {
            anyhow::bail!(
                "job {}: 'boot' is not a valid darwin operation",
                spec.hostname
            );
        }

        (SystemType::Nixos, Operation::Switch) => {
            cmds.push(vec![
                "ssh".into(),
                target.clone(),
                "sudo".into(),
                "nix-env".into(),
                "-p".into(),
                "/nix/var/nix/profiles/system".into(),
                "--set".into(),
                path.clone(),
            ]);
            cmds.push(vec![
                "ssh".into(),
                target.clone(),
                "sudo".into(),
                "systemd-run".into(),
                "--unit".into(),
                unit.clone(),
                "--remain-after-exit".into(),
                "--no-block".into(),
                "--".into(),
                format!("{path}/bin/switch-to-configuration"),
                op.to_string(),
            ]);
        }

        (SystemType::Nixos, Operation::Boot) => {
            cmds.push(vec![
                "ssh".into(),
                target.clone(),
                "sudo".into(),
                "nix-env".into(),
                "-p".into(),
                "/nix/var/nix/profiles/system".into(),
                "--set".into(),
                path.clone(),
            ]);
            cmds.push(vec![
                "ssh".into(),
                target.clone(),
                "sudo".into(),
                format!("{path}/bin/switch-to-configuration"),
                op.to_string(),
            ]);
        }
    }

    // 1. Copy the closure to the target.
    let (_out, debug) = run(
        "nix",
        &[
            "copy",
            "--to",
            &format!("ssh-ng://{target}"),
            &path,
            "--no-check-sigs",
        ],
    )
    .await
    .with_context(|| format!("copy to {target}"))?;

    // 2. Run each activation command in order.
    for cmd in &cmds {
        let args: Vec<&str> = cmd[1..].iter().map(String::as_str).collect();
        let (_out, _d) = run(&cmd[0], &args)
            .await
            .with_context(|| format!("activation on {target}"))?;
    }

    if matches!((sys, op), (SystemType::Nixos, Operation::Switch)) {
        poll_activation(&target, &unit)
            .await
            .with_context(|| format!("activation on {target}"))?;
    }

    Ok(debug)
}
