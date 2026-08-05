mod cli;
mod models;
mod nix;
mod op;
mod process;
mod ssh;
mod ui;
mod workflow;

use crate::cli::Command;
use crate::nix::EvaluationProgress;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = cli::parse_cli();

    // Logging level comes from `RUST_LOG` (default `info`). Set
    // `RUST_LOG=debug` to see every subprocess command and its output.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    let ui = ui::Ui::new();

    let mut evaluation = ui.start_section("Evaluating");
    let eval_result = nix::eval_deployments(&args.flake, |progress| match progress {
        EvaluationProgress::JobEvaluated { name } => {
            ui.add_job(&mut evaluation, &name, &name);
            ui.start_job(&evaluation, &name, &name);
        }
        EvaluationProgress::MetadataResolved { name } => {
            ui.finish_job_success(ui.job_progress(&evaluation, &name), &name);
        }
    })
    .await;

    let jobs = match eval_result {
        Ok(result) => {
            ui.finish_section_success(evaluation);
            result
        }
        Err(error) => {
            ui.finish_section_failure(evaluation);
            return Err(error);
        }
    };

    match args.command {
        Command::List => {
            ui.print_nodes(&jobs);
            return Ok(());
        }
        cmd @ (Command::Switch(_) | Command::Boot(_)) => {
            let (op, common) = cmd.into_deploy().expect("deploy command");
            workflow::run_deploy(op, common, jobs, &ui).await?;
        }
    }

    Ok(())
}
