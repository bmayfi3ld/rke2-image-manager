# rke2-image-manager

TUI for managing custom container images across RKE2 cluster nodes. Builds images from
Dockerfiles via Podman, distributes tarballs to RKE2 nodes via SCP, and cleans up stale
versions -- replacing the `ansible/build_and_distribute_images.yaml` playbook.

## Repository

- **Remote**: `git@github.com:bmayfi3ld/rke2-image-manager.git`
- **Local path**: `~/Source/rke2-image-manager`

## Project Structure

```
rke2-image-manager/
├── README.md
├── AGENTS.md              → symlink to CLAUDE.md
├── CLAUDE.md
├── config.example.toml
├── justfile
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── app.rs
│   ├── config.rs
│   ├── dockerfile.rs
│   ├── build.rs
│   ├── remote.rs
│   ├── models.rs
│   └── ui/
│       ├── mod.rs
│       ├── image_table.rs
│       ├── action_popup.rs
│       ├── log_viewer.rs
│       ├── server_picker.rs
│       └── status_bar.rs
└── .gitignore
```

## Data Model

### Core Types

```rust
enum BuildState {
    Idle,
    Building,
    Failed(String),
    Success,
}

struct ManagedImage {
    name: String,                             // "adguardhome"
    version: String,                          // "v0.107.68-1.0.2"
    dockerfile_path: PathBuf,                 // "adguardhome.dockerfile"
    context_dir: PathBuf,                     // "." or "immich_transfer"
    local_tarball: bool,                      // exists in staging_dir?
    server_presence: BTreeMap<String, bool>,  // server_name -> exists?
    build_state: BuildState,
    build_log: Vec<String>,                   // captured stdout/stderr lines
}

struct ImageFamily {
    name: String,                             // "adguardhome"
    current_version: Option<String>,          // "v0.107.68-1.0.2"
    current: Option<ManagedImage>,            // build state, log, local tarball
    stale_versions: BTreeSet<String>,         // e.g. {"v0.107.50-1.0.1"}
    // per-server tarball presence: version -> set of servers
    remote_presence: BTreeMap<String, BTreeSet<String>>,
}

struct UnknownTarball {
    filename: String,                         // raw remote filename, e.g. "strange-backup.tar"
    servers: BTreeSet<String>,                // which servers have it
}

enum ImageTableRow {
    Current(ManagedImage),
    Stale { family_name: String, version: String, servers: BTreeSet<String> },
    Unknown(UnknownTarball),
}
```

### Startup Flow

1. Parse `config.toml` -- server list, paths, registry
2. Scan `server_images/` recursively for `*.dockerfile` files (and `Dockerfile` in subdirs where parent dir name becomes image name)
3. Parse version from first `#` comment line in each Dockerfile
4. Concurrently `ssh <host> ls <rke2_images_dir>` on each server to collect remote `.tar` filenames
5. For each remote tarball `{name}-{version}.tar`:
   - If `name` matches an ImageFamily and `version` == `current_version` → mark present
   - If `name` matches an ImageFamily and `version` != `current_version` → add to `stale_versions`
   - If `name` does not match any family → create UnknownTarball entry
6. Check local staging dir for matching tarballs → set `local_tarball` on ManagedImage

### Tarball Name Parsing

Tarballs follow the convention `{name}-{version}.tar` (from `podman save`). Version is extracted from the Dockerfile's first comment line:

- `# v0.107.68-1.0.2` → version `v0.107.68-1.0.2`
- `# 2.10.2-0.4.0` → version `2.10.2-0.4.0`
- `# 1.1.1` → version `1.1.1`

For remote tarballs, strip `.tar` suffix, then split on first `-` that separates name from version.
Edge case: version strings containing `-` (like `v0.107.68-1.0.2`) require the split to occur at the right boundary. Strategy: match against known family names first; if the leading segment matches a known family name, the remainder is the version.

## UI Layout

