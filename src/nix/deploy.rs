use std::time::Duration;

use anyhow::{Context, Result};
use tokio::time::{sleep, Instant};
use uuid::Uuid;

use crate::models::{BuildResult, JobSpec, SystemType};
use crate::op::Operation;
use crate::process::{run_json, run_json_with_env, run_with_env};
use crate::ssh::{run as run_ssh, HostKeyPolicy};

/// Build a job's derivation and return the `out` store path.
pub async fn build_closure(
    spec: &JobSpec,
    build_host: &str,
    host_key_policy: HostKeyPolicy,
) -> Result<String> {
    let derivation = format!("{}^*", spec.drv_path);

    if build_host == "localhost" {
        return build_local_closure(&derivation).await;
    }

    build_remote_closure(&derivation, build_host, host_key_policy).await
}

async fn build_local_closure(derivation: &str) -> Result<String> {
    let results: Vec<BuildResult> =
        run_json("nix", &["build", "--no-link", "--json", derivation]).await?;
    build_output_path(results)
}

async fn build_remote_closure(
    derivation: &str,
    build_host: &str,
    host_key_policy: HostKeyPolicy,
) -> Result<String> {
    let store = format!("ssh-ng://{build_host}");
    let nix_ssh_options = nix_ssh_options(host_key_policy);
    let nix_env = [("NIX_SSHOPTS", nix_ssh_options.as_str())];

    run_with_env("nix", &["copy", "--to", &store, derivation], &nix_env)
        .await
        .with_context(|| format!("copy to {build_host}"))?;

    let results: Vec<BuildResult> = run_json_with_env(
        "nix",
        &[
            "build",
            "--no-link",
            "--json",
            "--store",
            &store,
            derivation,
        ],
        &nix_env,
    )
    .await
    .with_context(|| format!("build on {build_host}"))?;
    let output = build_output_path(results)?;

    run_with_env(
        "nix",
        &["copy", "--from", &store, &output, "--no-check-sigs"],
        &nix_env,
    )
    .await
    .with_context(|| format!("copy from {build_host}"))?;

    Ok(output)
}

fn build_output_path(results: Vec<BuildResult>) -> Result<String> {
    results
        .into_iter()
        .next()
        .and_then(|result| result.outputs.get("out").cloned())
        .context("build result missing 'out' output")
}

pub async fn deploy_closure(
    spec: &JobSpec,
    output_path: &str,
    operation: Operation,
    host_key_policy: HostKeyPolicy,
) -> Result<()> {
    let target = spec.target();
    copy_closure(output_path, &target, host_key_policy).await?;

    let activation_unit = format!("blzrd-activate-{}", Uuid::new_v4().simple());
    let commands = activation_commands(spec, output_path, operation, &activation_unit)?;
    run_activation_commands(&target, &commands, host_key_policy).await?;

    if matches!(
        (spec.system, operation),
        (SystemType::Nixos, Operation::Switch)
    ) {
        poll_activation(&target, &activation_unit, host_key_policy)
            .await
            .with_context(|| format!("activation on {target}"))?;
    }

    Ok(())
}

async fn copy_closure(
    output_path: &str,
    target: &str,
    host_key_policy: HostKeyPolicy,
) -> Result<()> {
    let nix_ssh_options = nix_ssh_options(host_key_policy);
    let nix_env = [("NIX_SSHOPTS", nix_ssh_options.as_str())];
    let remote_store = format!("ssh-ng://{target}");

    run_with_env(
        "nix",
        &[
            "copy",
            "--to",
            &remote_store,
            output_path,
            "--no-check-sigs",
        ],
        &nix_env,
    )
    .await
    .with_context(|| format!("copy to {target}"))?;

    Ok(())
}

async fn run_activation_commands(
    target: &str,
    commands: &[RemoteCommand],
    host_key_policy: HostKeyPolicy,
) -> Result<()> {
    for command in commands {
        let args: Vec<_> = command.arguments.iter().map(String::as_str).collect();
        run_ssh(target, &command.program, &args, host_key_policy)
            .await
            .with_context(|| format!("activation on {target}"))?;
    }

    Ok(())
}

