use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::models::{JobSpec, NixEvalJobsResult, SystemType};
use crate::process::{get_config_attr, run_json_lines};

#[derive(Debug, Clone)]
pub enum EvaluationProgress {
    JobEvaluated { name: String },
    MetadataResolved { name: String },
}

/// Evaluate the flake's `blzrd.nodes` and return enriched `JobSpec`s.
pub async fn eval_deployments<OnProgress>(
    cfg: &str,
    mut on_progress: OnProgress,
) -> Result<HashMap<String, JobSpec>>
where
    OnProgress: FnMut(EvaluationProgress),
{
    let results = run_eval_jobs(cfg, &mut on_progress).await?;
    let basics = collect_basics(&results)?;
    build_job_specs(cfg, &results, &basics, &mut on_progress).await
}

/// Run `nix-eval-jobs` on the flake's `blzrd.nodes`.
async fn run_eval_jobs<OnProgress>(
    cfg: &str,
    on_progress: &mut OnProgress,
) -> Result<Vec<NixEvalJobsResult>>
where
    OnProgress: FnMut(EvaluationProgress),
{
    let flake_reference = format!("{cfg}#blzrd.nodes");
    let gc_roots_dir = gc_roots_dir()?;

    run_json_lines(
        "nix-eval-jobs",
        &[
            "--gc-roots-dir",
            &gc_roots_dir.to_string_lossy(),
            "--force-recurse",
            "--flake",
            &flake_reference,
        ],
        |result: &NixEvalJobsResult| {
            let name = result
                .attr_path
                .as_ref()
                .and_then(|path| path.first())
                .cloned()
                .unwrap_or_else(|| result.attr.clone());
            on_progress(EvaluationProgress::JobEvaluated { name });
        },
    )
    .await
}

fn gc_roots_dir() -> Result<PathBuf> {
    let cache_home = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|path| !path.is_empty())
        .unwrap_or_else(|| {
            let home =
                std::env::var("HOME").expect("HOME must be set if XDG_CACHE_HOME is not set");
            format!("{home}/.cache")
        });
    let gc_roots_dir = PathBuf::from(cache_home).join("blzrd");

    std::fs::create_dir_all(&gc_roots_dir)
        .with_context(|| format!("failed to create gc-roots dir {}", gc_roots_dir.display()))?;

    Ok(gc_roots_dir)
}

/// First pass: extract the basic `(output, drv_path)` pair for each job.
fn collect_basics(results: &[NixEvalJobsResult]) -> Result<HashMap<String, (String, String)>> {
    let mut basics = HashMap::with_capacity(results.len());
    for result in results {
        if let Some(error) = &result.error {
            anyhow::bail!("job {}: {error}", result.attr);
        }

        let attr_path = result
            .attr_path
            .as_ref()
            .with_context(|| format!("job {}: missing attr_path", result.attr))?;
        if attr_path.len() < 2 {
            anyhow::bail!("job {}: malformed attr_path: {attr_path:?}", result.attr);
        }
        let job_name = attr_path[0].clone();

        let outputs = result
            .outputs
            .as_ref()
            .with_context(|| format!("job {job_name}: missing outputs"))?;
        let output_path = outputs
            .get("out")
            .with_context(|| format!("job {job_name}: missing 'out' output"))?;

        basics.insert(
            job_name,
            (
                output_path.clone(),
                result.drv_path.clone().unwrap_or_default(),
            ),
        );
    }
    Ok(basics)
}

/// Second pass: enrich each job's basic data with hostname, user, and system type.
async fn build_job_specs(
    cfg: &str,
    results: &[NixEvalJobsResult],
    basics: &HashMap<String, (String, String)>,
    on_progress: &mut impl FnMut(EvaluationProgress),
) -> Result<HashMap<String, JobSpec>> {
    let mut jobs = HashMap::with_capacity(basics.len());
    let mut names: Vec<_> = basics.keys().cloned().collect();
    names.sort();

    for name in names {
        let (output, drv_path) = basics.get(&name).cloned().unwrap_or_default();
        let hostname = configured_hostname(cfg, &name).await;
        let user = configured_value(cfg, &name, "user").await;
        let system = configured_system(cfg, &name, results).await?;

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
        on_progress(EvaluationProgress::MetadataResolved { name });
    }

    Ok(jobs)
}

async fn configured_hostname(cfg: &str, name: &str) -> String {
    let hostname = configured_value(cfg, name, "hostname").await;
    if hostname.is_empty() {
        name.to_owned()
    } else {
        hostname
    }
}

async fn configured_value(cfg: &str, name: &str, attribute: &str) -> String {
    get_config_attr(cfg, name, attribute)
        .await
        .unwrap_or_default()
}

async fn configured_system(
    cfg: &str,
    name: &str,
    results: &[NixEvalJobsResult],
) -> Result<SystemType> {
    let configured_type = configured_value(cfg, name, "type").await;
    let type_name = if configured_type.is_empty() {
        infer_system_type(name, results)?
    } else {
        configured_type
    };

    SystemType::parse(&type_name).map_err(|error| anyhow::anyhow!("job {name}: {error}"))
}

fn infer_system_type(name: &str, results: &[NixEvalJobsResult]) -> Result<String> {
    let system = results
        .iter()
        .find(|result| {
            result
                .attr_path
                .as_deref()
                .and_then(|path| path.first())
                .is_some_and(|job_name| job_name == name)
        })
        .and_then(|result| result.system.as_deref())
        .unwrap_or_default();

    if system.contains("darwin") {
        Ok("darwin".to_string())
    } else if system.contains("linux") {
        Ok("nixos".to_string())
    } else {
        anyhow::bail!("job {name}: unknown system type: {system}");
    }
}
