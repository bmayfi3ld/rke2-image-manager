use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

use rke2_image_manager::dockerfile::discover_image_families;
use rke2_image_manager::models::ImageFamily;

fn create_dockerfile(path: &PathBuf, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn discover(base: &std::path::Path) -> BTreeMap<String, ImageFamily> {
    discover_image_families(base).expect("discover_image_families failed").families
}

#[test]
fn test_discover_simple_dockerfiles() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("adguardhome.dockerfile"),
        "# v0.107.68-1.0.2\nFROM adguard/adguardhome:v0.107.68\n",
    );
    create_dockerfile(
        &base.join("caddy.dockerfile"),
        "# 2.10.2-0.4.0\nFROM caddy:2.10.2\n",
    );

    let families = discover(&base);

    assert_eq!(families.len(), 2);

    let adguard = families.get("adguardhome").expect("adguardhome not found");
    assert_eq!(adguard.name, "adguardhome");
    assert_eq!(adguard.current_version.as_deref(), Some("v0.107.68-1.0.2"));

    let caddy = families.get("caddy").expect("caddy not found");
    assert_eq!(caddy.name, "caddy");
    assert_eq!(caddy.current_version.as_deref(), Some("2.10.2-0.4.0"));
}

#[test]
fn test_discover_nested_dockerfile() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("immich_transfer").join("immich_transfer.dockerfile"),
        "# 1.1.1\nFROM python:3.13-slim\n",
    );
    create_dockerfile(
        &base.join("portal_knights_dedicated_server").join("Dockerfile"),
        "# v1.0.0-1.0.0\nFROM lancommander/wine:latest-amd64\n",
    );

    let families = discover(&base);

    assert_eq!(families.len(), 2);

    let immich = families.get("immich_transfer").expect("immich_transfer not found");
    assert_eq!(immich.name, "immich_transfer");
    assert_eq!(immich.current_version.as_deref(), Some("1.1.1"));
    assert_eq!(
        immich.current.as_ref().unwrap().dockerfile_path,
        PathBuf::from("immich_transfer.dockerfile")
    );
    assert_eq!(
        immich.current.as_ref().unwrap().context_dir,
        PathBuf::from("immich_transfer")
    );

    let portal = families
        .get("portal_knights_dedicated_server")
        .expect("portal_knights_dedicated_server not found");
    assert_eq!(portal.name, "portal_knights_dedicated_server");
    assert_eq!(portal.current_version.as_deref(), Some("v1.0.0-1.0.0"));
    assert_eq!(
        portal.current.as_ref().unwrap().dockerfile_path,
        PathBuf::from("Dockerfile")
    );
    assert_eq!(
        portal.current.as_ref().unwrap().context_dir,
        PathBuf::from("portal_knights_dedicated_server")
    );
}

#[test]
fn test_discover_no_version_comment_uses_latest() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(&base.join("nover.dockerfile"), "FROM ubuntu:latest\n");

    let discovery = discover_image_families(&base).expect("discover_image_families failed");

    assert_eq!(discovery.families.len(), 1);
    let nover = discovery.families.get("nover").expect("nover not found");
    assert_eq!(nover.current_version.as_deref(), Some("latest"));
    assert!(!discovery.warnings.is_empty());
    assert!(
        discovery.warnings.iter().any(|w| w.contains("nover.dockerfile")),
        "expected warning about missing version: {:?}",
        discovery.warnings
    );
}

#[test]
fn test_discover_ignores_non_dockerfiles() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("adguardhome.dockerfile"),
        "# v0.107.68-1.0.2\nFROM adguard/adguardhome:v0.107.68\n",
    );
    create_dockerfile(&base.join("readme.txt"), "This is not a dockerfile\n");
    create_dockerfile(&base.join("script.sh"), "#!/bin/bash\necho hello\n");

    let families = discover(&base);

    assert_eq!(families.len(), 1);
    assert!(families.contains_key("adguardhome"));
}

