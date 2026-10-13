---
name: agents
description: Always-loaded project anchor. Identity, non-negotiables, commands, pointer to ROUTER.
last_updated: 2026-10-12
---

# Complexity Eater

Start every session by reading .mex/ROUTER.md before any other work.

## What This Is
Complexity Eater is a Rust workspace that serves a markdown-centric shared room, allowing a human and their AI agents to read the same artifact, propose edits, and commit only after approval.

## Non-Negotiables
- Keep the markdown file as the source of truth; sidecar SQLite metadata is support, not a second write path.
- Preserve the protocol identifiers (`commonroom`, `commonroom_session`, `commonroom://join/`, `.commonroom.db`).
- Do not add hosted tenancy or admin features that are not in the roadmap.
- Never use a transport vendor name in `ce-core`, `ce-http`, or `ce-protocol`.
- All state changes pass through the workspace engine and authorization checks.

## Commands
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `task check`, `task test`, `task e2e`, `task release`

## Scaffold Growth
After meaningful work, run the GROW step in ROUTER.md. Create or update patterns when the task type is new or non-obvious. The scaffold grows from real work.

## Navigation
Read .mex/ROUTER.md at the start of every session before doing anything else.
