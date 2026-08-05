use std::collections::HashMap;
use std::fmt;

use crate::models::{JobSpec, SystemType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Operation {
    Switch,
    Boot,
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Operation::Switch => write!(f, "switch"),
            Operation::Boot => write!(f, "boot"),
        }
    }
}

pub fn validate(jobs: &HashMap<String, JobSpec>, op: Operation) -> anyhow::Result<()> {
    for (name, spec) in jobs {
        match (spec.system, op) {
            (SystemType::Darwin, Operation::Switch) => {}
            (SystemType::Darwin, Operation::Boot) => {
                anyhow::bail!("job {name}: 'boot' is not a valid darwin operation");
            }
            (SystemType::Nixos, Operation::Boot | Operation::Switch) => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JobSpec, SystemType};

    /// Build a minimal `JobSpec` with the given system type and default
    /// user/host values.
    fn job(system: SystemType) -> JobSpec {
        JobSpec {
            hostname: "host".into(),
            system,
            user: "deploy".into(),
            drv_path: "/nix/store/dummy-drv".into(),
        }
    }

    #[test]
    fn validate_nixos_switch_ok() {
        let mut jobs = HashMap::new();
        jobs.insert("nixos-host".into(), job(SystemType::Nixos));
        validate(&jobs, Operation::Switch).unwrap();
    }

    #[test]
    fn validate_darwin_boot_errors() {
        let mut jobs = HashMap::new();
        jobs.insert("mac".into(), job(SystemType::Darwin));
        assert!(validate(&jobs, Operation::Boot).is_err());
    }

    #[test]
    fn target_formats_user_at_host() {
        let spec = job(SystemType::Nixos);
        assert_eq!(spec.target(), "deploy@host");
    }
}
