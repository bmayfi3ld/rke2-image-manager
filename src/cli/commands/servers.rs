use anyhow::Result;
use std::process::ExitCode;

use crate::cli::output::{self, OutputOptions};
use crate::models::Config;

pub fn run(config: &Config, opts: &OutputOptions) -> Result<ExitCode> {
    if opts.json {
        output::print_json(&config.servers)?;
    } else {
        let rows: Vec<Vec<String>> = config
            .servers
            .iter()
            .map(|s| {
                vec![
                    s.name.clone(),
                    format!("{}@{}:{}", s.user, s.host, s.port),
                    s.identity_file.clone().unwrap_or_else(|| "-".to_string()),
                ]
            })
            .collect();
        print!(
            "{}",
            output::render_table(&["NAME", "USER@HOST:PORT", "IDENTITY"], &rows)
        );
    }
    Ok(ExitCode::SUCCESS)
}