```
┌──────────────────────────────────────────────────────────────┐
│ RKE2 Image Manager                            [3 servers]    │
├──────────────────────────────────────────────────────────────┤
│ Search: █                                                    │
│───────────                                                   │
│ Image                         Local   b4   b5   b6            │
│                                                              │
│ adguardhome     v0.107.68-1.0.2   ✗    ✓    ✓    ✓          │
│   v0.107.50-1.0.1                  —    ✓    ✓    ✗          │
│ caddy           2.10.2-0.4.0      ✓    ✓    ✓    ✓           │
│ jellyfin        v10.11.7-1.1.3    ✗    ✓    ✓    ✓           │
│ kavita          v0.8.8.3-1.0.1    ✓    ✓    ✓    ✓           │
│ lubelogger      v1.5.4-1.0.0      ✓    ✓    ✓    ✓           │
│ radarr          v5.28.0.10274-1.0.3 ✓   ✓    ✓    ✓          │
│   v5.20.0.9280-1.0.2               —    ✓    ✓    ✗          │
│ sabnzbd         v4.5.5-1.0.3      ✗    ✓    ✓    ✓           │
│ sonarr          v4.0.15.2941-1.0.0 ✓    ✓    ✓    ✓          │
│                                                              │
│ ? portal-knights-server.tar        —    ✓    ✗    ✗          │
│ ? strange-backup.tar               —    ✗    ✗    ✓          │
├──────────────────────────────────────────────────────────────┤
│ 10 managed (2 stale) | 2 unknown | 1 building...             │
│ [Enter] Actions  [Esc/q/Ctrl-C] Quit  [/] Search                    │
└──────────────────────────────────────────────────────────────┘
```

### Status Indicators

| Symbol | Meaning |
|--------|---------|
| `✓`    | Present on host |
| `✗`    | Not present on host (scan completed) |
| `⏳`   | Operation in progress (checking, building, deploying) |
| `?`    | Scan pending or failed (server unreachable) |
| `—`    | Not applicable (stale/unknown rows -- no local Dockerfile to build) |

### Styling

- Current version rows: normal/bold text
- Stale version rows: indented 2 spaces, dimmed color
- Unknown tarball rows: prefixed with `?`, dimmed color
- Section headers not used (flat list with visual grouping via indent + color is sufficient)

## Keyboard Navigation

| Key | Context | Action |
|-----|---------|--------|
| `Ctrl-C` | Any | Quit (force exit) |
| `↑` / `↓` / `j` / `k` | Table | Navigate rows |
| `Enter` | Table | Open action popup for selected row |
| `/` | Table | Focus search bar |
| `Esc` | Search bar | Clear search, return focus to table |
| `Esc` | Popup / Log viewer | Close popup / viewer |
| `q` | Table (no popup) | Quit |
| `d` | Log viewer | Dump log to file |

`Ctrl-C` force-quits from any state (popups, search, log viewer, etc.) and supersedes
the context-specific bindings above. `q` and `Esc` are honored only at the base table.

No global hotkeys for build/deploy/remove. All actions accessed through the popup menu.

## Fuzzy Search

Uses the `nucleo` crate. The search bar sits above the table. Pressing `/` focuses it; typing filters the table rows in real-time. Matching is against image name and version. `Esc` clears the filter and restores full list. `Enter` navigates focus to the first match.

## Action Popups

### Current Version Row

```
┌─ adguardhome v0.107.68-1.0.2 ────────┐
│ Build                                 │
│ Deploy to servers...                  │
│ Remove from servers...                │
│ View build logs                       │
│ Dump logs to file                     │
│ Cancel                                │
└───────────────────────────────────────┘
```

- **Build** -- always enabled. Spawns `podman build` in the context dir, then `podman save` to create tarball in staging. Disabled (greyed) with `[Building...]` label if already in progress.
- **Deploy to servers...** -- enabled only when local tarball exists. Opens server picker. SCPs tarball to selected servers' RKE2 dir.
- **Remove from servers...** -- enabled only when present on ≥1 server. Opens server picker. SSH `rm` the tarball.
- **View build logs** -- enabled when build log exists (success or failure). Opens scrollable log viewer.
- **Dump logs to file** -- writes `{staging_dir}/logs/{name}-{version}-{timestamp}.log`. Enabled when build log exists.

### Stale Version Row

```
┌─ adguardhome v0.107.50-1.0.1 (stale) ┐
│ Remove from servers...                │
│ Cancel                                │
└───────────────────────────────────────┘
```

Only cleanup. No build/deploy capabilities.

### Unknown Tarball Row

```
┌─ portal-knights-server.tar (unknown) ┐
│ Remove from servers...               │
│ Cancel                               │
└──────────────────────────────────────┘
```

Only cleanup. No build/deploy capabilities.

## Server Picker

Opened from "Deploy to servers..." or "Remove from servers..." actions.

