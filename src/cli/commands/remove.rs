use anyhow::Result;
use clap::Args;
use std::process::ExitCode;

use crate::cli::output::OutputOptions;
use crate::cli::select::{self, ImageSelector};
use crate::inventory::load;
use crate::models::Config;
use crate::remote;

#[derive(Args)]
pub struct RemoveArgs {
    /// Targets: name (current version), name:version (a specific/stale version), or a bare .tar filename (unknown tarball)
    #[arg(required = true)]
    pub targets: Vec<String>,

    /// Restrict to these servers (repeatable); defaults to every configured server
    #[arg(short = 's', long = "server")]
    pub server: Vec<String>,

    /// Print the exact removal targets without touching anything
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run(config: &Config, args: &RemoveArgs, opts: &OutputOptions) -> Result<ExitCode> {
    let inv = load(config, true).await?;

    let mut resolved = Vec::new();
    for raw in &args.targets {
        let selector = ImageSelector::parse(raw);
        match select::resolve(&selector, &inv.families, &inv.unknowns) {
            Ok(target) => resolved.push(target),
            Err(e) => {
                eprintln!("Error: {}", e);
                return Ok(ExitCode::from(2));
            }
        }
    }

    let servers = match select::resolve_servers(&args.server, &config.servers) {
        Ok(servers) => servers,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(ExitCode::from(2));
        }
    };

    if args.dry_run {
        for target in &resolved {
            let tarball = target.tarball_filename();
            for server in &servers {
                println!(
                    "{}",
                    remote::preview_remove_command(server, &tarball, &config.paths.rke2_images_dir)
                );
            }
        }
        return Ok(ExitCode::SUCCESS);
    }

    let mut any_failed = false;
    for target in &resolved {
        let tarball = target.tarball_filename();
        for server in &servers {
            match remote::remove_tarball(server, &tarball, &config.paths.rke2_images_dir).await {
                Ok(()) => {
                    if !opts.quiet {
                        println!("{}: removed from {}", tarball, server.name);
                    }
                }
                Err(e) => {
                    any_failed = true;
                    eprintln!("{}: remove from {} failed: {}", tarball, server.name, e);
                }
            }
        }
    }

    Ok(if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
