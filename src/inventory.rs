use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;

use crate::dockerfile::discover_image_families;
use crate::models::{Config, ImageFamily, ImageTableRow, ScanStatus, UnknownTarball};
use crate::remote;

pub struct Inventory {
    pub families: BTreeMap<String, ImageFamily>,
    pub unknowns: Vec<UnknownTarball>,
    pub server_names: Vec<String>,
    pub scan_status: BTreeMap<String, ScanStatus>,
    pub warnings: Vec<String>,
}

/// Discover Dockerfiles, refresh local staging tarballs, and (unless `scan`
/// is false) scan every configured server to completion.
pub async fn load(config: &Config, scan: bool) -> Result<Inventory> {
    let server_images_dir = Path::new(&config.paths.server_images_dir);
    let discovery = discover_image_families(server_images_dir)?;
    let mut families = discovery.families;
    let warnings = discovery.warnings;

    let staging_dir = Path::new(&config.paths.staging_dir).to_path_buf();
    remote::refresh_local_tarballs(&mut families, &staging_dir);

    let server_names: Vec<String> = config.servers.iter().map(|s| s.name.clone()).collect();
    let mut scan_status: BTreeMap<String, ScanStatus> = BTreeMap::new();
    for name in &server_names {
        scan_status.insert(name.clone(), ScanStatus::Pending);
    }

    let mut unknowns: Vec<UnknownTarball> = Vec::new();

    if scan {
        let mut rx = remote::start_remote_scan(
            config.servers.clone(),
            config.paths.rke2_images_dir.clone(),
        );

        while let Some(event) = rx.recv().await {
            if let remote::RemoteEvent::Scan {
                server_name,
                filenames,
            } = event
            {
                match filenames {
                    Ok(files) => {
                        scan_status.insert(server_name.clone(), ScanStatus::Ok);
                        remote::apply_scan_results(
                            &mut families,
                            &mut unknowns,
                            &server_name,
                            &files,
                        );
                    }
                    Err(err) => {
                        scan_status.insert(server_name.clone(), ScanStatus::Error(err));
                    }
                }
            }
        }
    }

    Ok(Inventory {
        families,
        unknowns,
        server_names,
        scan_status,
        warnings,
    })
}

/// True if any server's scan ended in an error. Commands that scan use this
/// to decide whether to exit non-zero.
pub fn any_scan_error(inv: &Inventory) -> bool {
    inv.scan_status
        .values()
        .any(|s| matches!(s, ScanStatus::Error(_)))
}

pub fn rows(inv: &Inventory) -> Vec<ImageTableRow> {
    rows_from(&inv.families, &inv.unknowns)
}

/// Core of `rows()`, taking the raw pieces directly. Lets callers that hold
/// `families`/`unknowns` without a full `Inventory` (e.g. `App`, which scans
/// incrementally) reuse the same row-ordering logic.
pub fn rows_from(
    families: &BTreeMap<String, ImageFamily>,
    unknowns: &[UnknownTarball],
) -> Vec<ImageTableRow> {
    let mut rows = Vec::new();

    for family in families.values() {
        if let Some(ref current) = family.current {
            rows.push(ImageTableRow::Current(current.clone()));
        }
        for version in &family.stale_versions {
            let servers = family
                .remote_presence
                .get(version)
                .cloned()
                .unwrap_or_default();
            rows.push(ImageTableRow::Stale {
                family_name: family.name.clone(),
                version: version.clone(),
                servers,
            });
        }
    }

    for unknown in unknowns {
        if !unknown.servers.is_empty() {
            rows.push(ImageTableRow::Unknown(unknown.clone()));
        }
    }

    rows
}