```
┌─ Deploy caddy to... ────────────────┐
│ [✓] bamserve4   (already present)   │
│ [✓] bamserve5   (already present)   │
│ [ ] bamserve6   192.168.2.12        │
│                                      │
│ [Continue]  [Cancel]                 │
└─────────────────────────────────────┘
```

- Checkboxes toggle with `Space` or `Enter`
- Servers where the tarball already exists show "(already present)" (deploy) or are pre-checked (remove)
- `Continue` executes the operation on selected servers
- Operations run concurrently across selected servers; per-server results reported on completion

## Build Log Viewer

Full-screen scrollable buffer showing `podman build` output in real-time.

```
┌─ Build Log: sabnzbd v4.5.5-1.0.3 ───────────────────────┐
│ STEP 1/10: FROM docker.io/python:3.11-slim                │
│ STEP 2/10: RUN apt-get update && apt-get install -y ...   │
│ ...                                                       │
│ STEP 10/10: ENTRYPOINT ["/entrypoint.sh"]                 │
│ COMMIT registry.local/sabnzbd:v4.5.5-1.0.3               │
│ ✓ Build complete                                          │
│                                                           │
│ Drag to select  [Ctrl+C] Copy  [s] Scrollback  [Esc] Close  [d] Dump  [↑/↓/PgUp/PgDn] Scroll
└──────────────────────────────────────────────────────────┘
```

- `PgUp`/`PgDn`/`↑`/`↓`/`Home`/`End` for navigation
- **Click and drag** to select a range of text; the highlighted cells are
  reversed. The selection works across multiple lines.
- **Ctrl+C** copies the current selection to the terminal clipboard via the
  OSC 52 escape sequence. With no selection active, it copies the entire log.
  Supported by kitty, WezTerm, iTerm2, recent gnome-terminal, and most other
  modern terminal emulators. The status bar reports `"Copied selection to
  clipboard (N bytes)"` on success.
- `s` enters scrollback mode: the TUI leaves the alternate screen, prints the
  log to the real terminal buffer, and waits for a keypress. The user can
  then use the terminal emulator's normal text selection (mouse drag,
  shift+arrow, or terminal copy shortcut) to copy the log out — useful as a
  fallback when OSC 52 isn't available. Any keypress re-enters the TUI.
- `d` dumps the full log to `{staging_dir}/logs/{name}-{version}-{ts}.log` for
  longer-term storage or sharing.
- Error lines are highlighted in red
- Podman's raw output is cleaned at capture time: ANSI escape sequences
  (cursor positioning, colors), `\r` progress overwrites, and trailing
  whitespace padding are stripped, so both the on-screen viewer and copied
  text are readable.
- Build status shown inline at end of log

## Build Engine

### Build Flow

1. User selects "Build" on a managed image
2. Set `BuildState::Building`; create `mpsc::unbounded_channel` for log lines
3. Spawn `tokio::task::spawn` that runs:
   ```
   cd {context_dir}
   podman build -f {dockerfile} -t {image_registry}/{name}:{version} .
   ```
4. Capture stdout and stderr line-by-line; send each line through the channel (with a monotonic `seq: u64` so the UI can sort interleaved stdout/stderr correctly)
5. UI thread receives lines, appends to `build_log`, and refreshes the log viewer if open
6. On success: run `podman save {image_registry}/{name}:{version} -o {staging_dir}/{name}-{version}.tar` (wrapped in `spawn_blocking` to avoid stalling the runtime)
7. Set `BuildState::Success` or `BuildState::Failed(error)`; update `local_tarball`

### Build Cancellation

A `BuildHandle` is held by `App` for the active build. It carries a `oneshot::Sender<()>` kill signal and the `JoinHandle` of the build task. On `App::quit` (or when a new build starts for the same image), the handle is dropped/cancelled, which:
- Sends the kill signal, causing the build task to `child.kill()` and return early
- Aborts the task via `JoinHandle::abort`

This ensures no orphan `podman build` processes survive a quit or are left running when a new build replaces the old one.

### Concurrent Builds

Multiple images can build simultaneously. Each build runs in its own `tokio::task`. Build state is tracked per-image; the "Build" menu option is greyed out with `[Building...]` while a build is active for that image. Starting a second build for an image that is already building is refused by `App::start_build_for_row`.

## SSH / SCP Strategy

Uses system `ssh` and `scp` binaries directly via `std::process::Command`. This inherits the user's SSH agent, `~/.ssh/config`, and `known_hosts` -- no key management needed.

