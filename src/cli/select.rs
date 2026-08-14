use std::collections::{BTreeMap, BTreeSet};

use crate::models::{ImageFamily, ManagedImage, Server, UnknownTarball};

/// A user-supplied target: `name`, `name:version`, or a bare `.tar` filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageSelector {
    Name(String),
    NameVersion(String, String),
    Tarball(String),
}

impl ImageSelector {
    pub fn parse(raw: &str) -> Self {
        if raw.ends_with(".tar") {
            return ImageSelector::Tarball(raw.to_string());
        }
        match raw.split_once(':') {
            Some((name, version)) if !name.is_empty() && !version.is_empty() => {
                ImageSelector::NameVersion(name.to_string(), version.to_string())
            }
            _ => ImageSelector::Name(raw.to_string()),
        }
    }
}

impl std::fmt::Display for ImageSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageSelector::Name(n) => write!(f, "{}", n),
            ImageSelector::NameVersion(n, v) => write!(f, "{}:{}", n, v),
            ImageSelector::Tarball(t) => write!(f, "{}", t),
        }
    }
}

/// A selector resolved against a discovered set of families/unknown tarballs.
#[derive(Debug, Clone)]
pub enum ResolvedTarget {
    Current {
        family_name: String,
        image: ManagedImage,
    },
    Stale {
        family_name: String,
        version: String,
        servers: BTreeSet<String>,
    },
    Unknown {
        filename: String,
        servers: BTreeSet<String>,
    },
}

impl ResolvedTarget {
    pub fn tarball_filename(&self) -> String {
        match self {
            ResolvedTarget::Current { image, .. } => {
                crate::remote::tarball_filename(&image.name, &image.version)
            }
            ResolvedTarget::Stale {
                family_name,
                version,
                ..
            } => crate::remote::tarball_filename(family_name, version),
            ResolvedTarget::Unknown { filename, .. } => filename.clone(),
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            ResolvedTarget::Current { image, .. } => format!("{}:{}", image.name, image.version),
            ResolvedTarget::Stale {
                family_name,
                version,
                ..
            } => format!("{}:{}", family_name, version),
            ResolvedTarget::Unknown { filename, .. } => filename.clone(),
        }
    }
}

/// Resolve a selector against the discovered families / unknown tarballs.
/// The `Err` string is the "unknown image name" error surfaced to the user.
pub fn resolve(
    selector: &ImageSelector,
    families: &BTreeMap<String, ImageFamily>,
    unknowns: &[UnknownTarball],
) -> Result<ResolvedTarget, String> {
    match selector {
        ImageSelector::Name(name) => {
            let family = families
                .get(name)
                .ok_or_else(|| unknown_name_error(name, families))?;
            let image = family
                .current
                .clone()
                .ok_or_else(|| format!("Image family '{}' has no current version", name))?;
            Ok(ResolvedTarget::Current {
                family_name: name.clone(),
                image,
            })
        }
        ImageSelector::NameVersion(name, version) => {
            let family = families
                .get(name)
                .ok_or_else(|| unknown_name_error(name, families))?;

            if let Some(ref current) = family.current {
                if &current.version == version {
                    return Ok(ResolvedTarget::Current {
                        family_name: name.clone(),
                        image: current.clone(),
                    });
                }
            }

            if family.stale_versions.contains(version) {
                let servers = family
                    .remote_presence
                    .get(version)
                    .cloned()
                    .unwrap_or_default();
                return Ok(ResolvedTarget::Stale {
                    family_name: name.clone(),
                    version: version.clone(),
                    servers,
                });
            }

            Err(format!(
                "'{}' has no known version '{}' (current: {})",
                name,
                version,
                family.current_version.as_deref().unwrap_or("none")
            ))
        }
        ImageSelector::Tarball(filename) => {
            let unknown = unknowns
                .iter()
                .find(|u| &u.filename == filename)
                .ok_or_else(|| format!("No unknown tarball named '{}'", filename))?;
            Ok(ResolvedTarget::Unknown {
                filename: unknown.filename.clone(),
                servers: unknown.servers.clone(),
            })
        }
    }
}

/// Resolve a raw arg to a family's *current* image, rejecting `:version` and
/// `.tar` selector forms. Used by commands (`build`, `deploy`) that only
/// ever operate on the current version.
pub fn resolve_current(
    raw: &str,
    families: &BTreeMap<String, ImageFamily>,
) -> Result<ManagedImage, String> {
    let selector = ImageSelector::parse(raw);
    if !matches!(selector, ImageSelector::Name(_)) {
        return Err(format!(
            "'{}' -- only a bare family name is accepted here (version comes from the Dockerfile)",
            raw
        ));
    }
    match resolve(&selector, families, &[])? {
        ResolvedTarget::Current { image, .. } => Ok(image),
        _ => unreachable!("Name selectors only resolve to Current"),
    }
}