#[test]
fn test_discover_handles_subdirs_with_dockerfile() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("myimage").join("Dockerfile"),
        "# v2.0.0-1.0.0\nFROM ubuntu:latest\n",
    );

    let families = discover(&base);

    assert_eq!(families.len(), 1);
    let myimage = families.get("myimage").expect("myimage not found");
    assert_eq!(myimage.current_version.as_deref(), Some("v2.0.0-1.0.0"));
}

#[test]
fn test_discover_empty_dir() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    let discovery = discover_image_families(&base).expect("discover_image_families failed");

    assert!(discovery.families.is_empty());
    assert!(discovery.warnings.is_empty());
}

#[test]
fn test_discover_respects_existing_family_name() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("adguardhome.dockerfile"),
        "# v0.107.68-1.0.2\nFROM adguard/adguardhome:v0.107.68\n",
    );

    let families = discover(&base);

    let adguard = families.get("adguardhome").expect("adguardhome not found");
    assert_eq!(adguard.current.as_ref().unwrap().name, "adguardhome");
    assert_eq!(adguard.current.as_ref().unwrap().version, "v0.107.68-1.0.2");
    assert!(!adguard.current.as_ref().unwrap().local_tarball);
    assert!(adguard.current.as_ref().unwrap().build_log.is_empty());
}

#[test]
fn test_discover_multiple_versions_different_files() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("radarr.dockerfile"),
        "# v5.28.0.10274-1.0.3\nFROM radarr:5.28.0\n",
    );
    create_dockerfile(
        &base.join("sonarr.dockerfile"),
        "# v4.0.15.2941-1.0.0\nFROM sonarr:4.0.15\n",
    );
    create_dockerfile(
        &base.join("jellyfin.dockerfile"),
        "# v10.11.7-1.1.3\nFROM jellyfin:10.11.7\n",
    );
    create_dockerfile(
        &base.join("sabnzbd.dockerfile"),
        "# v4.5.5-1.0.3\nFROM sabnzbd:4.5.5\n",
    );

    let families = discover(&base);

    assert_eq!(families.len(), 4);

    let radarr = families.get("radarr").expect("radarr not found");
    assert_eq!(radarr.current_version.as_deref(), Some("v5.28.0.10274-1.0.3"));

    let sonarr = families.get("sonarr").expect("sonarr not found");
    assert_eq!(sonarr.current_version.as_deref(), Some("v4.0.15.2941-1.0.0"));

    let jellyfin = families.get("jellyfin").expect("jellyfin not found");
    assert_eq!(jellyfin.current_version.as_deref(), Some("v10.11.7-1.1.3"));

    let sabnzbd = families.get("sabnzbd").expect("sabnzbd not found");
    assert_eq!(sabnzbd.current_version.as_deref(), Some("v4.5.5-1.0.3"));
}

#[test]
fn test_discover_lubelogger() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("lubelogger.dockerfile"),
        "# v1.5.4-1.0.0\nFROM lubelogger/lubelogger:latest\n",
    );

    let families = discover(&base);

    let lubelogger = families.get("lubelogger").expect("lubelogger not found");
    assert_eq!(lubelogger.name, "lubelogger");
    assert_eq!(lubelogger.current_version.as_deref(), Some("v1.5.4-1.0.0"));
}

#[test]
fn test_discover_kavita() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("kavita.dockerfile"),
        "# v0.8.8.3-1.0.1\nFROM kavita/kavita:latest\n",
    );

    let families = discover(&base);

    let kavita = families.get("kavita").expect("kavita not found");
    assert_eq!(kavita.name, "kavita");
    assert_eq!(kavita.current_version.as_deref(), Some("v0.8.8.3-1.0.1"));
}

