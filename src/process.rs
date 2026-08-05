use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

pub async fn run_with_env(cmd: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<Vec<u8>> {
    let display = format!("{} {}", cmd, args.join(" "));

    let mut command = Command::new(cmd);
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }

    let output = command
        .output()
        .await
        .with_context(|| format!("failed to spawn {display}"))?;

    let std_out = String::from_utf8_lossy(&output.stdout).into_owned();
    let std_err = String::from_utf8_lossy(&output.stderr).into_owned();
    let was_success = output.status.success();

    log_debug(&display, &std_out, &std_err);

    if !was_success {
        let detail = if std_err.trim().is_empty() {
            &std_out
        } else {
            &std_err
        };
        anyhow::bail!("{display} failed: {}", detail.trim());
    }

    Ok(output.stdout)
}

/// Run `cmd args...` and deserialize the JSON output into a struct of type `T`.
pub async fn run_json<T: DeserializeOwned>(cmd: &str, args: &[&str]) -> Result<T> {
    run_json_with_env(cmd, args, &[]).await
}

/// Run a command that emits one JSON value per line, notifying the caller as
/// each value becomes available.
pub async fn run_json_lines<T, OnLine>(
    cmd: &str,
    args: &[&str],
    mut on_line: OnLine,
) -> Result<Vec<T>>
where
    T: DeserializeOwned,
    OnLine: FnMut(&T),
{
    let display = format!("{} {}", cmd, args.join(" "));

    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to spawn {display}"))?;
    let stdout = child
        .stdout
        .take()
        .with_context(|| format!("failed to capture stdout from {display}"))?;
    let stderr = child
        .stderr
        .take()
        .with_context(|| format!("failed to capture stderr from {display}"))?;

    let stderr_task = tokio::spawn(async move {
        let mut text = String::new();
        BufReader::new(stderr).read_to_string(&mut text).await?;
        Ok::<String, std::io::Error>(text)
    });

    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stdout_text = String::new();
    let mut values = Vec::new();
    while let Some(line) = stdout_lines
        .next_line()
        .await
        .with_context(|| format!("failed to read stdout from {display}"))?
    {
        if line.trim().is_empty() {
            continue;
        }

        stdout_text.push_str(&line);
        stdout_text.push('\n');
        let value: T = serde_json::from_str(&line)
            .with_context(|| format!("invalid JSON from {display}: {line}"))?;
        on_line(&value);
        values.push(value);
    }

    let status = child
        .wait()
        .await
        .with_context(|| format!("failed waiting for {display}"))?;
    let stderr_text = stderr_task
        .await
        .context("stderr reader task failed")?
        .with_context(|| format!("failed to read stderr from {display}"))?;

    log_debug(&display, &stdout_text, &stderr_text);

    if !status.success() {
        let detail = if stderr_text.trim().is_empty() {
            &stdout_text
        } else {
            &stderr_text
        };
        anyhow::bail!("{display} failed: {}", detail.trim());
    }

    Ok(values)
}

pub async fn run_json_with_env<T: DeserializeOwned>(
    cmd: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<T> {
    let display = format!("{} {}", cmd, args.join(" "));
    let output = run_with_env(cmd, args, envs).await?;
    serde_json::from_slice(&output).with_context(|| format!("invalid JSON from {display}"))
}

/// Emit subprocess details at the debug log level (only visible with `--debug`).
pub(crate) fn log_debug(command: &str, std_out: &str, std_err: &str) {
    log::debug!("$ {command}");
    if !std_out.trim().is_empty() {
        log::debug!("stdout:\n{}", std_out.trim());
    }
    if !std_err.trim().is_empty() {
        log::debug!("stderr:\n{}", std_err.trim());
    }
}
