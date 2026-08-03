use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use tokio::process::Command;

use crate::models::DebugInfo;

pub async fn run(cmd: &str, args: &[&str]) -> Result<(Vec<u8>, DebugInfo)> {
    let display = format!("{} {}", cmd, args.join(" "));

    let output = Command::new(cmd)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to spawn {display}"))?;

    let std_out = String::from_utf8_lossy(&output.stdout).into_owned();
    let std_err = String::from_utf8_lossy(&output.stderr).into_owned();
    let was_success = output.status.success();

    let debug = DebugInfo {
        command: display.clone(),
        std_out: std_out.clone(),
        std_err: std_err.clone(),
    };

    log_debug(&debug);

    if !was_success {
        let detail = if std_err.trim().is_empty() {
            &std_out
        } else {
            &std_err
        };
        anyhow::bail!("{display} failed: {}", detail.trim());
    }

    // Return stdout only — stderr stays in `debug` for logging.
    Ok((output.stdout, debug))
}

/// Run `cmd args...` and deserialize the JSON output into a struct of type `T`.
pub async fn run_json<T: DeserializeOwned>(cmd: &str, args: &[&str]) -> Result<(T, DebugInfo)> {
    let display = format!("{} {}", cmd, args.join(" "));

    let output = Command::new(cmd)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to spawn {display}"))?;

    let std_out = String::from_utf8_lossy(&output.stdout).into_owned();
    let std_err = String::from_utf8_lossy(&output.stderr).into_owned();
    let was_success = output.status.success();

    let debug = DebugInfo {
        command: display.clone(),
        std_out: std_out.clone(),
        std_err: std_err.clone(),
    };

    log_debug(&debug);

    if !was_success {
        let detail = if std_err.trim().is_empty() {
            &std_out
        } else {
            &std_err
        };
        anyhow::bail!("{display} failed: {}", detail.trim());
    }

    let value: T =
        serde_json::from_str(&std_out).with_context(|| format!("invalid JSON from {display}"))?;

    Ok((value, debug))
}

/// Emit a `DebugInfo` at the debug log level (only visible with `--debug`).
fn log_debug(debug: &DebugInfo) {
    log::debug!("$ {}", debug.command);
    if !debug.std_out.trim().is_empty() {
        log::debug!("stdout:\n{}", debug.std_out.trim());
    }
    if !debug.std_err.trim().is_empty() {
        log::debug!("stderr:\n{}", debug.std_err.trim());
    }
}

pub async fn get_config_attr(cfg: &str, job: &str, attr: &str) -> Result<(String, DebugInfo)> {
    let attr_path = format!("{cfg}#blzrd.nodes.{job}.{attr}");
    let (value, debug): (String, _) = run_json("nix", &["eval", "--json", &attr_path]).await?;
    Ok((value, debug))
}
