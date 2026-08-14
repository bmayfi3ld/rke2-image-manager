# Feature Plan: Non-TUI CLI

**Date:** 2026-08-04
**Scope:** Add a scriptable, non-interactive command-line interface alongside the existing TUI.

---

## 1. Goal

Everything the TUI can do (discover images, scan servers, build, deploy, remove/clean) must be
doable from a single non-interactive command suitable for scripts, CI, and `ssh host -- cmd`
usage. The TUI stays the default when invoked with no arguments so existing muscle memory and
the `just run` target keep working.

Non-goals:
- No new remote transport (still `ssh`/`scp` subprocesses).
- No daemon, no server-side agent.
- No change to the on-disk config format.

---

## 2. Current State

| File | Role | TUI-coupled? |
|---|---|---|
| `src/config.rs` | TOML load | No |
| `src/models.rs` | `Config`, `Server`, `ManagedImage`, `ImageFamily`, `UnknownTarball`, `ImageTableRow` | No |
| `src/dockerfile.rs` | `discover_image_families()` — walks `server_images_dir`, parses version comments | No |
| `src/remote.rs` | `start_remote_scan()`, `apply_scan_results()`, `refresh_local_tarballs()`, `deploy_tarball()`, `remove_tarball()`, `tarball_filename()` | No |
| `src/build.rs` | `start_build()` → `(mpsc::UnboundedReceiver<BuildEvent>, BuildHandle)`, `dump_log_to_file()` | No |
| `src/app.rs` | `App` — owns inventory + popup/log-viewer/search state | **Yes** (`ratatui::layout::Rect`, `Popup`, `LogViewer`) |
| `src/ui/*` | Widgets | Yes |
| `src/main.rs` | Terminal setup, event loop, key handling, `find_config()` | Yes |

The reusable core is already ~90% of what the CLI needs. `App` is a state machine for interactive
selection; the CLI does not need it and should **not** be refactored to go through it.

### Pre-existing defect to fix in this work

`src/main.rs:1-7` declares `mod app; mod build; mod config; mod dockerfile; mod models; mod remote;
mod ui;` while `src/lib.rs:1-7` declares the same modules as `pub mod`. The crate is therefore
compiled twice — once into the lib, once into the bin — and the two copies have distinct types.
Phase 0 replaces the `mod` block in `main.rs` with `use rke2_image_manager::...`.

---

## 3. Command Surface

```
rke2-image-manager [GLOBAL OPTIONS] [COMMAND]
```

**Dispatch rule:** no subcommand → launch the TUI (current behavior, unchanged).
`tui` is also accepted as an explicit subcommand.

### Global options

| Flag | Meaning |
|---|---|
| `-c, --config <PATH>` | Override config discovery |
| `--json` | Machine-readable output (all read commands, and result summaries for write commands) |
| `-q, --quiet` | Suppress progress/log chatter; only final results and errors |
| `--no-color` | Disable ANSI styling (also honors `NO_COLOR` env var) |

**No interactive prompts, ever.** Every command performs its action and reports the result. The
only safety valve is `--dry-run` on the destructive commands, which the caller opts into. This
means the CLI behaves identically whether or not stdin is a TTY — no branch on terminal detection,
no `--yes` flag.

### Subcommands

#### `list` — inventory
```
rke2-image-manager list [--no-scan] [--filter <SUBSTR>]
                        [--current] [--stale] [--unknown]
```
- Default: scans all servers, prints every row type (current / stale / unknown).
- `--no-scan`: skip SSH entirely, report only Dockerfile discovery + local staging tarballs. Fast.
- `--current` / `--stale` / `--unknown`: restrict row types; combinable, default is all three.
- `--filter`: plain case-insensitive substring on `name` and `name-version`. **Not** nucleo fuzzy
  matching — fuzzy is right for interactive typing, wrong for scripts.

Human output — one row per image, a column per server:

```
NAME          VERSION           LOCAL  bamserve4  bamserve5  bamserve6  STATE
adguardhome   v0.107.68-1.0.2   yes    yes        yes        no         ok
immich        2.10.2-0.4.0      no     yes        yes        yes        ok
  (stale)     2.10.1-0.3.0      -      yes        no         no         stale
unbound-x.tar -                 -      no         no         yes        unknown
```

JSON output: `{ "servers": [...], "scan": {...}, "images": [...], "stale": [...], "unknown": [...] }`
built from the existing `Serialize` impls on `models.rs` types.

