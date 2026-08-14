use anyhow::Result;
use clap::Args;
use std::collections::BTreeMap;
use std::process::ExitCode;

use crate::cli::output::OutputOptions;
use crate::cli::select;
use crate::inventory::load;
use crate::models::Config;
use crate::remote::{self, tarball_filename};

#[derive(Args)]
pub struct CleanArgs {
    /// Sweep stale versions
    #[arg(long)]
    pub stale: bool,

    /// Sweep unknown tarballs
    #[arg(long)]
    pub unknown: bool,

    /// Restrict to these servers (repeatable); defaults to every configured server
    #[arg(short = 's', long = "server")]
    pub server: Vec<String>,

    /// Print the exact removal targets without touching anything
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run(config: &Config, args: &CleanArgs, opts: &OutputOptions) -> Result<ExitCode> {
    if !args.stale && !args.unknown {
        eprintln!("Error: specify --stale and/or --unknown");
        return Ok(ExitCode::from(2));
    }

    let inv = load(config, true).await?;

    let servers = match select::resolve_servers(&args.server, &config.servers) {
        Ok(servers) => servers,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(ExitCode::from(2));
        }
    };
    let server_by_name: BTreeMap<String, crate::models::Server> =
        servers.iter().map(|s| (s.name.clone(), s.clone())).collect();

    let mut targets: Vec<(String, String)> = Vec::new();

    if args.stale {
        for family in inv.families.values() {
            for version in &family.stale_versions {
                let tarball = tarball_filename(&family.name, version);
                if let Some(servers_with) = family.remote_presence.get(version) {
                    for server_name in servers_with {
                        if server_by_name.contains_key(server_name) {
                            targets.push((tarball.clone(), server_name.clone()));
                        }
                    }
                }
            }
        }
    }

    if args.unknown {
        for unknown in &inv.unknowns {
            for server_name in &unknown.servers {
                if server_by_name.contains_key(server_name) {
                    targets.push((unknown.filename.clone(), server_name.clone()));
                }
            }
        }
    }

    if targets.is_empty() {
        println!("Nothing to clean");
        return Ok(ExitCode::SUCCESS);
    }

    if args.dry_run {
        for (tarball, server_name) in &targets {
            let server = &server_by_name[server_name];
            println!(
                "{}",
                remote::preview_remove_command(server, tarball, &config.paths.rke2_images_dir)
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    let mut any_failed = false;
    for (tarball, server_name) in &targets {
        let server = &server_by_name[server_name];
        match remote::remove_tarball(server, tarball, &config.paths.rke2_images_dir).await {
            Ok(()) => {
                if !opts.quiet {
                    println!("{}: removed from {}", tarball, server_name);
                }
            }
            Err(e) => {
                any_failed = true;
                eprintln!("{}: remove from {} failed: {}", tarball, server_name, e);
            }
        }
    }

    Ok(if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
