use anyhow::Result;
use std::process::ExitCode;

use crate::cli::output::{self, OutputOptions};
use crate::inventory::{self, load};
use crate::models::{Config, ScanStatus};

pub async fn run(config: &Config, opts: &OutputOptions) -> Result<ExitCode> {
    let inv = load(config, true).await?;

    let managed = inv.families.len();
    let stale: usize = inv.families.values().map(|f| f.stale_versions.len()).sum();
    let unknown = inv.unknowns.len();

    if opts.json {
        let payload = serde_json::json!({
            "servers": inv.scan_status,
            "managed": managed,
            "stale": stale,
            "unknown": unknown,
        });
        output::print_json(&payload)?;
    } else {
        let rows: Vec<Vec<String>> = inv
            .server_names
            .iter()
            .map(|name| {
                let status = inv
                    .scan_status
                    .get(name)
                    .cloned()
                    .unwrap_or(ScanStatus::Pending);
                vec![name.clone(), status_label(&status)]
            })
            .collect();
        print!("{}", output::render_table(&["SERVER", "STATUS"], &rows));
        println!();
        println!("{} managed ({} stale) | {} unknown", managed, stale, unknown);
    }

    Ok(if inventory::any_scan_error(&inv) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn status_label(status: &ScanStatus) -> String {
    match status {
        ScanStatus::Pending => "pending".to_string(),
        ScanStatus::Ok => "ok".to_string(),
        ScanStatus::Error(e) => format!("error: {}", e),
    }
}
