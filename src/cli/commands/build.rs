use anyhow::Result;
use clap::Args;
use std::path::Path;
use std::process::ExitCode;

use crate::build;
use crate::cli::output::OutputOptions;
use crate::cli::select;
use crate::inventory::load;
use crate::models::{Config, ManagedImage};

#[derive(Args)]
pub struct BuildArgs {
    /// Image family names to build (version comes from the Dockerfile)
    pub images: Vec<String>,

    /// Build every discovered family
    #[arg(long)]
    pub all: bool,

    /// Write the build log to staging_dir/logs/<name>-<version>-<ts>.log
    #[arg(long)]
    pub dump_log: bool,
}

pub async fn run(config: &Config, args: &BuildArgs, opts: &OutputOptions) -> Result<ExitCode> {
    if !args.all && args.images.is_empty() {
        eprintln!("Error: specify one or more image names, or --all");
        return Ok(ExitCode::from(2));
    }

    let inv = load(config, false).await?;

    let targets = match resolve_targets(args, &inv.families) {
        Ok(targets) => targets,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Ok(ExitCode::from(2));
        }
    };

    if targets.is_empty() {
        eprintln!("No images to build");
        return Ok(ExitCode::from(2));
    }

    let mut any_failed = false;

    for image in targets {
        let name = image.name.clone();
        let (build_result, log_lines) = build_one(&image, config, opts.quiet).await;

        match &build_result {
            Ok(()) => println!("{}: build ok", name),
            Err(e) => {
                any_failed = true;
                eprintln!("{}: build failed: {}", name, e);
            }
        }

        if args.dump_log {
            match build::dump_log_to_file(
                &config.paths.staging_dir,
                &image.name,
                &image.version,
                &log_lines,
            ) {
                Ok(path) => println!("{}: log dumped to {}", name, path.display()),
                Err(e) => eprintln!("{}: failed to dump log: {}", name, e),
            }
        }
    }

    Ok(if any_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// Run a single build to completion, streaming log lines to stdout unless
/// `quiet`. Returns the build result plus the full log (collected
/// regardless of `quiet`, so callers can still dump it to a file).
pub async fn build_one(
    image: &ManagedImage,
    config: &Config,
    quiet: bool,
) -> (Result<(), String>, Vec<String>) {
    if !quiet {
        println!("==> Building {}:{}", image.name, image.version);
    }

    let (mut rx, _handle) = build::start_build(
        image.clone(),
        config.paths.image_registry.clone(),
        config.paths.staging_dir.clone(),
        Path::new(&config.paths.server_images_dir).to_path_buf(),
    );

    let mut log_lines: Vec<String> = Vec::new();
    let mut result: Result<(), String> = Ok(());

    while let Some(event) = rx.recv().await {
        match event {
            build::BuildEvent::LogLine { line, .. } => {
                if !quiet {
                    println!("{}", line);
                }
                log_lines.push(line);
            }
            build::BuildEvent::BuildComplete { result: r, .. } => {
                result = r;
            }
        }
    }

    (result, log_lines)
}

fn resolve_targets(
    args: &BuildArgs,
    families: &std::collections::BTreeMap<String, crate::models::ImageFamily>,
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
