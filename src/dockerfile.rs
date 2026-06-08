use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use crate::models::{BuildState, ImageFamily, ManagedImage};

pub struct DiscoveryResult {
    pub families: BTreeMap<String, ImageFamily>,
    pub warnings: Vec<String>,
}

pub fn discover_image_families(server_images_dir: &Path) -> Result<DiscoveryResult> {
    let mut families: BTreeMap<String, ImageFamily> = BTreeMap::new();
    let mut warnings: Vec<String> = Vec::new();

    scan_dir(server_images_dir, server_images_dir, &mut families, &mut warnings)?;

    Ok(DiscoveryResult { families, warnings })
}

fn scan_dir(
    dir: &Path,
    base_dir: &Path,
    families: &mut BTreeMap<String, ImageFamily>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }

    let entries: Vec<PathBuf> = match fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(e) => {
            warnings.push(format!("Failed to read {}: {}", dir.display(), e));
            return Ok(());
        }
    };

    for path in entries {
        if path.is_dir() {
            scan_dir(&path, base_dir, families, warnings)?;
        } else if path.is_file() {
            let (name, dockerfile_path, context_dir) = match classify_dockerfile(&path, base_dir) {
                Some(result) => result,
                None => continue,
            };

            if families.contains_key(&name) {
                warnings.push(format!(
                    "Duplicate image name '{}' at {}; skipping",
                    name,
                    path.display()
                ));
                continue;
            }

            let version = extract_version(&path).unwrap_or_else(|| {
                warnings.push(format!(
                    "No version comment found in {} -- using 'latest'",
                    path.display()
                ));
                "latest".to_string()
            });

            let managed = ManagedImage {
                name: name.clone(),
                version: version.clone(),
                dockerfile_path,
                context_dir,
                local_tarball: false,
                server_presence: BTreeMap::new(),
                build_state: BuildState::Idle,
                build_log: Vec::new(),
            };

            families.insert(
                name.clone(),
                ImageFamily {
                    name,
                    current_version: Some(version),
                    current: Some(managed),
                    stale_versions: BTreeSet::new(),
                    remote_presence: BTreeMap::new(),
                },
            );
        }
    }

    Ok(())
}

fn classify_dockerfile(
    path: &Path,
    base_dir: &Path,
) -> Option<(String, PathBuf, PathBuf)> {
    let file_name = path.file_name()?.to_str()?;

    if file_name == "Dockerfile" {
        let parent_dir = path.parent()?;
        let relative_parent = parent_dir.strip_prefix(base_dir).ok()?;
        if relative_parent.as_os_str().is_empty() {
            return None;
        }
        let name = parent_dir.file_name()?.to_str()?.to_string();
        let context_dir = relative_parent.to_path_buf();
        let dockerfile_path = path.strip_prefix(parent_dir).ok()?.to_path_buf();
        return Some((name, dockerfile_path, context_dir));
    }

    if file_name.ends_with(".dockerfile") {
        let stem = file_name.strip_suffix(".dockerfile")?;
        let parent_dir = path.parent()?;
        let name = stem.to_string();
        let context_dir = parent_dir
            .strip_prefix(base_dir)
            .ok()
            .map(|p| {
                if p.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    p.to_path_buf()
                }
            })?;
        let dockerfile_path = PathBuf::from(file_name);
        return Some((name, dockerfile_path, context_dir));
    }

    None
}

fn extract_version(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);

    for line in reader.lines() {
        let line = line.ok()?;
        let trimmed = line.trim();

        if trimmed.starts_with('#') {
            let comment = trimmed.trim_start_matches('#').trim();

            if let Some(version) = parse_version_comment(comment) {
                return Some(version);
            }
        }
    }

    None
}

fn parse_version_comment(comment: &str) -> Option<String> {
    let comment = comment.trim();

    if comment.is_empty() {
        return None;
    }

    let first_char = comment.chars().next()?;

    if first_char == 'v' {
        let rest = &comment[1..];
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            let end = rest
                .find(|c: char| c.is_whitespace())
                .unwrap_or(rest.len());

            let mut version = String::with_capacity(end + 1);
            version.push('v');
            version.push_str(&rest[..end]);
            return Some(version);
        }
    }

    if first_char.is_ascii_digit() {
        let end = comment
            .find(|c: char| c.is_whitespace())
            .unwrap_or(comment.len());
        return Some(comment[..end].to_string());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_comment_v_prefix() {
        assert_eq!(parse_version_comment("v0.107.68-1.0.2"), Some("v0.107.68-1.0.2".to_string()));
        assert_eq!(parse_version_comment("v1.0"), Some("v1.0".to_string()));
    }

    #[test]
    fn test_parse_version_comment_digit_prefix() {
        assert_eq!(parse_version_comment("2.10.2-0.4.0"), Some("2.10.2-0.4.0".to_string()));
        assert_eq!(parse_version_comment("1.1.1"), Some("1.1.1".to_string()));
    }

    #[test]
    fn test_parse_version_comment_trailing_text() {
        assert_eq!(parse_version_comment("v1.0.0-1.0.0 trailing"), Some("v1.0.0-1.0.0".to_string()));
        assert_eq!(parse_version_comment("1.2.3 stuff"), Some("1.2.3".to_string()));
    }

    #[test]
    fn test_parse_version_comment_rejects_non_version() {
        assert_eq!(parse_version_comment("version 1.2.3"), None);
        assert_eq!(parse_version_comment(""), None);
        assert_eq!(parse_version_comment("latest"), None);
    }
}
