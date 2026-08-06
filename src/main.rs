mod cli;
mod models;
mod nix;
mod op;
mod process;
mod ssh;
mod ui;
mod workflow;

use crate::nix::EvaluationProgress;
use crate::ui::Ui;

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
            ui.finish_job_success(Ui::job_progress(&evaluation, &name), &name);
        }
    })
    .await;

    let jobs = match eval_result {
        Ok(result) => {
            ui.finish_section_success(&evaluation);
            result
        }
        Err(error) => {
            ui.finish_section_failure(&evaluation);
            return Err(error);
        }
    };

    let (op, common) = args.command.into_deploy();
    workflow::run_deploy(op, common, jobs, &ui).await?;

    Ok(())
}
