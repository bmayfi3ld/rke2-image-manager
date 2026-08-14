use anyhow::Result;
use clap::Args;
use std::process::ExitCode;

use crate::cli::output::{self, OutputOptions};
use crate::inventory::{self, load, Inventory};
use crate::models::{Config, ImageTableRow};

#[derive(Args)]
pub struct ListArgs {
    /// Skip SSH scanning; report only Dockerfile discovery + local staging tarballs
    #[arg(long)]
    pub no_scan: bool,

    /// Case-insensitive substring filter on name and name-version
    #[arg(long)]
    pub filter: Option<String>,

    /// Show only current (up-to-date) images
    #[arg(long)]
    pub current: bool,

    /// Show only stale versions
    #[arg(long)]
    pub stale: bool,

    /// Show only unknown tarballs
    #[arg(long)]
    pub unknown: bool,
}

pub async fn run(config: &Config, args: &ListArgs, opts: &OutputOptions) -> Result<ExitCode> {
    let inv = load(config, !args.no_scan).await?;
    let all_rows = inventory::rows(&inv);

    let show_all = !args.current && !args.stale && !args.unknown;
    let filter = args.filter.as_deref().map(|s| s.to_lowercase());

    let filtered: Vec<&ImageTableRow> = all_rows
        .iter()
        .filter(|row| row_type_matches(row, show_all, args))
        .filter(|row| filter_matches(row, filter.as_deref()))
        .collect();

    if opts.json {
        print_json_output(&inv, &filtered)?;
    } else {
        print_table(&inv, &filtered);
    }

    Ok(if inventory::any_scan_error(&inv) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn row_type_matches(row: &ImageTableRow, show_all: bool, args: &ListArgs) -> bool {
    match row {
        ImageTableRow::Current(_) => show_all || args.current,
        ImageTableRow::Stale { .. } => show_all || args.stale,
        ImageTableRow::Unknown(_) => show_all || args.unknown,
    }
}

fn filter_matches(row: &ImageTableRow, filter: Option<&str>) -> bool {
    let Some(f) = filter else {
        return true;
    };
    let (name, name_version) = row_names(row);
    name.to_lowercase().contains(f) || name_version.to_lowercase().contains(f)
}

fn row_names(row: &ImageTableRow) -> (String, String) {
    match row {
        ImageTableRow::Current(img) => (img.name.clone(), format!("{}-{}", img.name, img.version)),
        ImageTableRow::Stale {
            family_name,
            version,
            ..
        } => (family_name.clone(), format!("{}-{}", family_name, version)),
        ImageTableRow::Unknown(u) => (u.filename.clone(), u.filename.clone()),
    }
}

fn print_table(inv: &Inventory, rows: &[&ImageTableRow]) {
    let mut headers: Vec<&str> = vec!["NAME", "VERSION", "LOCAL"];
    for s in &inv.server_names {
        headers.push(s.as_str());
    }
    headers.push("STATE");

    let table_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| build_row(row, &inv.server_names))
        .collect();

    print!("{}", output::render_table(&headers, &table_rows));
}

fn build_row(row: &ImageTableRow, server_names: &[String]) -> Vec<String> {
    match row {
        ImageTableRow::Current(img) => {
            let mut cells = vec![img.name.clone(), img.version.clone(), yn(img.local_tarball)];
            for s in server_names {
                cells.push(yn(img.server_presence.get(s).copied().unwrap_or(false)));
            }
            cells.push(build_state_label(&img.build_state).to_string());
            cells
        }
        ImageTableRow::Stale {
            version, servers, ..
        } => {
            let mut cells = vec!["  (stale)".to_string(), version.clone(), "-".to_string()];
            for s in server_names {
                cells.push(yn(servers.contains(s)));
            }
            cells.push("stale".to_string());
            cells
        }
        ImageTableRow::Unknown(u) => {
            let mut cells = vec![u.filename.clone(), "-".to_string(), "-".to_string()];
            for s in server_names {
                cells.push(yn(u.servers.contains(s)));
            }
            cells.push("unknown".to_string());
            cells
        }
    }
}

fn yn(b: bool) -> String {
    if b {
        "yes".to_string()
    } else {
        "no".to_string()
    }
}

fn build_state_label(state: &crate::models::BuildState) -> &'static str {
    match state {
        crate::models::BuildState::Building => "building",
        crate::models::BuildState::Failed(_) => "failed",
        crate::models::BuildState::Success | crate::models::BuildState::Idle => "ok",
    }
}

fn print_json_output(inv: &Inventory, rows: &[&ImageTableRow]) -> Result<()> {
    let mut images = Vec::new();
    let mut stale = Vec::new();
    let mut unknown = Vec::new();

    for row in rows {
        match row {
            ImageTableRow::Current(img) => images.push(img.clone()),
            ImageTableRow::Stale {
                family_name,
                version,
                servers,
            } => {
                stale.push(serde_json::json!({
                    "family_name": family_name,
                    "version": version,
                    "servers": servers,
                }));
            }
            ImageTableRow::Unknown(u) => unknown.push(u.clone()),
        }
    }

    let payload = serde_json::json!({
        "servers": inv.server_names,
        "scan": inv.scan_status,
        "images": images,
        "stale": stale,
        "unknown": unknown,
    });

    output::print_json(&payload)
}
