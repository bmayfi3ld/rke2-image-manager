use std::collections::BTreeMap;

use rke2_image_manager::cli::output::render_table;
use rke2_image_manager::cli::select::{resolve_servers, ImageSelector};
use rke2_image_manager::inventory::{any_scan_error, Inventory};
use rke2_image_manager::models::{ScanStatus, Server};

fn make_server(name: &str) -> Server {
    Server {
        name: name.to_string(),
        host: format!("{}.local", name),
        user: "deploy".to_string(),
        port: 22,
        identity_file: None,
    }
}

// -- selector parsing --------------------------------------------------

#[test]
fn test_selector_parse_plain_name() {
    assert_eq!(
        ImageSelector::parse("caddy"),
        ImageSelector::Name("caddy".to_string())
    );
}

#[test]
fn test_selector_parse_name_with_dotted_version() {
    assert_eq!(
        ImageSelector::parse("jellyfin:v10.11.7-1.2.0"),
        ImageSelector::NameVersion("jellyfin".to_string(), "v10.11.7-1.2.0".to_string())
    );
}

#[test]
fn test_selector_parse_bare_tarball_filename() {
    assert_eq!(
        ImageSelector::parse("crowdsec-v1.6.4-1.0.0.tar"),
        ImageSelector::Tarball("crowdsec-v1.6.4-1.0.0.tar".to_string())
    );
}

#[test]
fn test_selector_parse_empty_string_is_a_name() {
    // Degenerate input; resolution will reject it, but parsing shouldn't panic.
    assert_eq!(ImageSelector::parse(""), ImageSelector::Name(String::new()));
}

#[test]
fn test_selector_parse_trailing_colon_is_a_name() {
    // "foo:" has an empty version half, so it's not a valid NameVersion split.
    assert_eq!(
        ImageSelector::parse("foo:"),
        ImageSelector::Name("foo:".to_string())
    );
}

// -- server-selection resolution ----------------------------------------

#[test]
fn test_resolve_servers_empty_selection_returns_all_configured() {
    let configured = vec![make_server("bamserve4"), make_server("bamserve5")];
    let resolved = resolve_servers(&[], &configured).unwrap();
    assert_eq!(resolved.len(), 2);
}

#[test]
fn test_resolve_servers_narrows_to_named_subset() {
    let configured = vec![
        make_server("bamserve4"),
        make_server("bamserve5"),
        make_server("bamserve6"),
    ];
    let resolved = resolve_servers(&["bamserve5".to_string()], &configured).unwrap();
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].name, "bamserve5");
}

#[test]
fn test_resolve_servers_unknown_name_is_hard_error() {
    let configured = vec![make_server("bamserve4")];
    let err = resolve_servers(&["nope".to_string()], &configured).unwrap_err();
    assert!(err.contains("nope"));
    assert!(err.contains("bamserve4"));
}

// -- table rendering ------------------------------------------------------

#[test]
fn test_render_table_multi_server_columns() {
    let headers = ["NAME", "VERSION", "LOCAL", "bamserve4", "bamserve5", "STATE"];
    let rows = vec![
        vec![
            "adguardhome".to_string(),
            "v0.107.68-1.0.2".to_string(),
            "yes".to_string(),
            "yes".to_string(),
            "no".to_string(),
            "ok".to_string(),
        ],
        vec![
            "  (stale)".to_string(),
            "v0.107.63-1.0.2".to_string(),
            "-".to_string(),
            "yes".to_string(),
            "yes".to_string(),
            "stale".to_string(),
        ],
    ];
    let table = render_table(&headers, &rows);
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines.len(), 3);
    // Every column should start at the same offset across all rows.
    let header_local_offset = lines[0].find("LOCAL").unwrap();
    let row_local_offset = lines[1].find("yes").unwrap();
    assert!(row_local_offset >= header_local_offset);
    assert!(lines[2].trim_start().starts_with("(stale)"));
}

#[test]
fn test_render_table_no_trailing_whitespace() {
    let table = render_table(&["A", "B"], &[vec!["x".to_string(), "y".to_string()]]);
    for line in table.lines() {
        assert_eq!(line, line.trim_end());
    }
}

// -- exit-code mapping ------------------------------------------------------

fn inventory_with_scan_status(status: BTreeMap<String, ScanStatus>) -> Inventory {
    Inventory {
        families: BTreeMap::new(),
        unknowns: Vec::new(),
        server_names: status.keys().cloned().collect(),
        scan_status: status,
        warnings: Vec::new(),
    }
}

#[test]
fn test_any_scan_error_false_when_all_ok() {
    let mut status = BTreeMap::new();
    status.insert("bamserve4".to_string(), ScanStatus::Ok);
    status.insert("bamserve5".to_string(), ScanStatus::Ok);
    let inv = inventory_with_scan_status(status);
    assert!(!any_scan_error(&inv));
}

#[test]
fn test_any_scan_error_true_when_one_errors() {
    let mut status = BTreeMap::new();
    status.insert("bamserve4".to_string(), ScanStatus::Ok);
    status.insert(
        "bamserve5".to_string(),
        ScanStatus::Error("SSH timeout".to_string()),
    );
    let inv = inventory_with_scan_status(status);
    assert!(any_scan_error(&inv));
}

#[test]
fn test_any_scan_error_false_when_pending() {
    let mut status = BTreeMap::new();
    status.insert("bamserve4".to_string(), ScanStatus::Pending);
    let inv = inventory_with_scan_status(status);
    assert!(!any_scan_error(&inv));
}
