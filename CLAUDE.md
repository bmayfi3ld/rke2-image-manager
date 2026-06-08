# rke2-image-manager

TUI for managing custom container images across RKE2 cluster nodes. Builds images from Dockerfiles via Podman, distributes tarballs to RKE2 nodes via SCP, and cleans up stale versions.

## Build & Run

- `just check` — cargo check
- `just build` — cargo build
- `just run` — cargo run
- `just lint` — cargo clippy
- `just test` — cargo test

## Architecture

See `specs/primary.md` for the full specification. Each phase must pass `cargo check` before proceeding.