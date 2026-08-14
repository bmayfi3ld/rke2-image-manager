use anyhow::Result;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use clap_complete::generate;

use rke2_image_manager::cli::{self, Cli, Command};
use rke2_image_manager::config::{find_config, load_config};
use rke2_image_manager::tui;

fn main() -> ExitCode {
    let cli_args = Cli::parse();

    // Shell completions need neither a config file nor a runtime.
    if let Some(Command::Completions { shell }) = &cli_args.command {
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        generate(*shell, &mut cmd, name, &mut std::io::stdout());
        return ExitCode::SUCCESS;
    }

    let config_path = match find_config(cli_args.config.as_deref()) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Error: {:#}", e);
            return ExitCode::from(3);
        }
    };
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {:#}", e);
            return ExitCode::from(3);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(2)
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Error: failed to start async runtime: {}", e);
            return ExitCode::from(1);
        }
    };

    let result: Result<ExitCode> = match &cli_args.command {
        None | Some(Command::Tui) => runtime.block_on(tui::run(config)).map(|_| ExitCode::SUCCESS),
        Some(_) => runtime.block_on(cli::run(cli_args, config)),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {:#}", e);
            ExitCode::from(1)
        }
    }
}