#### `status` — reachability + counts
```
rke2-image-manager status
```
Per-server scan result (`ScanStatus::{Pending,Ok,Error}`) plus the aggregate counts that
`app::count_status()` renders in the TUI status bar. Exits non-zero if any server failed to scan.

#### `servers` — list configured servers
```
rke2-image-manager servers
```
Name, user@host:port, identity file. Does not connect. Trivial, but makes `--server` values
discoverable.

#### `build` — build image(s) + save tarball to staging
```
rke2-image-manager build <IMAGE>... | --all [--dump-log]
```
- `<IMAGE>` is a family name (`adguardhome`). Version comes from the Dockerfile — you cannot build
  an arbitrary version, so no `:VERSION` suffix here.
- `--all`: every discovered family.
- Streams podman output to stdout as it arrives (already cleaned by `build::clean_podman_line`).
  Under `--quiet`, suppress log lines and print only per-image pass/fail.
- `--dump-log`: also write `staging_dir/logs/<name>-<version>-<ts>.log` via `build::dump_log_to_file`.
- Builds run **sequentially**. `start_build` already streams one image's output; interleaving two
  builds on one stdout is unreadable, and podman layer cache contention makes it a poor trade.

#### `deploy` — copy tarball(s) to servers
```
rke2-image-manager deploy <IMAGE>... | --all
                          [-s, --server <NAME>]...
                          [--build] [--missing-only] [--dry-run]
```
- **Server selection defaults to every server in the config.** The `[[servers]]` list *is* the
  managed set — that is what the config file means — so fanning out is the correct default.
  `-s, --server <NAME>` (repeatable) narrows to a subset when you want one node.
- Unknown `--server` names are a hard error listing the valid names, not a silent no-op.
- `--build`: build first if the staging tarball is absent or the Dockerfile is newer.
- `--missing-only`: skip servers that already report the tarball present in the scan.
- Reuses `remote::deploy_tarball` unchanged (scp to `/tmp`, then `sudo -n mv` into place, with
  `/tmp` cleanup on mv failure).

#### `remove` — delete specific tarballs from servers
```
rke2-image-manager remove <IMAGE[:VERSION]>...
                          [-s, --server <NAME>]...
                          [--dry-run]
```
- `IMAGE` alone → the current version. `IMAGE:VERSION` → that exact version (this is how you target
  a stale one). A bare `.tar` filename targets an unknown tarball.
- Server selection defaults to all configured servers, same as `deploy`.
- No confirmation prompt — the command removes what you named. `--dry-run` prints the exact
  `ssh ... sudo -n rm <path>` targets and exits 0 without touching anything.

#### `clean` — bulk sweep of stale/unknown tarballs
```
rke2-image-manager clean [--stale] [--unknown]
                         [-s, --server <NAME>]...
                         [--dry-run]
```
- Requires at least one of `--stale` / `--unknown`. This is the one remaining guard: with no
  prompts anywhere, a bare `clean` that swept everything unrecognized would be too easy to fire by
  accident. Naming the category is cheap and makes intent explicit.
- **Servers only.** Stale tarballs in the local `staging_dir` are left alone — that is cheap disk,
  and deleting a local build artifact as a side effect of a remote cleanup would surprise.
- Prints the full target list as it acts (or, with `--dry-run`, instead of acting).

### Exit codes

| Code | Meaning |
|---|---|
| 0 | All requested work succeeded |
| 1 | One or more per-target operations failed (partial failure); details on stderr |
| 2 | Usage error (clap handles most of these) |
| 3 | Config not found / unparsable |

Partial failures never abort the run — every remaining target is still attempted, and the summary
at the end lists what failed. This matters because a single unreachable node should not block a
three-node deploy.

---

## 4. Implementation Phases

Each phase must pass `just check` and `just lint` before the next, per `CLAUDE.md`.

### Phase 0 — Dependencies + module hygiene

- `Cargo.toml`: add
  - `clap = { version = "4", features = ["derive"] }`
  - `serde_json = "1"`
- TTY detection uses `std::io::IsTerminal` (stable since 1.70) — no extra dependency.
- `src/main.rs`: delete the `mod ...;` block, import from `rke2_image_manager::` instead. Verify the
  binary still builds and the TUI still runs.
