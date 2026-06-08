use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use tokio::sync::mpsc;

use crate::models::{ImageFamily, Server, UnknownTarball};

pub enum RemoteEvent {
    Scan {
        server_name: String,
        filenames: Result<Vec<String>, String>,
    },
    Deploy {
        server_name: String,
        success: bool,
        error: Option<String>,
    },
    Remove {
        server_name: String,
        tarball: String,
        success: bool,
        error: Option<String>,
    },
}

pub fn start_remote_scan(
    servers: Vec<Server>,
    rke2_images_dir: String,
) -> mpsc::UnboundedReceiver<RemoteEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    for server in servers {
        let tx = tx.clone();
        let dir = rke2_images_dir.clone();
        let server_name = server.name.clone();

        tokio::task::spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                list_remote_tarballs(&server, &dir)
            })
            .await
            .unwrap_or_else(|e| Err(format!("Join error: {}", e)));

            let _ = tx.send(RemoteEvent::Scan {
                server_name,
                filenames: result,
            });
        });
    }

    drop(tx);
    rx
}

pub fn apply_scan_results(
    families: &mut BTreeMap<String, ImageFamily>,
    unknowns: &mut Vec<UnknownTarball>,
    server_name: &str,
    filenames: &[String],
) {
    let family_names: BTreeSet<String> = families.keys().cloned().collect();
    let mut unknown_map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for raw in filenames {
        let filename = raw.trim();
        if !filename.ends_with(".tar") {
            continue;
        }

        let stem = filename.strip_suffix(".tar").unwrap_or(filename);

        let (matched_family, version_part) = match_tarball(&family_names, stem);

        if let Some(family_name) = matched_family {
            let version = version_part.unwrap_or("unknown");

            if let Some(family) = families.get_mut(family_name) {
                let effective_version = if version.is_empty() {
                    family.current_version.as_deref().unwrap_or("unknown")
                } else {
                    version
                };

                if Some(effective_version) == family.current_version.as_deref() {
                    if let Some(ref mut current) = family.current {
                        current.server_presence.insert(server_name.to_string(), true);
                    }
                } else {
                    family.stale_versions.insert(effective_version.to_string());
                    family
                        .remote_presence
                        .entry(effective_version.to_string())
                        .or_default()
                        .insert(server_name.to_string());
                }
            }
        } else {
            unknown_map
                .entry(filename.to_string())
                .or_default()
                .insert(server_name.to_string());
        }
    }

    for (filename, servers) in unknown_map {
        if let Some(existing) = unknowns.iter_mut().find(|u| u.filename == filename) {
            existing.servers.extend(servers);
        } else {
            unknowns.push(UnknownTarball { filename, servers });
        }
    }

    for family in families.values_mut() {
        if let Some(ref mut current) = family.current {
            current
                .server_presence
                .entry(server_name.to_string())
                .or_insert(false);
        }
    }
}

pub fn refresh_local_tarballs(
    families: &mut BTreeMap<String, ImageFamily>,
    staging_dir: &Path,
) {
    for family in families.values_mut() {
        if let Some(ref mut current) = family.current {
            let tarball_name = format!("{}-{}.tar", current.name, current.version);
            let path = staging_dir.join(&tarball_name);
            current.local_tarball = path.is_file();
        }
    }
}

fn match_tarball<'a>(
    family_names: &'a BTreeSet<String>,
    stem: &'a str,
) -> (Option<&'a str>, Option<&'a str>) {
    let mut best_match: Option<(&str, &str)> = None;

    for family_name in family_names {
        if stem == *family_name {
            return (Some(family_name.as_str()), Some(""));
        }

        let prefix = format!("{}-", family_name);
        if stem.starts_with(&prefix) {
            let version_part = &stem[prefix.len()..];
            match best_match {
                Some((existing, _)) if family_name.len() <= existing.len() => {}
                _ => best_match = Some((family_name.as_str(), version_part)),
            }
        }
    }

    best_match
        .map(|(name, version)| (Some(name), Some(version)))
        .unwrap_or((None, None))
}

fn list_remote_tarballs(server: &Server, dir: &str) -> Result<Vec<String>, String> {
    let output = run_ssh_command(server, &["sudo", "-n", "ls", "--color=never", "-1", dir])?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let filenames: Vec<String> = stdout
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(filenames)
}

fn run_ssh_command(server: &Server, remote_cmd: &[&str]) -> Result<std::process::Output, String> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-o").arg("BatchMode=yes");
    cmd.arg("-o").arg("ConnectTimeout=15");

    if server.port != 22 {
        cmd.arg("-p").arg(server.port.to_string());
    }

    if let Some(ref identity) = server.identity_file {
        cmd.arg("-i").arg(identity);
    }

    cmd.arg(format!("{}@{}", server.user, server.host));
    cmd.arg("--");
    for arg in remote_cmd {
        cmd.arg(arg);
    }

    cmd.output()
        .map_err(|e| format!("SSH to {} failed: {}", server.name, e))
}