fn activation_commands(
    spec: &JobSpec,
    output_path: &str,
    operation: Operation,
    activation_unit: &str,
) -> Result<Vec<RemoteCommand>> {
    let update_profile = root_command(
        spec,
        "/run/current-system/sw/bin/nix-env",
        vec![
            "-p".to_owned(),
            "/nix/var/nix/profiles/system".to_owned(),
            "--set".to_owned(),
            output_path.to_owned(),
        ],
    );

    let activate = match (spec.system, operation) {
        (SystemType::Darwin, Operation::Switch) => {
            root_command(spec, &format!("{output_path}/activate"), Vec::new())
        }
        (SystemType::Darwin, Operation::Boot) => {
            anyhow::bail!(
                "job {}: 'boot' is not a valid darwin operation",
                spec.hostname
            );
        }
        (SystemType::Nixos, Operation::Switch) => root_command(
            spec,
            "/run/current-system/sw/bin/systemd-run",
            vec![
                "--unit".to_owned(),
                activation_unit.to_owned(),
                "--remain-after-exit".to_owned(),
                "--no-block".to_owned(),
                "--".to_owned(),
                format!("{output_path}/bin/switch-to-configuration"),
                operation.to_string(),
            ],
        ),
        (SystemType::Nixos, Operation::Boot) => root_command(
            spec,
            &format!("{output_path}/bin/switch-to-configuration"),
            vec![operation.to_string()],
        ),
    };

    Ok(vec![update_profile, activate])
}

async fn poll_activation(target: &str, unit: &str, host_key_policy: HostKeyPolicy) -> Result<()> {
    let mut sleep_duration = Duration::from_secs(5);
    let deadline = Instant::now() + Duration::from_mins(5);

    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("activation on {target}: timed out waiting for {unit} unit");
        }

        let result = run_ssh(target, "systemctl", &["show", unit], host_key_policy)
            .await
            .ok();
        let (sub_state, exit_status) = result
            .as_deref()
            .map(parse_systemd_state)
            .unwrap_or_default();

        match (sub_state.as_deref(), exit_status.unwrap_or(0)) {
            (Some("exited"), 0) => break,
            (Some("exited"), status) => {
                anyhow::bail!(
                    "activation on {target}: switch-to-configuration exited with code {status}"
                );
            }
            (Some("failed"), _) => {
                anyhow::bail!(
                    "activation on {target}: unit failed (ExecMainStatus={})",
                    exit_status.unwrap_or(-1)
                );
            }
            _ => {
                sleep(sleep_duration).await;
                sleep_duration = (sleep_duration * 2).min(Duration::from_mins(1));
            }
        }
    }

    Ok(())
}

fn parse_systemd_state(output: &[u8]) -> (Option<String>, Option<i32>) {
    let mut sub_state = None;
    let mut exit_status = None;

    for line in String::from_utf8_lossy(output).lines() {
        if let Some(value) = line.strip_prefix("SubState=") {
            sub_state = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("ExecMainStatus=") {
            exit_status = value.trim().parse().ok();
        }
    }

    (sub_state, exit_status)
}

fn nix_ssh_options(host_key_policy: HostKeyPolicy) -> String {
    let host_key_option = host_key_policy.nix_ssh_option();
    match std::env::var("NIX_SSHOPTS") {
        Ok(existing) if !existing.trim().is_empty() => format!("{existing} {host_key_option}"),
        _ => host_key_option.to_owned(),
    }
}

struct RemoteCommand {
    program: String,
    arguments: Vec<String>,
}

fn root_command(spec: &JobSpec, program: &str, arguments: Vec<String>) -> RemoteCommand {
    if spec.user == "root" {
        return RemoteCommand {
            program: program.to_owned(),
            arguments,
        };
    }

    let mut sudo_arguments = Vec::with_capacity(arguments.len() + 2);
    sudo_arguments.push("-n".to_owned());
    sudo_arguments.push(program.to_owned());
    sudo_arguments.extend(arguments);
    RemoteCommand {
        program: "sudo".to_owned(),
        arguments: sudo_arguments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(user: &str) -> JobSpec {
        JobSpec {
            hostname: "host".to_owned(),
            system: SystemType::Nixos,
            user: user.to_owned(),
            drv_path: "/nix/store/example.drv".to_owned(),
        }
    }

    #[test]
    fn root_command_runs_directly_for_root() {
        let command = root_command(&job("root"), "activate", vec!["switch".to_owned()]);

        assert_eq!(command.program, "activate");
        assert_eq!(command.arguments, ["switch"]);
    }

    #[test]
    fn root_command_uses_noninteractive_sudo_for_other_users() {
        let command = root_command(&job("deploy"), "activate", vec!["switch".to_owned()]);

        assert_eq!(command.program, "sudo");
        assert_eq!(command.arguments, ["-n", "activate", "switch"]);
    }
}