- Move `find_config()` / `dirs_lookup()` out of `main.rs` into `config.rs` as
  `pub fn find_config(override_path: Option<&Path>) -> Result<PathBuf>`, with search order:
  1. `--config` argument
  2. `$RKE2_IMAGE_MANAGER_CONFIG`
  3. `./config.toml`
  4. `$XDG_CONFIG_HOME/rke2-image-manager/config.toml` (fallback `~/.config/...`)
  5. alongside the executable (current `dirs_lookup` behavior)

**Verify:** `just check`, `just build`, `just test`, and `just run` still opens the TUI.

### Phase 1 — Shared inventory assembly

New `src/inventory.rs`:

```rust
pub struct Inventory {
    pub families: BTreeMap<String, ImageFamily>,
    pub unknowns: Vec<UnknownTarball>,
    pub server_names: Vec<String>,
    pub scan_status: BTreeMap<String, ScanStatus>,
    pub warnings: Vec<String>,
}

/// Discover Dockerfiles, refresh local staging tarballs, and (unless `scan` is
/// false) scan every configured server to completion.
pub async fn load(config: &Config, scan: bool) -> Result<Inventory>;

pub fn rows(inv: &Inventory) -> Vec<ImageTableRow>;
```

`load()` composes the existing primitives — `dockerfile::discover_image_families`,
`remote::refresh_local_tarballs`, `remote::start_remote_scan`, `remote::apply_scan_results` — and
drains the scan channel to completion instead of polling it. `rows()` is `app::build_rows` lifted
out of `app.rs`; `app.rs` then calls `inventory::rows`.

**Deliberate decision:** `App` keeps its own incremental/streaming scan path. It needs progressive
redraws as each server reports; the CLI wants a single blocking answer. The shared code is the
per-event logic, which both already call. Only the ~15-line assembly order is duplicated.

**Verify:** `cargo test`; add `tests/inventory_tests.rs` covering `load(config, scan: false)`
against a `tempfile` fixture tree of Dockerfiles + staging tarballs, and `rows()` ordering
(current → stale → unknown), mirroring the existing `dockerfile_tests.rs` style.

### Phase 2 — CLI skeleton + read-only commands

New `src/cli/` module:

| File | Contents |
|---|---|
| `mod.rs` | `#[derive(Parser)] struct Cli`, `enum Command`, `pub async fn run(cli: Cli) -> Result<ExitCode>` |
| `output.rs` | `Renderer` — column-aligned table writer + `--json` emitter + `--no-color`/`NO_COLOR` handling |
| `select.rs` | `ImageSelector::parse("name" \| "name:version" \| "file.tar")`, resolution against an `Inventory`, and the "unknown image name" error |

`main.rs` becomes thin:

```rust
fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        None | Some(Command::Tui) => tui::run(cli.config),   // existing code, moved
        Some(cmd)                 => cli::run(cli, cmd),
    }
}
```

The existing TUI bootstrap (raw mode, alternate screen, `TerminalGuard`, panic hook, event loop,
`handle_key`, `draw`) moves verbatim into `src/tui.rs`. **The terminal guard and panic hook must
not run on the CLI path** — a CLI that leaves the terminal in raw mode on error is worse than no
CLI.

Implement `list`, `status`, `servers` in this phase.

**Verify:** `just lint`; manual `list --no-scan`, `list --json | jq .`, `servers`; `status` against
the real cluster; `list` piped to a file has no ANSI escapes.

### Phase 3 — `build`

`src/cli/commands/build.rs`. Resolves selectors → families, then per image drives
`build::start_build` and drains `BuildEvent`s to stdout until `BuildComplete`. Honors `--quiet` and
`--dump-log`. Accumulates per-image results for the final summary and exit code.

**Verify:** build one real image; confirm the tarball lands in `staging_dir`; confirm a deliberately
broken Dockerfile exits 1 with the podman error on stderr.

### Phase 4 — `deploy`

`src/cli/commands/deploy.rs`. Resolves images + servers, checks the staging tarball exists (build
first if `--build`), then calls `remote::deploy_tarball` per (image, server). Sequential per server
with a one-line-per-target progress report; `--missing-only` filters against scan results.

**Verify:** deploy to one node, re-run `list` and confirm presence flips to `yes`; deploy with one
node powered off and confirm exit code 1 with the other nodes still succeeding.

### Phase 5 — `remove` and `clean`

`src/cli/commands/remove.rs`, `src/cli/commands/clean.rs`. Both build a target list of
`(server, tarball_filename)`, print it, then call `remote::remove_tarball` per target.
`--dry-run` stops after printing. No confirmation step.

