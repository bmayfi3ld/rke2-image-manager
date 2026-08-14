use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

use rke2_image_manager::inventory::{load, rows};
use rke2_image_manager::models::{Config, ImageTableRow, PathsConfig};

fn create_dockerfile(path: &PathBuf, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn base_config(server_images_dir: &std::path::Path, staging_dir: &std::path::Path) -> Config {
    Config {
        paths: PathsConfig {
            server_images_dir: server_images_dir.to_string_lossy().to_string(),
            staging_dir: staging_dir.to_string_lossy().to_string(),
            rke2_images_dir: "/opt/rke2-images".to_string(),
            image_registry: "registry.local".to_string(),
        },
        servers: Vec::new(),
    }
}

#[tokio::test]
async fn test_load_no_scan_discovers_families_and_local_tarballs() {
    let temp = TempDir::new().unwrap();
    let server_images_dir = temp.path().join("server_images");
    let staging_dir = temp.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();

    create_dockerfile(
        &server_images_dir.join("adguardhome.dockerfile"),
        "# v0.107.68-1.0.2\nFROM adguard/adguardhome:v0.107.68\n",
    );
    create_dockerfile(
        &server_images_dir.join("caddy.dockerfile"),
        "# 2.10.2-0.4.0\nFROM caddy:2.10.2\n",
    );

    fs::write(
        staging_dir.join("adguardhome-v0.107.68-1.0.2.tar"),
        b"fake tarball",
    )
    .unwrap();

    let config = base_config(&server_images_dir, &staging_dir);
    let inv = load(&config, false).await.expect("load failed");

    assert_eq!(inv.families.len(), 2);
    assert!(inv.server_names.is_empty());
    assert!(inv.scan_status.is_empty());

    let adguard = inv.families.get("adguardhome").expect("adguardhome missing");
    let current = adguard.current.as_ref().expect("current missing");
    assert!(current.local_tarball, "expected local tarball to be detected");

    let caddy = inv.families.get("caddy").expect("caddy missing");
    let caddy_current = caddy.current.as_ref().expect("current missing");
    assert!(!caddy_current.local_tarball);
}

#[tokio::test]
async fn test_load_scan_true_with_no_servers_completes() {
    let temp = TempDir::new().unwrap();
    let server_images_dir = temp.path().join("server_images");
    let staging_dir = temp.path().join("staging");
    fs::create_dir_all(&server_images_dir).unwrap();
    fs::create_dir_all(&staging_dir).unwrap();

    let config = base_config(&server_images_dir, &staging_dir);
    let inv = load(&config, true).await.expect("load failed");

    assert!(inv.families.is_empty());
    assert!(inv.scan_status.is_empty());
}

#[tokio::test]
async fn test_rows_orders_current_then_stale_then_unknown() {
    let temp = TempDir::new().unwrap();
    let server_images_dir = temp.path().join("server_images");
    let staging_dir = temp.path().join("staging");
    fs::create_dir_all(&staging_dir).unwrap();

    create_dockerfile(
        &server_images_dir.join("adguardhome.dockerfile"),
        "# v0.107.68-1.0.2\nFROM adguard/adguardhome:v0.107.68\n",
    );

    let config = base_config(&server_images_dir, &staging_dir);
    let mut inv = load(&config, false).await.expect("load failed");

    // Simulate a stale version and an unknown tarball discovered by a scan,
    // the way apply_scan_results would.
    let family = inv.families.get_mut("adguardhome").unwrap();
    family.stale_versions.insert("v0.107.67-1.0.1".to_string());
    family.remote_presence.insert(
        "v0.107.67-1.0.1".to_string(),
        std::iter::once("bamserve4".to_string()).collect(),
    );
    inv.unknowns.push(rke2_image_manager::models::UnknownTarball {
        filename: "mystery.tar".to_string(),
        servers: std::iter::once("bamserve5".to_string()).collect(),
    });

    let result = rows(&inv);
    assert_eq!(result.len(), 3);
    assert!(matches!(result[0], ImageTableRow::Current(_)));
    assert!(matches!(result[1], ImageTableRow::Stale { .. }));
    assert!(matches!(result[2], ImageTableRow::Unknown(_)));
}

#[tokio::test]
async fn test_rows_skips_unknowns_with_no_servers() {
    let temp = TempDir::new().unwrap();
    let server_images_dir = temp.path().join("server_images");
    let staging_dir = temp.path().join("staging");
    fs::create_dir_all(&server_images_dir).unwrap();
    fs::create_dir_all(&staging_dir).unwrap();

    let config = base_config(&server_images_dir, &staging_dir);
    let mut inv = load(&config, false).await.expect("load failed");
    inv.unknowns.push(rke2_image_manager::models::UnknownTarball {
        filename: "orphan.tar".to_string(),
        servers: Default::default(),
    });

    let result = rows(&inv);
    assert!(result.is_empty());
}