/// Resolve `-s/--server` names against the configured server list. Empty
/// input means "every configured server" -- the config file's `[[servers]]`
/// list *is* the managed set. Unknown names are a hard error.
pub fn resolve_servers(names: &[String], configured: &[Server]) -> Result<Vec<Server>, String> {
    if names.is_empty() {
        return Ok(configured.to_vec());
    }

    let mut resolved = Vec::new();
    for name in names {
        match configured.iter().find(|s| &s.name == name) {
            Some(server) => resolved.push(server.clone()),
            None => {
                let known: Vec<&str> = configured.iter().map(|s| s.name.as_str()).collect();
                return Err(format!(
                    "Unknown server '{}'. Configured servers: {}",
                    name,
                    known.join(", ")
                ));
            }
        }
    }
    Ok(resolved)
}

fn unknown_name_error(name: &str, families: &BTreeMap<String, ImageFamily>) -> String {
    let known: Vec<&str> = families.keys().map(|s| s.as_str()).collect();
    format!("Unknown image '{}'. Known images: {}", name, known.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BuildState, ImageFamily, ManagedImage};
    use std::path::PathBuf;

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
    fn test_parse_name() {
        assert_eq!(
            ImageSelector::parse("adguardhome"),
            ImageSelector::Name("adguardhome".to_string())
        );
    }

    #[test]
    fn test_parse_name_version() {
        assert_eq!(
            ImageSelector::parse("adguardhome:v1.0.0"),
            ImageSelector::NameVersion("adguardhome".to_string(), "v1.0.0".to_string())
        );
    }

    #[test]
    fn test_parse_tarball() {
        assert_eq!(
            ImageSelector::parse("mystery.tar"),
            ImageSelector::Tarball("mystery.tar".to_string())
        );
    }

    #[test]
    fn test_parse_tarball_takes_priority_over_colon() {
        // a literal filename containing a colon before .tar is still a tarball selector
        assert_eq!(
            ImageSelector::parse("weird:name.tar"),
            ImageSelector::Tarball("weird:name.tar".to_string())
        );
    }

    #[test]
    fn test_resolve_current_by_name() {
        let mut families = BTreeMap::new();
        families.insert("adguardhome".to_string(), make_family("adguardhome", "v1"));
        let selector = ImageSelector::Name("adguardhome".to_string());

        let resolved = resolve(&selector, &families, &[]).unwrap();
        match resolved {
            ResolvedTarget::Current { family_name, image } => {
                assert_eq!(family_name, "adguardhome");
                assert_eq!(image.version, "v1");
            }
            _ => panic!("expected Current"),
        }
    }

    #[test]
    fn test_resolve_unknown_name_errors_with_known_list() {
        let mut families = BTreeMap::new();
        families.insert("adguardhome".to_string(), make_family("adguardhome", "v1"));
        let selector = ImageSelector::Name("nope".to_string());

        let err = resolve(&selector, &families, &[]).unwrap_err();
        assert!(err.contains("adguardhome"));
        assert!(err.contains("nope"));
    }

    #[test]
    fn test_resolve_stale_version() {
        let mut family = make_family("adguardhome", "v2");
        family.stale_versions.insert("v1".to_string());
        family
            .remote_presence
            .insert("v1".to_string(), std::iter::once("bamserve4".to_string()).collect());
        let mut families = BTreeMap::new();
        families.insert("adguardhome".to_string(), family);

        let selector = ImageSelector::NameVersion("adguardhome".to_string(), "v1".to_string());
        let resolved = resolve(&selector, &families, &[]).unwrap();
        match resolved {
            ResolvedTarget::Stale {
                family_name,
                version,
                servers,
            } => {
                assert_eq!(family_name, "adguardhome");
                assert_eq!(version, "v1");
                assert!(servers.contains("bamserve4"));
            }
            _ => panic!("expected Stale"),
        }
    }

    #[test]
    fn test_resolve_unknown_version_errors() {
        let families_map = {
            let mut m = BTreeMap::new();
            m.insert("adguardhome".to_string(), make_family("adguardhome", "v2"));
            m
        };
        let selector = ImageSelector::NameVersion("adguardhome".to_string(), "v99".to_string());
        let err = resolve(&selector, &families_map, &[]).unwrap_err();
        assert!(err.contains("v99"));
    }

    #[test]
    fn test_resolve_unknown_tarball() {
        let unknowns = vec![UnknownTarball {
            filename: "mystery.tar".to_string(),
            servers: std::iter::once("bamserve4".to_string()).collect(),
        }];
        let selector = ImageSelector::Tarball("mystery.tar".to_string());
        let resolved = resolve(&selector, &BTreeMap::new(), &unknowns).unwrap();
        match resolved {
            ResolvedTarget::Unknown { filename, servers } => {
                assert_eq!(filename, "mystery.tar");
                assert!(servers.contains("bamserve4"));
            }
            _ => panic!("expected Unknown"),
        }
    }

    #[test]
    fn test_resolve_missing_tarball_errors() {
        let selector = ImageSelector::Tarball("ghost.tar".to_string());
        let err = resolve(&selector, &BTreeMap::new(), &[]).unwrap_err();
        assert!(err.contains("ghost.tar"));
    }
}
