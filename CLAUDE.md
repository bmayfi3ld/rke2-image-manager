# rke2-image-manager

TUI (and scriptable CLI) for managing custom container images across RKE2 cluster nodes. Builds images from Dockerfiles via Podman, distributes tarballs to RKE2 nodes via SCP, and cleans up stale versions.

The binary has two modes: run with no arguments (or `tui`) for the interactive TUI (`src/tui.rs`),
or with a subcommand (`list`, `status`, `servers`, `build`, `deploy`, `remove`, `clean`) for the
non-interactive CLI (`src/cli/`). Both share the same discovery/scan/build/remote core
(`src/inventory.rs`, `src/dockerfile.rs`, `src/build.rs`, `src/remote.rs`) -- see README.md for the
CLI command reference.

## Build & Run

- `just check` — cargo check
- `just build` — cargo build
- `just run` — cargo run (TUI)
- `just lint` — cargo clippy
- `just test` — cargo test
- `just completions` — generate shell completions into ./completions

## Architecture

See `specs/primary.md` for the full specification. Each phase must pass `cargo check` before proceeding.