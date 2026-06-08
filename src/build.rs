use std::io::BufRead;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::models::ManagedImage;

static LOG_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub enum BuildEvent {
    LogLine {
        image_name: String,
        line: String,
        seq: u64,
    },
    BuildComplete {
        image_name: String,
        result: Result<(), String>,
        tarball_written: bool,
    },
}

pub struct BuildHandle {
    kill_tx: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl BuildHandle {
    pub fn cancel(mut self) {
        if let Some(tx) = self.kill_tx.take() {
            let _ = tx.send(());
        }
        if let Some(j) = self.join.take() {
            j.abort();
        }
    }
}

impl Drop for BuildHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.kill_tx.take() {
            let _ = tx.send(());
        }
        if let Some(j) = self.join.take() {
            j.abort();
        }
    }
}

pub fn start_build(
    image: ManagedImage,
    image_registry: String,
    staging_dir: String,
    server_images_dir: std::path::PathBuf,
) -> (mpsc::UnboundedReceiver<BuildEvent>, BuildHandle) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (kill_tx, kill_rx) = oneshot::channel();

    let join = tokio::task::spawn(async move {
        let name = image.name.clone();
        let version = image.version.clone();
        let dockerfile = image.dockerfile_path.clone();
        let context_relative = image.context_dir.clone();
        let context = server_images_dir.join(&context_relative);

        let tag = format!("{}/{}:{}", image_registry, name, version);
        let output_name = format!("{}-{}.tar", name, version);
        let output_path = Path::new(&staging_dir).join(&output_name);

        send_log(&tx, &name, format!("Building {}:{} with {} in {}", name, version, dockerfile.display(), context.display()));

        let result = run_podman_build(&tx, &name, &dockerfile, &context, &tag, kill_rx).await;

        match result {
            Ok(()) => {
                send_log(&tx, &name, "Build complete. Saving tarball...".to_string());

                let save_result = run_podman_save_blocking(&tx, &name, &tag, &output_path).await;
                match save_result {
                    Ok(()) => {
                        send_log(&tx, &name, format!("Tarball saved to {}", output_path.display()));
                        let _ = tx.send(BuildEvent::BuildComplete {
                            image_name: name,
                            result: Ok(()),
                            tarball_written: true,
                        });
                    }
                    Err(e) => {
                        send_log(&tx, &name, format!("Error saving tarball: {}", e));
                        let _ = tx.send(BuildEvent::BuildComplete {
                            image_name: name,
                            result: Err(format!("Save error: {}", e)),
                            tarball_written: false,
                        });
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(BuildEvent::BuildComplete {
                    image_name: name,
                    result: Err(e),
                    tarball_written: false,
                });
            }
        }
    });

    (rx, BuildHandle {
        kill_tx: Some(kill_tx),
        join: Some(join),
    })
}

fn send_log(tx: &mpsc::UnboundedSender<BuildEvent>, name: &str, line: String) {
    let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
    let _ = tx.send(BuildEvent::LogLine {
        image_name: name.to_string(),
        line,
        seq,
    });
}

/// Normalize a single line of podman output for display and clipboard use.
///
/// Podman emits:
/// - ANSI escape sequences for cursor positioning and color (we don't want
///   those in the in-TUI viewer or in copied text)
/// - `\r` to overwrite progress lines in place (we keep only the last segment)
/// - Trailing `\r` from CRLF line endings
/// - Whitespace padding to align to the terminal width
///
/// This strips all of that and returns a clean, single-line string.
fn clean_podman_line(line: &str) -> String {
    let stripped = strip_ansi_escapes(line);
    let no_trailing_cr = stripped.trim_end_matches('\r');
    let last_segment = no_trailing_cr.rsplit('\r').next().unwrap_or("");
    last_segment.trim().to_string()
}

fn strip_ansi_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                // CSI: parameter bytes (0x30-0x3F), intermediate bytes (0x20-0x2F),
                // final byte (0x40-0x7E). Skip until the final byte.
                while let Some(&nc) = chars.peek() {
                    chars.next();
                    if (0x40..=0x7e).contains(&(nc as u32)) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // OSC: skip until BEL (\x07) or ST (ESC \).
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                // 2-byte escape (Fe): skip the next char.
                chars.next();
            }
            None => {}
        }
    }
    out
}

async fn run_podman_build(
    tx: &mpsc::UnboundedSender<BuildEvent>,
    name: &str,
    dockerfile: &Path,
    context: &Path,
    tag: &str,
    mut kill_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let mut child = std::process::Command::new("podman")
        .arg("build")
        .arg("-f")
        .arg(dockerfile)
        .arg("-t")
        .arg(tag)
        .arg(".")
        .current_dir(context)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn podman build: {}", e))?;

    let stdout = child.stdout.take().ok_or("stdout not piped")?;
    let stderr = child.stderr.take().ok_or("stderr not piped")?;
    let tx_stdout = tx.clone();
    let tx_stderr = tx.clone();

    let stdout_name = name.to_string();
    let stderr_name = name.to_string();

    let stdout_task = tokio::task::spawn_blocking(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let line = clean_podman_line(&line);
            let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
            if tx_stdout.send(BuildEvent::LogLine {
                image_name: stdout_name.clone(),
                line,
                seq,
            }).is_err() {
                break;
            }
        }
    });

    let stderr_task = tokio::task::spawn_blocking(move || {
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            let line = clean_podman_line(&line);
            let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
            if tx_stderr.send(BuildEvent::LogLine {
                image_name: stderr_name.clone(),
                line,
                seq,
            }).is_err() {
                break;
            }
        }
    });

    // Either both reader streams drain (meaning child has closed its pipes and is
    // about to exit), or we receive the kill signal and need to terminate the child.
    let killed = tokio::select! {
        _ = async {
            let _ = stdout_task.await;
            let _ = stderr_task.await;
        } => false,
        _ = &mut kill_rx => true,
    };

    if killed {
        let _ = child.kill();
    }

    // Reap the child in a blocking thread so we don't stall the runtime.
    let status = tokio::task::spawn_blocking(move || child.wait())
        .await
        .map_err(|e| format!("Wait task join failed: {}", e))?
        .map_err(|e| format!("Failed to wait on podman build: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("podman build exited with status: {}", status))
    }
}

