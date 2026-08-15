use std::collections::HashMap;

use anyhow::Context;
use futures::stream::{self, StreamExt};

use crate::cli::CommonArgs;
use crate::models::JobSpec;
use crate::nix;
use crate::op::{self, Operation};
use crate::ssh::HostKeyPolicy;
use crate::ui::Ui;

/// Filter, validate, build, and deploy the jobs for the given operation.
pub async fn run_deploy(
    operation: Operation,
    common: CommonArgs,
    jobs: HashMap<String, JobSpec>,
    ui: &Ui,
) -> anyhow::Result<()> {
    let jobs = prepare_jobs(jobs, &common, operation)?;
    let host_key_policy = host_key_policy(&common);
    let closures = build_closures(&jobs, &common, host_key_policy, ui).await?;
    deploy_closures(
        jobs,
        closures,
        common.parallel,
        operation,
        host_key_policy,
        ui,
    )
    .await
}

fn prepare_jobs(
    jobs: HashMap<String, JobSpec>,
    common: &CommonArgs,
    operation: Operation,
) -> anyhow::Result<HashMap<String, JobSpec>> {
    let (jobs, unknown_skips) = filter_jobs(jobs, common)?;
    for name in unknown_skips {
        Ui::print_warning(format!("ignoring unknown node '{name}' in --skip"));
    }

    op::validate(&jobs, operation)?;
    Ok(jobs)
}

/// Apply the `--skip` and `nodes` filters to the evaluated job map.
fn filter_jobs(
    mut jobs: HashMap<String, JobSpec>,
    common: &CommonArgs,
) -> anyhow::Result<(HashMap<String, JobSpec>, Vec<String>)> {
    let unknown_skips = common
        .skip
        .iter()
        .filter(|skip_name| !jobs.contains_key(*skip_name))
        .cloned()
        .collect();

    for node_name in &common.nodes {
        if !jobs.contains_key(node_name) {
            anyhow::bail!("node '{node_name}' not found in flake");
        }
    }

    jobs.retain(|name, _| !common.skip.iter().any(|skip_name| skip_name == name));
    if !common.nodes.is_empty() {
        jobs.retain(|name, _| common.nodes.iter().any(|node_name| node_name == name));
    }

    Ok((jobs, unknown_skips))
}

fn host_key_policy(common: &CommonArgs) -> HostKeyPolicy {
    if common.accept_new_host_keys {
        HostKeyPolicy::AcceptNew
    } else {
        HostKeyPolicy::Strict
    }
}

async fn build_closures(
    jobs: &HashMap<String, JobSpec>,
    common: &CommonArgs,
    host_key_policy: HostKeyPolicy,
    ui: &Ui,
) -> anyhow::Result<HashMap<String, String>> {
    ui.start_section("Building");
    let names = sorted_job_names(jobs);

    let mut closures = HashMap::with_capacity(jobs.len());
    for name in names {
        let result = nix::build_closure(&jobs[&name], &common.build_host, host_key_policy).await;
        match result {
            Ok(closure) => {
                ui.finish_job_success(&name);
                closures.insert(name, closure);
            }
            Err(error) => {
                ui.finish_job_failure(&name, &error);
                ui.finish_section_failure();
                return Err(error).context(format!("building {name}"));
            }
        }
    }

    ui.finish_section_success();
    Ok(closures)
}

async fn deploy_closures(
    jobs: HashMap<String, JobSpec>,
    closures: HashMap<String, String>,
    parallelism: usize,
    operation: Operation,
    host_key_policy: HostKeyPolicy,
    ui: &Ui,
) -> anyhow::Result<()> {
    ui.start_section("Deploying");
    let jobs = sorted_jobs(jobs);

    let results = stream::iter(jobs)
        .map(|(name, spec)| {
            let closure = closures[&name].clone();
            async move {
                let label = deployment_label(&name, &spec);
                let result = nix::deploy_closure(&spec, &closure, operation, host_key_policy).await;
                match &result {
                    Ok(()) => ui.finish_job_success(&label),
                    Err(error) => ui.finish_job_failure(&label, error),
                }
                result
            }
        })
        .buffer_unordered(parallelism)
        .collect::<Vec<_>>()
        .await;

    if results.iter().any(Result::is_err) {
        ui.finish_section_failure();
        anyhow::bail!("deployment failed");
    }

    ui.finish_section_success();
    Ok(())
}

fn sorted_job_names(jobs: &HashMap<String, JobSpec>) -> Vec<String> {
    let mut names: Vec<_> = jobs.keys().cloned().collect();
    names.sort();
    names
}

fn sorted_jobs(jobs: HashMap<String, JobSpec>) -> Vec<(String, JobSpec)> {
    let mut jobs: Vec<_> = jobs.into_iter().collect();
    jobs.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
    jobs
}

fn deployment_label(name: &str, spec: &JobSpec) -> String {
    format!("{name} -> {}", spec.target())
}
