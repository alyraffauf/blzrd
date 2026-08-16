use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::models::{JobSpec, NixEvalJobsResult, NodeMetadata, SystemType};
use crate::process::run_json_lines;

const SELECT_METADATA: &str = "output: output.blzrdMetadata";

/// Evaluate the flake's `blzrd.nodes` and return enriched `JobSpec`s.
pub async fn eval_deployments<OnJobCompleted>(
    cfg: &str,
    selected_nodes: &[String],
    mut on_job_completed: OnJobCompleted,
) -> Result<HashMap<String, JobSpec>>
where
    OnJobCompleted: FnMut(String),
{
    let results = run_eval_jobs(cfg, selected_nodes).await?;
    collect_job_specs(&results, &mut on_job_completed)
}

/// Run `nix-eval-jobs` on the flake's `blzrd.nodes`.
async fn run_eval_jobs(cfg: &str, selected_nodes: &[String]) -> Result<Vec<NixEvalJobsResult>> {
    let flake_reference = format!("{cfg}#blzrd.nodes");
    let gc_roots_dir = gc_roots_dir()?;
    let deployment_selector = deployment_selector(selected_nodes);

    run_json_lines(
        "nix-eval-jobs",
        &[
            "--gc-roots-dir",
            &gc_roots_dir.to_string_lossy(),
            "--force-recurse",
            "--flake",
            &flake_reference,
            "--select",
            &deployment_selector,
            "--apply",
            SELECT_METADATA,
        ],
        |_: &NixEvalJobsResult| {},
    )
    .await
}

fn deployment_selector(selected_nodes: &[String]) -> String {
    let serialized_node_names = selected_nodes
        .iter()
        .map(|name| serde_json::to_string(name).expect("node names must serialize as JSON"))
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        r#"
nodes:
let
  selectedNames = [ {serialized_node_names} ];
  requestedNodes = builtins.listToAttrs (
    map (name: {{ inherit name; value = null; }}) selectedNames
  );
  nodesToDeploy =
    if selectedNames == [ ]
    then nodes
    else builtins.intersectAttrs requestedNodes nodes;
in
builtins.mapAttrs (_: node:
  node.output // {{
    blzrdMetadata = {{
      hostname = node.hostname or null;
      type = node.type or "";
      user = node.user or "";
    }};
  }}
) nodesToDeploy
"#
    )
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

fn collect_job_specs(
    results: &[NixEvalJobsResult],
    on_job_completed: &mut impl FnMut(String),
) -> Result<HashMap<String, JobSpec>> {
    let mut sorted_results: Vec<_> = results.iter().collect();
    sorted_results.sort_by(|left, right| left.attr.cmp(&right.attr));

    let mut jobs = HashMap::with_capacity(results.len());
    for result in sorted_results {
        if let Some(error) = &result.error {
            anyhow::bail!("job {}: {error}", result.attr);
        }

        let attr_path = result
            .attr_path
            .as_ref()
            .with_context(|| format!("job {}: missing attr_path", result.attr))?;
        if attr_path.len() != 1 {
            anyhow::bail!("job {}: malformed attr_path: {attr_path:?}", result.attr);
        }
        let name = attr_path[0].clone();

        let outputs = result
            .outputs
            .as_ref()
            .with_context(|| format!("job {name}: missing outputs"))?;
        outputs
            .get("out")
            .with_context(|| format!("job {name}: missing 'out' output"))?;

        let metadata = result.extra_value.clone().unwrap_or_default();
        let hostname = metadata.hostname_or_name(&name);
        let user = metadata.user();
        let system = metadata.system_type(&name, result.system.as_deref())?;
        let drv_path = result.drv_path.clone().unwrap_or_default();

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
        on_job_completed(name);
    }

    Ok(jobs)
}

fn infer_system_type(name: &str, system: Option<&str>) -> Result<String> {
    let system = system.unwrap_or_default();
    if system.contains("darwin") {
        Ok("darwin".to_string())
    } else if system.contains("linux") {
        Ok("nixos".to_string())
    } else {
        anyhow::bail!("job {name}: unknown system type: {system}");
    }
}

impl NodeMetadata {
    fn hostname_or_name(&self, name: &str) -> String {
        self.hostname
            .as_deref()
            .filter(|hostname| !hostname.is_empty())
            .unwrap_or(name)
            .to_owned()
    }

    fn user(&self) -> String {
        self.user.clone().unwrap_or_default()
    }

    fn system_type(&self, name: &str, system: Option<&str>) -> Result<SystemType> {
        let configured_type = self
            .type_name
            .as_deref()
            .filter(|type_name| !type_name.is_empty())
            .map(str::to_owned);
        let type_name = match configured_type {
            Some(type_name) => type_name,
            None => infer_system_type(name, system)?,
        };

        SystemType::parse(&type_name).map_err(|error| anyhow::anyhow!("job {name}: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_selector_quotes_selected_node_names() {
        let selector = deployment_selector(&["mauville".to_string(), "quoted\"node".to_string()]);

        assert!(selector.contains(r#"selectedNames = [ "mauville" "quoted\"node" ];"#));
    }

    #[test]
    fn collect_job_specs_uses_embedded_metadata() {
        let results = [NixEvalJobsResult {
            attr: "mauville".to_string(),
            attr_path: Some(vec!["mauville".to_string()]),
            drv_path: Some("/nix/store/mauville.drv".to_string()),
            error: None,
            extra_value: Some(NodeMetadata {
                hostname: None,
                type_name: Some("nixos".to_string()),
                user: Some("root".to_string()),
            }),
            outputs: Some(HashMap::from([(
                "out".to_string(),
                "/nix/store/mauville".to_string(),
            )])),
            system: Some("x86_64-linux".to_string()),
        }];
        let mut completed = Vec::new();

        let jobs = collect_job_specs(&results, &mut |name| completed.push(name)).unwrap();
        let mauville = &jobs["mauville"];

        assert_eq!(completed, ["mauville"]);
        assert_eq!(mauville.hostname, "mauville");
        assert_eq!(mauville.system, SystemType::Nixos);
        assert_eq!(mauville.user, "root");
        assert_eq!(mauville.drv_path, "/nix/store/mauville.drv");
    }

    #[test]
    fn node_metadata_accepts_a_null_hostname() {
        let metadata: NodeMetadata =
            serde_json::from_str(r#"{"hostname":null,"type":"nixos","user":"root"}"#).unwrap();

        assert_eq!(metadata.hostname_or_name("jubilife"), "jubilife");
        assert_eq!(metadata.user(), "root");
    }
}