async fn run_podman_save_blocking(
    tx: &mpsc::UnboundedSender<BuildEvent>,
    name: &str,
    tag: &str,
    output_path: &Path,
) -> Result<(), String> {
    let tag = tag.to_string();
    let output_path = output_path.to_path_buf();
    let tx = tx.clone();
    let name = name.to_string();

    tokio::task::spawn_blocking(move || -> Result<(), String> {
        if let Some(parent) = output_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create staging dir {}: {}", parent.display(), e))?;
            }
        }

        let mut child = std::process::Command::new("podman")
            .arg("save")
            .arg(&tag)
            .arg("-o")
            .arg(&output_path)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn podman save: {}", e))?;

        let stderr = child.stderr.take().ok_or("stderr not piped")?;
        let stderr_tx = tx.clone();
        let stderr_name = name.clone();
        let stderr_handle = std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            let mut collected: Vec<String> = Vec::new();
            for line in reader.lines().map_while(Result::ok) {
                let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
                let _ = stderr_tx.send(BuildEvent::LogLine {
                    image_name: stderr_name.clone(),
                    line: line.clone(),
                    seq,
                });
                collected.push(line);
            }
            collected
        });

        let status = child
            .wait()
            .map_err(|e| format!("Failed to wait on podman save: {}", e))?;

        let stderr_lines = stderr_handle.join().unwrap_or_default();

        if status.success() {
            Ok(())
        } else {
            let detail = if stderr_lines.is_empty() {
                String::new()
            } else {
                format!(": {}", stderr_lines.join(" | "))
            };
            Err(format!("podman save exited with status: {}{}", status, detail))
        }
    })
    .await
    .map_err(|e| format!("podman save join error: {}", e))?
}

pub fn dump_log_to_file(
    staging_dir: &str,
    name: &str,
    version: &str,
    log: &[String],
) -> Result<std::path::PathBuf, String> {
    use std::io::Write;

    let logs_dir = std::path::Path::new(staging_dir).join("logs");
    std::fs::create_dir_all(&logs_dir)
        .map_err(|e| format!("Failed to create logs dir: {}", e))?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("Time error: {}", e))?
        .as_secs();
    let filename = format!("{}-{}-{}.log", name, version, now);
    let path = logs_dir.join(&filename);

    let mut file = std::fs::File::create(&path)
        .map_err(|e| format!("Failed to create log file: {}", e))?;
    for line in log {
        writeln!(file, "{}", line).map_err(|e| format!("Failed to write log: {}", e))?;
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tarball_filename_format() {
        let name = "adguardhome";
        let version = "v0.107.68-1.0.2";
        let tarball = format!("{}-{}.tar", name, version);
        assert_eq!(tarball, "adguardhome-v0.107.68-1.0.2.tar");
    }

    #[test]
    fn test_dump_log_to_file_creates_path() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().to_str().unwrap();
        let path = dump_log_to_file(staging, "test", "v1", &vec!["hello".into(), "world".into()]).unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("world"));
        assert_eq!(path.parent().unwrap(), temp.path().join("logs"));
    }

    #[test]
    fn test_dump_log_filename_uses_unix_timestamp() {
        let temp = tempfile::tempdir().unwrap();
        let staging = temp.path().to_str().unwrap();
        let path = dump_log_to_file(staging, "img", "v1", &[]).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(filename.starts_with("img-v1-"));
        assert!(filename.ends_with(".log"));
        let ts_part = filename.trim_start_matches("img-v1-").trim_end_matches(".log");
        assert!(ts_part.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_clean_podman_line_plain() {
        assert_eq!(clean_podman_line("STEP 1/5: FROM ubuntu"), "STEP 1/5: FROM ubuntu");
    }

    #[test]
    fn test_clean_podman_line_strips_ansi_csi() {
        // \x1b[80C = cursor forward 80 cols
        assert_eq!(clean_podman_line("\x1b[80C STEP 1/5: FROM ubuntu"), "STEP 1/5: FROM ubuntu");
    }

    #[test]
    fn test_clean_podman_line_strips_ansi_color() {
        // \x1b[31m = red, \x1b[0m = reset
        assert_eq!(clean_podman_line("\x1b[31mERROR\x1b[0m: boom"), "ERROR: boom");
    }

    #[test]
    fn test_clean_podman_line_strips_ansi_osc() {
        // OSC 0 (set title) terminated by BEL
        assert_eq!(clean_podman_line("before\x1b]0;title\x07after"), "beforeafter");
    }

    #[test]
    fn test_clean_podman_line_takes_last_cr_segment() {
        assert_eq!(clean_podman_line("old\roverwrite"), "overwrite");
    }

    #[test]
    fn test_clean_podman_line_handles_trailing_cr() {
        assert_eq!(clean_podman_line("hello\r"), "hello");
    }

    #[test]
    fn test_clean_podman_line_strips_whitespace_padding() {
        assert_eq!(
            clean_podman_line("                    Using cache abc123"),
            "Using cache abc123"
        );
    }

    #[test]
    fn test_clean_podman_line_preserves_internal_spaces() {
        assert_eq!(
            clean_podman_line("STEP 1/5: RUN apt-get install -y curl"),
            "STEP 1/5: RUN apt-get install -y curl"
        );
    }
}