**Verify:** `--dry-run` on real stale versions first and confirm nothing was touched; then a real
`remove` of a stale version and `list` confirming it is gone; `clean` with no category flag exits 2.

### Phase 6 — Docs, completions, tests

- `justfile`: add `completions` target generating bash/fish/zsh via `clap_complete`
  (`clap_complete = "4"` — dev-time only usage but a normal dependency since generation happens in
  a `completions` subcommand or build script; prefer a hidden `__completions <SHELL>` subcommand to
  avoid a build script).
- `README.md`: a "CLI" section with the command table and worked examples.
- `CLAUDE.md`: note that the binary has both a TUI and a CLI mode and where each lives.
- `specs/primary.md`: append a CLI section so the spec stays the source of truth.
- `tests/cli_tests.rs`: selector parsing, server-selection resolution, table rendering, exit-code
  mapping. No network, no podman — those paths stay manually verified, consistent with the existing
  test suite.

---

## 5. Files Touched

**New**
```
src/inventory.rs
src/tui.rs
src/cli/mod.rs
src/cli/output.rs
src/cli/select.rs
src/cli/commands/{mod,list,status,servers,build,deploy,remove,clean}.rs
tests/inventory_tests.rs
tests/cli_tests.rs
```

**Modified**
```
Cargo.toml        clap, serde_json, clap_complete
src/main.rs       reduced to dispatch; mod-duplication fixed
src/lib.rs        + pub mod cli; pub mod inventory; pub mod tui;
src/config.rs     find_config() moved in, search order extended
src/app.rs        build_rows() moved to inventory::rows()
justfile          completions target
README.md         CLI docs
specs/primary.md  CLI spec section
```

**Untouched:** `src/build.rs`, `src/remote.rs`, `src/dockerfile.rs`, `src/models.rs`, `src/ui/*`.
That the four core modules need no changes is the main evidence this split is clean.

---

## 6. Risks and Decisions

| Item | Decision | Why |
|---|---|---|
| Arg parser | `clap` derive | Standard, gives `--help`/completions/usage errors free. The hand-rolled alternative is not worth it at this command count. |
| Default when no subcommand | TUI | Backwards compatible; `just run` and existing habits unaffected. |
| Fuzzy search in `list` | No — plain substring | Nucleo fuzzy matching is a typing affordance. In a script, `--filter immich` matching `i-m-m-i-c-h` scattered across an unrelated name is a bug. |
| Server default for writes | All configured servers | The `[[servers]]` list is the managed set by definition; `-s/--server` narrows when needed. |
| Interactive prompts | None, anywhere | The command performs the action. Behavior is identical on and off a TTY, there is no terminal-detection branch to test, and no `--yes` flag. `--dry-run` is the opt-in safety valve. |
| `clean` category flag | Still required | With no prompts anywhere, requiring `--stale`/`--unknown` is the only thing standing between a typo and a full sweep. Cheap to type, makes intent explicit. |
| Partial failure | Continue, report, exit 1 | One unreachable node should not abort a multi-node operation. |
| Build parallelism | Sequential | Interleaved podman output is unreadable; layer-cache contention hurts. `--jobs` is a possible follow-up. |
| `App` refactor | Not doing it | `App` is interactive-selection state. Forcing the CLI through it would couple the CLI to ratatui for no gain. |
| Double module compilation | Fixed in Phase 0 | Pre-existing; adding a third entry point makes it actively harmful. |

## 7. Settled

- **No TTY prompts.** Confirmed: commands act. `--dry-run` only.
- **Substring filter, not fuzzy.** Confirmed.
- **Writes fan out to all configured servers by default.** Confirmed — the config file is the
  management boundary.
- **`clean` is servers-only.** Confirmed; local `staging_dir` is never touched.
- **Fix the double module compilation** in Phase 0. Confirmed.

## 8. Deferred

1. **Version-pinned deploy** — `deploy` only handles the current version, since that is the only
   one with a local tarball. Deploying an archived tarball would need a staging-dir lookup by
   filename. Not building it until asked.
2. **`--jobs N`** for parallel deploy across servers — sequential first; measure before optimizing.
3. **Local staging cleanup** — if `staging_dir` growth ever becomes a problem, a separate
   `prune --local` command is the right shape, not a flag bolted onto `clean`.
