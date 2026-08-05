mod cli;
mod models;
mod nix;
mod op;
mod process;
mod ssh;

use std::collections::HashSet;

use futures::stream::{self, StreamExt};

use crate::cli::{Command, CommonArgs};
use crate::ssh::HostKeyPolicy;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = cli::parse_cli();

    // Logging level comes from `RUST_LOG` (default `info`). Set
    // `RUST_LOG=debug` to see every subprocess command and its output.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    let start = std::time::Instant::now();

    log::info!("Reading nodes from {}", args.flake);
    let (jobs, _debug) = nix::eval_deployments(&args.flake).await?;

    match args.command {
        Command::List => {
            print_nodes(&jobs);
            return Ok(());
        }
        cmd @ (Command::Switch(_) | Command::Boot(_)) => {
            let (op, common) = cmd.into_deploy().expect("deploy command");
            run_deploy(op, common, jobs, start).await?;
        }
    }

    Ok(())
}

/// Apply the `--skip` and `nodes` filters to the evaluated job map.
fn filter_jobs(
    mut jobs: std::collections::HashMap<String, crate::models::JobSpec>,
    common: &CommonArgs,
) -> anyhow::Result<std::collections::HashMap<String, crate::models::JobSpec>> {
    let all_names: HashSet<&String> = jobs.keys().collect();

    for s in &common.skip {
        if !all_names.contains(&s) {
            log::warn!("ignoring unknown node '{s}' in --skip");
        }
    }

    if !common.nodes.is_empty() {
        for n in &common.nodes {
            if !all_names.contains(&n) {
                anyhow::bail!("node '{n}' not found in flake");
            }
        }
    }

    jobs.retain(|name, _| !common.skip.iter().any(|s| s == name));
    if !common.nodes.is_empty() {
        jobs.retain(|name, _| common.nodes.iter().any(|n| n == name));
    }

    Ok(jobs)
}

/// Print the resolved node list with their system/user/host.
fn print_nodes(jobs: &std::collections::HashMap<String, crate::models::JobSpec>) {
    log::info!("Found {} node(s):", jobs.len());
    for (name, spec) in jobs {
        log::info!(
            "  {name} -> system={} user={} host={}",
            spec.system,
            spec.user,
            spec.hostname,
        );
    }
}

/// Filter, validate, build, and deploy the jobs for the given operation.
async fn run_deploy(
    op: crate::op::Operation,
    common: CommonArgs,
    jobs: std::collections::HashMap<String, crate::models::JobSpec>,
    start: std::time::Instant,
) -> anyhow::Result<()> {
    let jobs = filter_jobs(jobs, &common)?;
    print_nodes(&jobs);

    op::validate(&jobs, op)?;

    log::info!("Operation {op} is valid for all nodes");

    let host_key_policy = if common.accept_new_host_keys {
        HostKeyPolicy::AcceptNew
    } else {
        HostKeyPolicy::Strict
    };

    // Build each node's closure locally or on a remote builder.
    log::info!("Building {} output(s)...", jobs.len());
    let mut outs: std::collections::HashMap<String, String> =
        std::collections::HashMap::with_capacity(jobs.len());
    for (name, spec) in &jobs {
        let (out, _debug) = nix::build_closure(spec, &common.build_host, host_key_policy).await?;
        log::info!(" ✔ {name} ({})", spec.system);
        outs.insert(name.clone(), out);
    }

    log::info!("Deploying {} output(s)...", jobs.len());

    // One future per node, run concurrently.
    let tasks: Vec<_> = jobs
        .iter()
        .map(|(name, spec)| {
            let out = outs[name].clone();
            let name = name.clone();
            let spec = spec.clone();
            async move {
                let result = nix::deploy_closure(&spec, &out, op, host_key_policy).await;
                (name, spec, result)
            }
        })
        .collect();

    let results = stream::iter(tasks)
        .buffer_unordered(common.parallel)
        .collect::<Vec<_>>()
        .await;

    let errors: Vec<_> = results
        .into_iter()
        .filter_map(|(name, spec, result)| match result {
            Ok(_) => {
                log::info!(
                    " ✔ {name} ({}) -> {}@{}",
                    spec.system,
                    spec.user,
                    spec.hostname
                );
                None
            }
            Err(e) => {
                let target = spec.target();
                log::warn!("Failed to deploy to {target}: {e}");
                Some(e)
            }
        })
        .collect();

    let duration = start.elapsed();

    if !errors.is_empty() {
        log::info!(
            "Deployment failed with {} error(s) ({duration:?})",
            errors.len()
        );
        anyhow::bail!("deployment failed");
    }

    log::info!("Completed successfully in {duration:?}");
    Ok(())
}
