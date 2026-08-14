use anyhow::Result;
use clap::Args;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use crate::cli::commands::build::build_one;
use crate::cli::output::OutputOptions;
use crate::cli::select;
use crate::inventory::load;
use crate::models::{Config, ImageFamily, ManagedImage, Server};
use crate::remote::{self, tarball_filename};

#[derive(Args)]
pub struct DeployArgs {
    /// Image family names to deploy (version comes from the Dockerfile)
    pub images: Vec<String>,

    /// Deploy every discovered family
    #[arg(long)]
    pub all: bool,

    /// Restrict to these servers (repeatable); defaults to every configured server
    #[arg(short = 's', long = "server")]
    pub server: Vec<String>,

    /// Build first if the staging tarball is absent or the Dockerfile is newer
    #[arg(long)]
    pub build: bool,

    /// Skip servers that already report the tarball present
    #[arg(long)]
    pub missing_only: bool,

    /// Print what would be deployed without touching anything
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run(config: &Config, args: &DeployArgs, opts: &OutputOptions) -> Result<ExitCode> {
    if !args.all && args.images.is_empty() {
        eprintln!("Error: specify one or more image names, or --all");
        return Ok(ExitCode::from(2));
    }

    let inv = load(config, true).await?;

    let targets = match resolve_targets(args, &inv.families) {
        Ok(targets) => targets,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(ExitCode::from(2));
        }
    };
    if targets.is_empty() {
        eprintln!("No images to deploy");
        return Ok(ExitCode::from(2));
    }

    let servers = match select::resolve_servers(&args.server, &config.servers) {
        Ok(servers) => servers,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(ExitCode::from(2));
        }
    };

    let mut any_failed = false;

    for image in targets {
        let server_subset = servers_for_image(&image, &servers, args.missing_only);
        if server_subset.is_empty() {
            if !opts.quiet {
                println!("{}: no target servers (all up to date)", image.name);
            }
            continue;
        }

        let tarball_name = tarball_filename(&image.name, &image.version);
        let local_path = Path::new(&config.paths.staging_dir).join(&tarball_name);
        let needs_build = args.build && should_build(&image, &local_path, config);

        if args.dry_run {
            if needs_build {
                println!("{}: would build {}", image.name, tarball_name);
            }
            for server in &server_subset {
                println!("{}: would deploy {} to {}", image.name, tarball_name, server.name);
            }
            continue;
        }

        if needs_build {
            let (result, _log) = build_one(&image, config, opts.quiet).await;
            if let Err(e) = result {
                any_failed = true;
                eprintln!("{}: build failed, skipping deploy: {}", image.name, e);
                continue;
            }
        }

        if !local_path.is_file() {
            any_failed = true;
            eprintln!(
                "{}: no local tarball at {} (use --build or build first)",
                image.name,
                local_path.display()
            );
            continue;
        }

        for server in &server_subset {
            match remote::deploy_tarball(server, &local_path, &config.paths.rke2_images_dir).await
            {
                Ok(()) => {
                    println!("{}: deployed to {}", image.name, server.name);
                }
                Err(e) => {
                    any_failed = true;
                    eprintln!("{}: deploy to {} failed: {}", image.name, server.name, e);
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

fn resolve_targets(
    args: &DeployArgs,
    families: &BTreeMap<String, ImageFamily>,
) -> Result<Vec<ManagedImage>, String> {
    if args.all {
        return Ok(families.values().filter_map(|f| f.current.clone()).collect());
    }
    let mut targets = Vec::new();
    for raw in &args.images {
        targets.push(select::resolve_current(raw, families)?);
    }
    Ok(targets)
}

fn servers_for_image(
    image: &ManagedImage,
    servers: &[Server],
    missing_only: bool,
) -> Vec<Server> {
    servers
        .iter()
        .filter(|server| {
            if !missing_only {
                return true;
            }
            !image
                .server_presence
                .get(&server.name)
                .copied()
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

/// True if the local tarball is missing, or the Dockerfile has been
/// modified more recently than the tarball was written.
fn should_build(image: &ManagedImage, tarball_path: &Path, config: &Config) -> bool {
    let tarball_mtime = match std::fs::metadata(tarball_path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true,
    };

    let dockerfile_path = Path::new(&config.paths.server_images_dir)
        .join(&image.context_dir)
        .join(&image.dockerfile_path);

    match std::fs::metadata(&dockerfile_path).and_then(|m| m.modified()) {
        Ok(dockerfile_mtime) => dockerfile_mtime > tarball_mtime,
        Err(_) => false,
    }
}
