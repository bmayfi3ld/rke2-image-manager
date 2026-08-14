use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

use crate::models::Config;

pub mod commands;
pub mod output;
pub mod select;

use output::OutputOptions;

#[derive(Parser)]
#[command(
    name = "rke2-image-manager",
    version,
    about = "Manage custom container images across RKE2 cluster nodes"
)]
pub struct Cli {
    /// Override config discovery
    #[arg(short, long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Machine-readable output
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress progress/log chatter; only final results and errors
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable ANSI styling (also honors the NO_COLOR env var)
    #[arg(long = "no-color", global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Launch the interactive TUI (default when no subcommand is given)
    Tui,
    /// List managed images across all servers
    List(commands::list::ListArgs),
    /// Show per-server reachability and aggregate counts
    Status,
    /// List configured servers
    Servers,
    /// Build image(s) and save the tarball to staging
    Build(commands::build::BuildArgs),
    /// Copy tarball(s) to servers
    Deploy(commands::deploy::DeployArgs),
    /// Delete specific tarballs from servers
    Remove(commands::remove::RemoveArgs),
    /// Bulk sweep of stale/unknown tarballs from servers
    Clean(commands::clean::CleanArgs),
    /// Generate shell completion scripts
    #[command(name = "__completions", hide = true)]
    Completions { shell: clap_complete::Shell },
}

/// Dispatch a non-TUI subcommand. Callers must handle `None`/`Some(Command::Tui)`
/// themselves and route those to `tui::run` instead.
pub async fn run(cli: Cli, config: Config) -> Result<ExitCode> {
    let opts = OutputOptions::new(cli.json, cli.quiet, cli.no_color);

    match cli.command {
        Some(Command::List(args)) => commands::list::run(&config, &args, &opts).await,
        Some(Command::Status) => commands::status::run(&config, &opts).await,
        Some(Command::Servers) => commands::servers::run(&config, &opts),
        Some(Command::Build(args)) => commands::build::run(&config, &args, &opts).await,
        Some(Command::Deploy(args)) => commands::deploy::run(&config, &args, &opts).await,
        Some(Command::Remove(args)) => commands::remove::run(&config, &args, &opts).await,
        Some(Command::Clean(args)) => commands::clean::run(&config, &args, &opts).await,
        Some(Command::Tui) | Some(Command::Completions { .. }) | None => {
            unreachable!("Tui/Completions/None must be handled by the caller before cli::run")
        }
    }
}
