---
name: setup
description: Local setup, validation commands, and commonly used runtime details.
edges:
  - target: stack.md
    condition: when checking the language and dependency stack
  - target: architecture.md
    condition: when confirming what runs where in the local room lifecycle
  - target: patterns/INDEX.md
    condition: before troubleshooting a project-specific setup issue
last_updated: 2026-10-13
---

# Setup

## Install
```sh
cargo fetch --locked
```

## Build and validate
```sh
task setup
task build
task test
task check
task e2e
task release
task supply-chain
task image-verify
```

## Common commands
- `cargo run --bin ce -- --help`
- `cargo run --bin ce -- serve notes.md --name Ada`
- `cargo test --workspace --locked`
- `cargo clippy --all-targets --locked -- -D warnings`

## Environment and runtime notes
- The core protocol identifiers are intentionally fixed and should not be renamed casually.
- `COMMONROOM_WORKSPACE`, `COMMONROOM_TAILCAT`, and `COMMONROOM_SKILL` are recognized fallback environment variables for workspace and transport selection.
- The project targets macOS and Linux for development; the runtime is designed around loopback-first behavior.
- `task supply-chain` installs pinned scanners into `target/supply-chain-bin` and runs the advisory, license, and secret gates.
- `task image-verify` builds the distroless image and runs it read-only as UID 65532. Compose uses host networking because the process binds loopback only.

## Known boundaries
- A two-machine Tailcat session is not part of the automated suite.
- The room uses the markdown file as the artifact; metadata goes into the SQLite sidecar stored beside it unless `--ephemeral` is used.