| Operation | Command |
|-----------|---------|
| List remote tarballs | `ssh {host} -- sudo -n ls --color=never -1 {rke2_images_dir}/` |
| Deploy tarball | `scp {local_tarball} {host}:{rke2_images_dir}/` |
| Remove tarball | `ssh {host} -- sudo -n rm {rke2_images_dir}/{tarball}` |

The rke2 images directory is root-owned, so `sudo` is required. The `-n` (non-interactive) flag makes sudo fail fast with NOPASSWD-not-set rather than hang waiting for a password. All SSH invocations also pass `-o BatchMode=yes` to fail fast when key auth isn't available (no interactive password prompt).

Per-server `port` and `identity_file` from config map to `-p` / `-P` and `-i` flags.

### Timeouts and Errors

- SSH connections timeout after 15 seconds (configurable via `ssh -o ConnectTimeout=15`)
- Failed SSH doesn't block startup -- server shows `?` status and error is logged
- Failed SCP during deploy shows per-server pass/fail; successful deploys to other servers proceed
- All errors produce user-visible messages in the status bar

## Threading Model

| Component | Runtime | Communication |
|-----------|---------|---------------|
| TUI event loop | Main thread (crossterm, blocking I/O) | -- |
| Tokio runtime | **Multi-threaded** (`new_multi_thread`) | -- |
| Remote scans (startup) | `tokio::task::spawn` per server | `mpsc::unbounded_channel` → TUI |
| Podman builds | `tokio::task::spawn` per image | `mpsc::unbounded_channel` for log lines |
| SCP / SSH ops | `tokio::task::spawn_blocking` | `mpsc::unbounded_channel` → TUI |

The Tokio runtime **must** be multi-threaded. The TUI event loop blocks the main thread on `terminal.draw` and `event::poll`, so a `current_thread` runtime starves all spawned background tasks. All remote events (scan results, deploy results, remove results) flow through channels that the TUI drains on every loop iteration via `try_recv`.

## Configuration

### `config.example.toml` (committed)

```toml
# rke2-image-manager configuration
# Copy to config.toml and edit for your environment

[paths]
server_images_dir = "../k8s-config/server_images"
staging_dir = "/tmp/k8s-images"
rke2_images_dir = "/var/lib/rancher/rke2/agent/images"
image_registry = "registry.local"

[[servers]]
name = "bamserve4"
host = "192.168.2.6"
user = "mayfiba"

[[servers]]
name = "bamserve5"
host = "192.168.2.11"
user = "mayfiba"

[[servers]]
name = "bamserve6"
host = "192.168.2.12"
user = "mayfiba"
```

Optional per-server fields: `port` (default 22), `identity_file` (passed as `-i`).

### `.gitignore`

```
target/
config.toml
*.log
```

## Dockerfile Discovery

Recursively scan `server_images/` for files ending in `.dockerfile`. Additionally, for subdirectories containing a file named `Dockerfile` (without `.dockerfile` extension), treat the parent directory name as the image name and use the subdirectory as the build context.

Version is extracted from the first line matching `^#\s*(v?[\d][^\s]*)`.

### Dockerfile Manifest (from k8s-config repo)

| Image Name | File | Version | Context |
|-----------|------|---------|---------|
| adguardhome | `adguardhome.dockerfile` | `v0.107.68-1.0.2` | `.` |
| caddy | `caddy.dockerfile` | `2.10.2-0.4.0` | `.` |
| jellyfin | `jellyfin.dockerfile` | `v10.11.7-1.1.3` | `.` |
| kavita | `kavita.dockerfile` | `v0.8.8.3-1.0.1` | `.` |
| lubelogger | `lubelogger.dockerfile` | `v1.5.4-1.0.0` | `.` |
| radarr | `radarr.dockerfile` | `v5.28.0.10274-1.0.3` | `.` |
| sabnzbd | `sabnzbd.dockerfile` | `v4.5.5-1.0.3` | `.` |
| sonarr | `sonarr.dockerfile` | `v4.0.15.2941-1.0.0` | `.` |
| immich_transfer | `immich_transfer/immich_transfer.dockerfile` | `1.1.1` | `immich_transfer/` |
| portal_knights_dedicated_server | `portal_knights_dedicated_server/Dockerfile` | `v1.0.0-1.0.0` | `portal_knights_dedicated_server/` |

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `ratatui` | 0.29 | TUI framework |
| `crossterm` | 0.28 | Terminal raw mode, input events |
| `serde` (with `derive` feature) | 1.x | Config parsing |
| `toml` | 0.8 | Config parsing |
| `tokio` | 1.43 (features: full) | Async runtime |
| `nucleo` | 0.5 | Fuzzy matching for search |
| `anyhow` | 1.0 | Error handling |