pub async fn deploy_tarball(
    server: &Server,
    local_path: &Path,
    remote_dir: &str,
) -> Result<(), String> {
    let server_name = server.name.clone();
    let local_path = local_path.to_path_buf();
    let remote_dir = remote_dir.to_string();
    let tarball_name = local_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("Invalid local tarball path: {}", local_path.display()))?
        .to_string();
    let tmp_path = format!("/tmp/{}", tarball_name);
    let dest_path = format!("{}/{}", remote_dir, tarball_name);

    let server_for_scp = server.clone();
    let local_path_for_scp = local_path.clone();
    let tmp_path_for_scp = tmp_path.clone();

    let output = tokio::task::spawn_blocking(move || {
        let mut scp = Command::new("scp");
        scp.arg("-o").arg("BatchMode=yes");
        scp.arg("-o").arg("ConnectTimeout=15");

        if server_for_scp.port != 22 {
            scp.arg("-P").arg(server_for_scp.port.to_string());
        }

        if let Some(ref identity) = server_for_scp.identity_file {
            scp.arg("-i").arg(identity);
        }

        scp.arg(&local_path_for_scp);
        scp.arg(format!(
            "{}@{}:{}",
            server_for_scp.user, server_for_scp.host, tmp_path_for_scp
        ));

        scp.output()
    })
    .await
    .map_err(|e| format!("SCP spawn failed: {}", e))?
    .map_err(|e| format!("SCP to {} failed: {}", server_name, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "SCP upload to {} failed: {}",
            server_name,
            stderr.trim()
        ));
    }

    let server_for_mv = server.clone();
    let tmp_path_for_mv = tmp_path.clone();
    let dest_path_for_mv = dest_path.clone();

    let mv_output = tokio::task::spawn_blocking(move || {
        let mut ssh = Command::new("ssh");
        ssh.arg("-o").arg("BatchMode=yes");
        ssh.arg("-o").arg("ConnectTimeout=15");

        if server_for_mv.port != 22 {
            ssh.arg("-p").arg(server_for_mv.port.to_string());
        }

        if let Some(ref identity) = server_for_mv.identity_file {
            ssh.arg("-i").arg(identity);
        }

        ssh.arg(format!("{}@{}", server_for_mv.user, server_for_mv.host));
        ssh.arg("--");
        ssh.arg("sudo").arg("-n").arg("mv").arg(&tmp_path_for_mv).arg(&dest_path_for_mv);

        ssh.output()
    })
    .await
    .map_err(|e| format!("SSH mv spawn failed: {}", e))?
    .map_err(|e| format!("SSH mv on {} failed: {}", server_name, e))?;

    if !mv_output.status.success() {
        let mv_stderr = String::from_utf8_lossy(&mv_output.stderr);
        let server_for_cleanup = server.clone();
        let tmp_path_for_cleanup = tmp_path.clone();

        let _ = tokio::task::spawn_blocking(move || -> std::io::Result<std::process::Output> {
            let mut ssh = Command::new("ssh");
            ssh.arg("-o").arg("BatchMode=yes");
            ssh.arg("-o").arg("ConnectTimeout=15");

            if server_for_cleanup.port != 22 {
                ssh.arg("-p").arg(server_for_cleanup.port.to_string());
            }

            if let Some(ref identity) = server_for_cleanup.identity_file {
                ssh.arg("-i").arg(identity);
            }

            ssh.arg(format!("{}@{}", server_for_cleanup.user, server_for_cleanup.host));
            ssh.arg("--");
            ssh.arg("sudo").arg("-n").arg("rm").arg(&tmp_path_for_cleanup);
            ssh.output()
        })
        .await
        .ok()
        .and_then(|r| r.ok());

        return Err(format!(
            "sudo mv to {} failed: {}",
            server_name,
            mv_stderr.trim()
        ));
    }

    Ok(())
}

pub async fn remove_tarball(
    server: &Server,
    tarball: &str,
    remote_dir: &str,
) -> Result<(), String> {
    let server_name = server.name.clone();
    let server_for_blocking = server.clone();
    let tarball = tarball.to_string();
    let remote_dir = remote_dir.to_string();
    let target = format!("{}/{}", remote_dir, tarball);

    let output = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ssh");
        cmd.arg("-o").arg("BatchMode=yes");
        cmd.arg("-o").arg("ConnectTimeout=15");

        if server_for_blocking.port != 22 {
            cmd.arg("-p").arg(server_for_blocking.port.to_string());
        }

        if let Some(ref identity) = server_for_blocking.identity_file {
            cmd.arg("-i").arg(identity);
        }

        cmd.arg(format!("{}@{}", server_for_blocking.user, server_for_blocking.host));
        cmd.arg("--");
        cmd.arg("sudo").arg("-n").arg("rm").arg(&target);

        cmd.output()
    })
    .await
    .map_err(|e| format!("SSH spawn failed: {}", e))?
    .map_err(|e| format!("SSH rm on {} failed: {}", server_name, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("SSH rm failed on {}: {}", server_name, stderr.trim()));
    }

    Ok(())
}

pub fn tarball_filename(image_name: &str, version: &str) -> String {
    format!("{}-{}.tar", image_name, version)
}
