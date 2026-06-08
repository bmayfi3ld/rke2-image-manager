use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::models::Config;

pub fn load_config(path: &Path) -> Result<Config> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: Config = toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

    Ok(config)
}