Build log dumps use `std::time::SystemTime` for timestamps (no `chrono` needed). `serde_derive` is not a separate dep -- the `derive` feature on `serde` re-exports the proc-macros.

## Implementation Sequence

Each phase must pass `cargo check` before proceeding to the next.

| # | Phase | Modules | Milestone |
|---|-------|---------|-----------|
| P1 | Scaffold | `Cargo.toml`, `main.rs`, `config.rs`, `models.rs`, `README.md`, `AGENTS.md`, `CLAUDE.md`, `justfile`, `.gitignore`, `config.example.toml` | Project compiles, config loads |
| P2 | Dockerfile scan | `dockerfile.rs` | Parses all Dockerfiles, extracts versions, builds ImageFamily map |
| P3 | Remote scan | `remote.rs` | Concurrent `ssh ls` on each server, parses tarballs, builds full image matrix with stale + unknown entries |
| P4 | UI table | `ui/mod.rs`, `ui/image_table.rs`, `ui/status_bar.rs`, `app.rs` | Ratatui event loop with scrollable table showing all rows |
| P5 | Fuzzy search | Search bar widget, nucleo integration | Type to filter table in real-time |
| P6 | Action popup + server picker | `ui/action_popup.rs`, `ui/server_picker.rs` | Enter opens context-sensitive menu, server picker with checkboxes |
| P7 | Build engine | `build.rs` | Podman build + save with real-time log capture via channels |
| P8 | Deploy / remove | Extended `remote.rs` | SCP deploy and SSH remove, per-server results |
| P9 | Log viewer | `ui/log_viewer.rs` | Scrollable log viewer, dump to file |
| P10 | Polish | Error states, progress indicators, color theming, dimmed stale rows, unknown indicators | `cargo run --release` -- production-ready |

## Edge Cases

1. **SSH timeout/failure**: Show `?` in status column, error detail in status bar. Do not block startup.
2. **Missing version comment**: Treat as unversioned -- image tagged as `<name>:latest` with a warning surfaced in the status bar (not `eprintln`, which would corrupt the TUI). Do not error out.
3. **`Dockerfile` naming (no `.dockerfile` extension)**: Use parent directory name as image name.
4. **Duplicate image names across Dockerfiles**: Distinct directory paths prevent collision, but log a warning.
5. **Stale tarball after build**: `podman save` overwrites existing tarball with same name -- correct behavior.
6. **Partial deploy**: If SCP fails on one server, continue to remaining servers. Show per-server pass/fail.
7. **Build failure**: Set `BuildState::Failed`, preserve log, allow retry.
8. **Ambiguous tarball name parsing**: When a remote tarball's name portion matches multiple families, match against known family names by longest-prefix match.
9. **Concurrent builds of same image**: Prevented by `BuildState::Building` check -- "Build" is greyed out.
10. **Stale version fully removed**: When the last server holding a stale version has it removed (via the picker or a future scan), drop the version from `stale_versions` and `remote_presence` so the stale row disappears. Don't leave ghost rows with empty server sets.
11. **Status after multi-server operation**: Per-server progress messages ("Removed from bamserve4", etc.) stream during the operation. When the last expected result arrives, the status is replaced with the running counts line (`"X managed (Y stale) | Z unknown | 0 building..."`).
12. **Podman child leak on quit/new build**: A `BuildHandle` is held by `App`; on `Drop` it sends a kill signal and aborts the task. No `podman build` process outlives the app.

## Design Decisions

- **No embedded SSH library**: System `ssh`/`scp` inherits user's agent, config, and keys. Simpler, more reliable.
- **In-memory logs**: Build logs exist only for the session. "Dump logs to file" for persistence.
- **No global hotkeys for actions**: Everything through the popup menu. Reduces accidental operations.
- **One server list in config**: Config-driven rather than parsing `ansible/hosts`. Single source of truth for the TUI.
- **Concurrent builds allowed**: `podman build` is I/O-bound and per-image. No resource contention on build cache.
- **`nucleo` for fuzzy search**: Fast, well-maintained, used by Helix editor. Matches against both name and version.