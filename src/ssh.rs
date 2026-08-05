use std::time::Duration;

use anyhow::{Context, Result};
use openssh::{KnownHosts, SessionBuilder};

use crate::models::DebugInfo;
use crate::process::log_debug;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy)]
pub enum HostKeyPolicy {
    Strict,
    AcceptNew,
}

impl HostKeyPolicy {
    fn known_hosts(self) -> KnownHosts {
        match self {
            Self::Strict => KnownHosts::Strict,
            Self::AcceptNew => KnownHosts::Add,
        }
    }

    pub fn nix_ssh_option(self) -> &'static str {
        match self {
            Self::Strict => "-o StrictHostKeyChecking=yes",
            Self::AcceptNew => "-o StrictHostKeyChecking=accept-new",
        }
    }
}

/// Run one remote command over a fresh SSH session.
pub async fn run(
    target: &str,
    program: &str,
    args: &[&str],
    host_key_policy: HostKeyPolicy,
) -> Result<(Vec<u8>, DebugInfo)> {
    let display = format!("ssh {target} {program} {}", args.join(" "));

    let mut builder = SessionBuilder::default();
    builder
        .known_hosts_check(host_key_policy.known_hosts())
        .connect_timeout(CONNECT_TIMEOUT);

    let session = builder
        .connect(target)
        .await
        .with_context(|| format!("failed to connect to {target}"))?;

    let output_result = session
        .command(program)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to run {display}"));

    if let Err(error) = session.close().await {
        log::debug!("failed to close SSH session to {target}: {error}");
    }

    let output = output_result?;

    let std_out = String::from_utf8_lossy(&output.stdout).into_owned();
    let std_err = String::from_utf8_lossy(&output.stderr).into_owned();
    let debug = DebugInfo {
        command: display.clone(),
        std_out,
        std_err: std_err.clone(),
    };

    log_debug(&debug);

    if !output.status.success() {
        let detail = if std_err.trim().is_empty() {
            &debug.std_out
        } else {
            &debug.std_err
        };
        anyhow::bail!("{display} failed: {}", detail.trim());
    }

    Ok((output.stdout, debug))
}
