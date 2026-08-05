mod deploy;
mod evaluate;

pub use deploy::{build_closure, deploy_closure};
pub use evaluate::{eval_deployments, EvaluationProgress};
