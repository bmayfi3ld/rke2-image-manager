use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tempfile::TempDir;

use rke2_image_manager::models::{BuildState, ImageFamily, ManagedImage};
use rke2_image_manager::remote::{apply_scan_results, refresh_local_tarballs, tarball_filename};

fn make_image(name: &str, version: &str) -> ManagedImage {
    ManagedImage {
        name: name.to_string(),
        version: version.to_string(),
        dockerfile_path: PathBuf::from(format!("{}.dockerfile", name)),
        context_dir: PathBuf::from("."),
        local_tarball: false,
        server_presence: BTreeMap::new(),
        build_state: BuildState::Idle,
        build_log: Vec::new(),
    }
}

fn make_family(name: &str, version: &str) -> ImageFamily {
    ImageFamily {
        name: name.to_string(),
        current_version: Some(version.to_string()),
        current: Some(make_image(name, version)),
        stale_versions: BTreeSet::new(),
        remote_presence: BTreeMap::new(),
    }
}

#[test]
fn test_apply_scan_results_current_version_present() {
    let mut families = BTreeMap::new();
    families.insert("adguardhome".to_string(), make_family("adguardhome", "v0.107.68-1.0.2"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["adguardhome-v0.107.68-1.0.2.tar".to_string()],
    );

    let adguard = families.get("adguardhome").unwrap();
    assert!(adguard.current.is_some());
    assert_eq!(
        adguard.current.as_ref().unwrap().server_presence.get("bamserve4"),
        Some(&true)
    );
    assert!(unknowns.is_empty());
}

#[test]
fn test_apply_scan_results_stale_version() {
    let mut families = BTreeMap::new();
    families.insert("adguardhome".to_string(), make_family("adguardhome", "v0.107.68-1.0.2"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["adguardhome-v0.107.50-1.0.1.tar".to_string()],
    );

    let adguard = families.get("adguardhome").unwrap();
    assert!(adguard.current.is_some());
    // Successful scan completes the presence map (false = absent)
    assert_eq!(
        adguard.current.as_ref().unwrap().server_presence.get("bamserve4"),
        Some(&false)
    );
    assert!(adguard.stale_versions.contains("v0.107.50-1.0.1"));
    assert_eq!(
        adguard.remote_presence.get("v0.107.50-1.0.1"),
        Some(&["bamserve4".to_string()].into_iter().collect())
    );
    assert!(unknowns.is_empty());
}

#[test]
fn test_apply_scan_results_unknown_tarball() {
    let mut families = BTreeMap::new();
    families.insert("adguardhome".to_string(), make_family("adguardhome", "v0.107.68-1.0.2"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["portal-knights-server-v1.0.0.tar".to_string()],
    );

    assert_eq!(
        families
            .get("adguardhome")
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .server_presence
            .get("bamserve4"),
        Some(&false)
    );
    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].filename, "portal-knights-server-v1.0.0.tar");
    assert!(unknowns[0].servers.contains("bamserve4"));
}

#[test]
fn test_apply_scan_results_ignores_non_tar_files() {
    let mut families = BTreeMap::new();
    families.insert("adguardhome".to_string(), make_family("adguardhome", "v0.107.68-1.0.2"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &[
            "somefile.txt".to_string(),
            "readme.md".to_string(),
            "adguardhome-v0.107.68-1.0.2.tar".to_string(),
        ],
    );

    let adguard = families.get("adguardhome").unwrap();
    assert_eq!(
        adguard.current.as_ref().unwrap().server_presence.get("bamserve4"),
        Some(&true)
    );
    assert!(unknowns.is_empty());
}

#[test]
fn test_apply_scan_results_multiple_servers_unknown_tarball() {
    let mut families = BTreeMap::new();
    families.insert("adguardhome".to_string(), make_family("adguardhome", "v0.107.68-1.0.2"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["strange-backup.tar".to_string()],
    );
    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve5",
        &["strange-backup.tar".to_string()],
    );

    assert_eq!(unknowns.len(), 1);
    assert_eq!(unknowns[0].servers.len(), 2);
    assert!(unknowns[0].servers.contains("bamserve4"));
    assert!(unknowns[0].servers.contains("bamserve5"));
}

#[test]
fn test_apply_scan_results_longest_prefix_matching() {
    let mut families = BTreeMap::new();
    families.insert("adguard".to_string(), make_family("adguard", "v1.0.0"));
    families.insert("adguardhome".to_string(), make_family("adguardhome", "v0.107.68-1.0.2"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["adguardhome-v0.107.50-1.0.1.tar".to_string()],
    );

    let adguardhome = families.get("adguardhome").unwrap();
    assert!(adguardhome.stale_versions.contains("v0.107.50-1.0.1"));
    assert!(families.get("adguard").unwrap().stale_versions.is_empty());
    assert!(unknowns.is_empty());
}

#[test]
fn test_apply_scan_results_caddy_version() {
    let mut families = BTreeMap::new();
    families.insert("caddy".to_string(), make_family("caddy", "2.10.2-0.4.0"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["caddy-2.10.2-0.4.0.tar".to_string()],
    );

    let caddy = families.get("caddy").unwrap();
    assert_eq!(
        caddy.current.as_ref().unwrap().server_presence.get("bamserve4"),
        Some(&true)
    );
    assert!(unknowns.is_empty());
}

#[test]
fn test_apply_scan_results_multiple_stale_versions() {
    let mut families = BTreeMap::new();
    families.insert("radarr".to_string(), make_family("radarr", "v5.28.0.10274-1.0.3"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["radarr-v5.20.0.9280-1.0.2.tar".to_string()],
    );
    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve5",
        &["radarr-v5.20.0.9280-1.0.2.tar".to_string()],
    );

    let radarr = families.get("radarr").unwrap();
    assert_eq!(radarr.stale_versions.len(), 1);
    assert!(radarr.stale_versions.contains("v5.20.0.9280-1.0.2"));
    assert_eq!(
        radarr.remote_presence.get("v5.20.0.9280-1.0.2"),
        Some(
            &["bamserve4".to_string(), "bamserve5".to_string()]
                .into_iter()
                .collect()
        )
    );
}

#[test]
fn test_apply_scan_results_inserts_false_for_absent_servers() {
    // After a successful scan, every server should have an entry in server_presence
    // (true if present, false if absent). This is the foundation for the `?` indicator
    // (which only shows when NO entry exists, i.e. scan failed or pending).
    let mut families = BTreeMap::new();
    families.insert("caddy".to_string(), make_family("caddy", "2.10.2-0.4.0"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &[],
    );

    let caddy = families.get("caddy").unwrap();
    let presence = &caddy.current.as_ref().unwrap().server_presence;
    assert_eq!(presence.get("bamserve4"), Some(&false));
}

#[test]
fn test_apply_scan_results_ambiguous_three_family_prefix() {
    let mut families = BTreeMap::new();
    families.insert("adguard".to_string(), make_family("adguard", "v1.0.0"));
    families.insert("adguardhome".to_string(), make_family("adguardhome", "v2.0.0"));
    families.insert("adguardhome-old".to_string(), make_family("adguardhome-old", "v3.0.0"));
    let mut unknowns = Vec::new();

    apply_scan_results(
        &mut families,
        &mut unknowns,
        "bamserve4",
        &["adguardhome-v9.9.9.tar".to_string()],
    );

    // "adguardhome" should match (longest prefix)
    assert!(families
        .get("adguardhome")
        .unwrap()
        .stale_versions
        .contains("v9.9.9"));
    // "adguard" should NOT pick up this version
    assert!(families.get("adguard").unwrap().stale_versions.is_empty());
    // "adguardhome-old" should NOT pick this up either
    assert!(families
        .get("adguardhome-old")
        .unwrap()
        .stale_versions
        .is_empty());
}

#[test]
fn test_refresh_local_tarballs_marks_existing_files() {
    let temp = TempDir::new().unwrap();
    let staging = temp.path();
    let tarball_name = "myimg-v1.0.0.tar";
    let tarball_path = staging.join(tarball_name);
    std::fs::write(&tarball_path, b"dummy tarball").unwrap();

    let mut families = BTreeMap::new();
    families.insert("myimg".to_string(), make_family("myimg", "v1.0.0"));
    families.insert("other".to_string(), make_family("other", "v2.0.0"));

    refresh_local_tarballs(&mut families, staging);

    assert!(families.get("myimg").unwrap().current.as_ref().unwrap().local_tarball);
    assert!(!families.get("other").unwrap().current.as_ref().unwrap().local_tarball);
}

#[test]
fn test_refresh_local_tarballs_marks_missing_files() {
    let temp = TempDir::new().unwrap();
    let staging = temp.path();
    let mut families = BTreeMap::new();
    families.insert("myimg".to_string(), make_family("myimg", "v1.0.0"));

    refresh_local_tarballs(&mut families, staging);

    assert!(!families.get("myimg").unwrap().current.as_ref().unwrap().local_tarball);
}

#[test]
fn test_tarball_filename_helper() {
    assert_eq!(tarball_filename("adguardhome", "v0.107.68-1.0.2"), "adguardhome-v0.107.68-1.0.2.tar");
    assert_eq!(tarball_filename("caddy", "2.10.2-0.4.0"), "caddy-2.10.2-0.4.0.tar");
}
