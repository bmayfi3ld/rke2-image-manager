use std::path::PathBuf;

use rke2_image_manager::models::{Config, PathsConfig, Server};

#[test]
fn test_parse_config_example() {
    let config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.toml");

    let content = std::fs::read_to_string(&config_path)
        .expect("Failed to read config.example.toml");

    let config: Config = toml::from_str(&content)
        .expect("Failed to parse config.example.toml");

    assert_eq!(config.paths.server_images_dir, "../k8s-config/server_images");
    assert_eq!(config.paths.staging_dir, "/tmp/k8s-images");
    assert_eq!(config.paths.rke2_images_dir, "/var/lib/rancher/rke2/agent/images");
    assert_eq!(config.paths.image_registry, "registry.local");

    assert_eq!(config.servers.len(), 3);

    let s4 = &config.servers[0];
    assert_eq!(s4.name, "bamserve4");
    assert_eq!(s4.host, "192.168.2.6");
    assert_eq!(s4.user, "mayfiba");
    assert_eq!(s4.port, 22);
    assert!(s4.identity_file.is_none());

    let s5 = &config.servers[1];
    assert_eq!(s5.name, "bamserve5");
    assert_eq!(s5.host, "192.168.2.11");
    assert_eq!(s5.user, "mayfiba");
    assert_eq!(s5.port, 22);

    let s6 = &config.servers[2];
    assert_eq!(s6.name, "bamserve6");
    assert_eq!(s6.host, "192.168.2.12");
    assert_eq!(s6.user, "mayfiba");
}

#[test]
fn test_server_with_optional_fields() {
    let toml_str = r#"
[paths]
server_images_dir = "."
staging_dir = "/tmp"
rke2_images_dir = "/var/lib/rancher/rke2/agent/images"
image_registry = "registry.local"

[[servers]]
name = "custom-server"
host = "192.168.1.100"
user = "admin"
port = 2222
identity_file = "/home/user/.ssh/custom_key"
"#;

    let config: Config = toml::from_str(toml_str).expect("Failed to parse TOML");

    assert_eq!(config.servers.len(), 1);
    let s = &config.servers[0];
    assert_eq!(s.name, "custom-server");
    assert_eq!(s.host, "192.168.1.100");
    assert_eq!(s.user, "admin");
    assert_eq!(s.port, 2222);
    assert_eq!(s.identity_file.as_deref(), Some("/home/user/.ssh/custom_key"));
}

#[test]
fn test_paths_config_serialization() {
    let paths = PathsConfig {
        server_images_dir: "../k8s-config/server_images".to_string(),
        staging_dir: "/tmp/k8s-images".to_string(),
        rke2_images_dir: "/var/lib/rancher/rke2/agent/images".to_string(),
        image_registry: "registry.local".to_string(),
    };

    let serialized = toml::to_string(&paths).expect("Failed to serialize PathsConfig");
    assert!(serialized.contains("server_images_dir"));
    assert!(serialized.contains("../k8s-config/server_images"));
}

#[test]
fn test_server_struct_clone() {
    let server = Server {
        name: "test".to_string(),
        host: "192.168.1.1".to_string(),
        user: "admin".to_string(),
        port: 22,
        identity_file: Some("/path/to/key".to_string()),
    };

    let cloned = server.clone();
    assert_eq!(cloned.name, server.name);
    assert_eq!(cloned.host, server.host);
    assert_eq!(cloned.identity_file, server.identity_file);
}
