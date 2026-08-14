# rke2-image-manager

TUI (and scriptable CLI) for managing custom container images across RKE2 cluster nodes. Builds
images from Dockerfiles via Podman, distributes tarballs to RKE2 nodes via SCP, and cleans up
stale versions.

## Usage

```
rke2-image-manager             # interactive TUI (default)
rke2-image-manager tui         # same, explicit
rke2-image-manager <command>   # non-interactive, scriptable
```

## CLI

Every command performs its action and reports the result -- there are no interactive prompts.
The only safety valve on destructive commands is `--dry-run`.

### Global options

| Flag | Meaning |
|---|---|
| `-c, --config <PATH>` | Override config discovery |
| `--json` | Machine-readable output |
| `-q, --quiet` | Suppress progress/log chatter; only final results and errors |
| `--no-color` | Disable ANSI styling (also honors `NO_COLOR`) |

### Commands

| Command | Purpose |
|---|---|
| `list [--no-scan] [--filter S] [--current] [--stale] [--unknown]` | Inventory across all servers |
| `status` | Per-server reachability + aggregate counts |
| `servers` | List configured servers (no connection) |
| `build <IMAGE>... \| --all [--dump-log]` | Build image(s), save tarball to staging |
| `deploy <IMAGE>... \| --all [-s NAME]... [--build] [--missing-only] [--dry-run]` | Copy tarball(s) to servers |
| `remove <IMAGE[:VERSION]>... [-s NAME]... [--dry-run]` | Delete specific tarballs from servers |
| `clean [--stale] [--unknown] [-s NAME]... [--dry-run]` | Bulk sweep of stale/unknown tarballs |

`IMAGE` selectors:
- `name` -- the current version (version comes from the Dockerfile)
- `name:version` -- a specific version, e.g. a stale one (`remove`/`clean` only)
- `file.tar` -- an unrecognized tarball by filename (`remove`/`clean` only)

Server selection (`deploy`, `remove`, `clean`) defaults to every server in the config; `-s/--server`
(repeatable) narrows to a subset. Writes fan out to all configured servers because the `[[servers]]`
list *is* the managed set.

### Examples

```sh
# See everything without touching the network
rke2-image-manager list --no-scan

# Full inventory as JSON, for scripts
rke2-image-manager list --json | jq '.stale'

# Build and deploy one image everywhere
rke2-image-manager deploy adguardhome --build

# Deploy only to servers missing the current version
rke2-image-manager deploy --all --missing-only

# Preview a cleanup before running it
rke2-image-manager clean --stale --unknown --dry-run
rke2-image-manager clean --stale --unknown

# Remove one stale version from one server
rke2-image-manager remove caddy:2.11.4-0.4.0 -s bamserve4
```

### Exit codes

| Code | Meaning |
|---|---|
| 0 | All requested work succeeded |
| 1 | One or more per-target operations failed (partial failure); details on stderr |
| 2 | Usage error |
| 3 | Config not found / unparsable |

### Shell completions

```sh
just completions   # writes bash/zsh/fish scripts to ./completions
```