#[test]
fn test_discover_duplicate_name_produces_warning() {
    let temp = TempDir::new().unwrap();
    let base = temp.path().to_path_buf();

    create_dockerfile(
        &base.join("a").join("Dockerfile"),
        "# v1.0.0\nFROM ubuntu:latest\n",
    );
    create_dockerfile(
        &base.join("b").join("Dockerfile"),
        "# v1.0.0\nFROM ubuntu:latest\n",
    );

    // Both would have empty parent name in subdirs. Let's use two .dockerfile files in the
    // same parent with the same stem -- not possible. We need a different collision scenario.
    // Use two .dockerfile files with the same name in different subdirs:
    // subdir/Dockerfile (uses parent dir as name) and parent/samedir.dockerfile (uses file stem).
    // Actually easier: have a top-level "x" dir with Dockerfile AND x.dockerfile in another subdir.
    // The simplest: just have two .dockerfile files with the same name; that won't work either since
    // they live in different paths but have the same stem.
    // Realistic case: we have a/inner.dockerfile AND a/b/inner.dockerfile -- both have name "inner"
    // but the second is in a subdir. Both paths are walked. Both try to add "inner".
    let temp2 = TempDir::new().unwrap();
    let base2 = temp2.path().to_path_buf();
    create_dockerfile(
        &base2.join("inner.dockerfile"),
        "# v1.0.0\nFROM ubuntu:latest\n",
    );
    create_dockerfile(
        &base2.join("sub").join("inner.dockerfile"),
        "# v2.0.0\nFROM ubuntu:latest\n",
    );

    let discovery = discover_image_families(&base2).expect("discover_image_families failed");

    // Only one family should exist (the second one skipped with a warning)
    assert_eq!(discovery.families.len(), 1);
    assert!(!discovery.warnings.is_empty());
    assert!(
        discovery.warnings.iter().any(|w| w.contains("inner") && w.contains("Duplicate")),
        "expected duplicate warning, got: {:?}",
        discovery.warnings
    );
}

#[test]
fn test_discover_stores_paths_relative_to_base_dir() {
    // Regression: when base_dir contains "..", the stored dockerfile_path
    // and context_dir must remain relative to base_dir so they don't get
    // re-resolved against a changed cwd during the build.
    let temp = TempDir::new().unwrap();
    let base = temp.path();

    fs::create_dir_all(base.join("foo")).unwrap();

    create_dockerfile(
        &base.join("adguardhome.dockerfile"),
        "# v0.107.68-1.0.2\nFROM adguard/adguardhome:v0.107.68\n",
    );
    create_dockerfile(
        &base.join("immich_transfer").join("immich_transfer.dockerfile"),
        "# 1.1.1\nFROM python:3.13-slim\n",
    );
    create_dockerfile(
        &base.join("portal_knights_dedicated_server").join("Dockerfile"),
        "# v1.0.0-1.0.0\nFROM lancommander/wine:latest-amd64\n",
    );

    let base_with_dotdot = base.join("foo").join("..");
    let families = discover(&base_with_dotdot);

    let adguard = families.get("adguardhome").expect("adguardhome not found");
    let adguard_path = &adguard.current.as_ref().unwrap().dockerfile_path;
    let adguard_ctx = &adguard.current.as_ref().unwrap().context_dir;
    assert_eq!(adguard_path, &PathBuf::from("adguardhome.dockerfile"));
    assert_eq!(adguard_ctx, &PathBuf::from("."));
    assert!(!adguard_path.to_string_lossy().contains(".."));
    assert!(!adguard_ctx.to_string_lossy().contains(".."));

    let immich = families.get("immich_transfer").expect("immich_transfer not found");
    let immich_path = &immich.current.as_ref().unwrap().dockerfile_path;
    let immich_ctx = &immich.current.as_ref().unwrap().context_dir;
    assert_eq!(immich_path, &PathBuf::from("immich_transfer.dockerfile"));
    assert_eq!(immich_ctx, &PathBuf::from("immich_transfer"));
    assert!(!immich_path.to_string_lossy().contains(".."));
    assert!(!immich_ctx.to_string_lossy().contains(".."));

    let portal = families
        .get("portal_knights_dedicated_server")
        .expect("portal_knights_dedicated_server not found");
    let portal_path = &portal.current.as_ref().unwrap().dockerfile_path;
    let portal_ctx = &portal.current.as_ref().unwrap().context_dir;
    assert_eq!(portal_path, &PathBuf::from("Dockerfile"));
    assert_eq!(portal_ctx, &PathBuf::from("portal_knights_dedicated_server"));
    assert!(!portal_path.to_string_lossy().contains(".."));
    assert!(!portal_ctx.to_string_lossy().contains(".."));
}
