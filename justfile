# Default target lists available commands
default:
    @just --list

# Run cargo check to verify compilation without building
check:
    cargo check

# Build the project in debug mode
build:
    cargo build

# Build the project in release mode
release:
    cargo build --release

# Run the application
run:
    cargo run

# Run clippy linter with warnings as errors
lint:
    cargo clippy -- -D warnings

# Format source code
fmt:
    cargo fmt

# Run tests
test:
    cargo test

# Install the binary to ~/.cargo/bin
install:
    cargo install --path .