use clap::{Args, Parser, Subcommand};

use crate::op::Operation;

/// Blazing fast fleet deployments for NixOS and nix-darwin.
#[derive(Parser, Debug)]
#[command(name = "blzrd", author, version, about)]
pub struct Cli {
    /// Flake URL or path (e.g. `github:alyraffauf/infra` or `.`).
    #[arg(short, long, env = "FLAKE", default_value = ".", global = true)]
    pub flake: String,

    #[command(subcommand)]
    pub command: Command,
}

/// Arguments shared by every deployment operation.
#[derive(Args, Debug, Clone)]
pub struct CommonArgs {
    /// Nodes to deploy (default: all).
    #[arg(value_delimiter = ',', num_args = 0..)]
    pub nodes: Vec<String>,

    /// Nodes to skip.
    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<String>,

    /// Build closures on a remote host instead of locally.
    #[arg(long, default_value = "localhost")]
    pub build_host: String,

    /// Accept and record previously unknown SSH host keys.
    #[arg(long)]
    pub accept_new_host_keys: bool,

    /// Set simultaneous deployments.
    #[arg(long, default_value_t = 5)]
    pub parallel: usize,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Activate the new configuration and make it the boot default.
    Switch(CommonArgs),
    /// Set the new configuration as the boot default without activating it.
    Boot(CommonArgs),
}

impl Command {
    /// Map a deployment command into its logical `Operation` and shared args.
    pub fn into_deploy(self) -> (Operation, CommonArgs) {
        match self {
            Command::Switch(args) => (Operation::Switch, args),
            Command::Boot(args) => (Operation::Boot, args),
        }
    }
}

pub fn parse_cli() -> Cli {
    Cli::parse()
}
