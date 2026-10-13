---
name: stack
description: Languages, frameworks, and runtime tools used in the project.
edges:
  - target: architecture.md
    condition: when checking architecture or subsystem boundaries
  - target: setup.md
    condition: when confirming build, test, or local run steps
  - target: decisions.md
    condition: when tracing why the current runtime choices matter
last_updated: 2026-10-12
---

# Stack

## Language and workspace
- Rust stable toolchain, pinned via `rust-toolchain.toml`.
- Cargo workspace with `crates/ce-cli`, `ce-core`, `ce-http`, `ce-protocol`, and `ce-transport`.
- Edition 2021; workspace version is currently `0.1.0`.

## Key libraries
- `axum` for the HTTP surface in `ce-http`.
- `tokio` for async runtime support and request handling.
- `rusqlite` with bundled SQLite for the sidecar database.
- `serde` / `serde_json` for protocol payloads and invitations.
- `clap` for the CLI commands and flags.
- `tracing` and `tracing-subscriber` for structured logging.
- `sha2`, `base64`, and `hex` for IDs, encoding, and signatures.

## Build and validation tools
- `Taskfile.yml` defines `task setup`, `task check`, `task test`, `task e2e`, `task release`, and `task parity`.
- The project uses `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test --workspace` as the standard validation path.
- GitHub Actions handles CI and release automation; docs deploy via Jekyll from `docs/`.

## External runtime boundary
- Optional `tailcat` transport is supported conditionally, but it is not part of the required runtime path.
- The supported development environments are macOS and Linux; no Windows-specific runtime is required for the main checks.
