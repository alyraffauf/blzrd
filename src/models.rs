use serde::Deserialize;
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemType {
    Nixos,
    Darwin,
}

impl SystemType {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "nixos" => Ok(SystemType::Nixos),
            "darwin" => Ok(SystemType::Darwin),
            other => Err(format!("unsupported system type: {other}")),
        }
    }
}

impl fmt::Display for SystemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SystemType::Nixos => write!(f, "nixos"),
            SystemType::Darwin => write!(f, "darwin"),
        }
    }
}

/// Captured details of a subprocess invocation, for debug logging.
#[derive(Debug, Clone, Default)]
pub struct DebugInfo {
    pub command: String,
    pub std_out: String,
    pub std_err: String,
}

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub hostname: String,
    pub system: SystemType,
    pub user: String,
    pub drv_path: String,
}

impl JobSpec {
    /// The SSH target string (`user@hostname`) for this deployment.
    pub fn target(&self) -> String {
        format!("{}@{}", self.user, self.hostname)
    }
}

/// One entry from `nix build --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct BuildResult {
    pub outputs: HashMap<String, String>,
}

/// One JSON-Lines record emitted by `nix-eval-jobs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NixEvalJobsResult {
    pub attr: String,
    pub attr_path: Option<Vec<String>>,
    pub drv_path: Option<String>,
    pub error: Option<String>,
    pub outputs: Option<HashMap<String, String>>,
    pub system: Option<String>,
}
