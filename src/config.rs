use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::Config;

pub fn load_config(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

    Ok(config)
}

/// Locate `config.toml`, checking (in order): an explicit override path,
/// `$RKE2_IMAGE_MANAGER_CONFIG`, `./config.toml`, `$XDG_CONFIG_HOME` (or
/// `~/.config`) under `rke2-image-manager/`, and finally alongside the
/// running executable.
pub fn find_config(override_path: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(anyhow::anyhow!(
            "Config file not found at {}",
            path.display()
        ));
    }

    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(env_path) = std::env::var("RKE2_IMAGE_MANAGER_CONFIG") {
        candidates.push(PathBuf::from(env_path));
    }

    candidates.push(PathBuf::from("config.toml"));

    if let Some(xdg_config) = xdg_config_dir() {
        candidates.push(xdg_config.join("rke2-image-manager").join("config.toml"));
    }

    if let Some(path) = exe_dir_config() {
        candidates.push(path);
    }

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(anyhow::anyhow!(
        "config.toml not found. Copy config.example.toml to config.toml and edit it."
    ))
}

fn xdg_config_dir() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("XDG_CONFIG_HOME") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    std::env::var("HOME").ok().map(|home| PathBuf::from(home).join(".config"))
}

fn exe_dir_config() -> Option<PathBuf> {
    let exe_dir = std::env::current_exe().ok()?;
    let exe_parent = exe_dir.parent()?;
    Some(exe_parent.join("config.toml"))
}
