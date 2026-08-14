use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuildState {
    #[default]
    Idle,
    Building,
    Failed(String),
    Success,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedImage {
    pub name: String,
    pub version: String,
    pub dockerfile_path: PathBuf,
    pub context_dir: PathBuf,
    pub local_tarball: bool,
    pub server_presence: BTreeMap<String, bool>,
    pub build_state: BuildState,
    pub build_log: Vec<String>,
}

impl ManagedImage {
    pub fn present_on_any_server(&self) -> bool {
        self.server_presence.values().any(|v| *v)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageFamily {
    pub name: String,
    pub current_version: Option<String>,
    pub current: Option<ManagedImage>,
    pub stale_versions: BTreeSet<String>,
    pub remote_presence: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownTarball {
    pub filename: String,
    pub servers: BTreeSet<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageTableRow {
    Current(ManagedImage),
    Stale {
        family_name: String,
        version: String,
        servers: BTreeSet<String>,
    },
    Unknown(UnknownTarball),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub name: String,
    pub host: String,
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub identity_file: Option<String>,
}

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub paths: PathsConfig,
    #[serde(default)]
    pub servers: Vec<Server>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            paths: PathsConfig {
                server_images_dir: String::new(),
                staging_dir: String::new(),
                rke2_images_dir: String::new(),
                image_registry: String::new(),
            },
            servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub server_images_dir: String,
    pub staging_dir: String,
    pub rke2_images_dir: String,
    pub image_registry: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanStatus {
    Pending,
    Ok,
    Error(String),
}
