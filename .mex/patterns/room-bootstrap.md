---
name: room-bootstrap
description: Start and validate a room around a markdown file with the correct owner and session boundaries.
triggers:
  - open a new workspace
  - start a local room
  - debug room creation or authorization behavior
edges:
  - target: ../context/architecture.md
    condition: when confirming room boundaries and the write path
  - target: ../context/setup.md
    condition: when verifying local build or run commands
  - target: ../ROUTER.md
    condition: when starting a new session or updating project state
last_updated: 2026-10-14
---

# Room bootstrap

## Context
The room is opened with `ce serve` or `ce <file>`, and it creates a loopback owner port plus a session ingress. The markdown file stays as the artifact while SQLite holds metadata.

## Steps
1. Build or verify the binary with `cargo build --locked --bin ce` or `task build`.
2. Run `ce serve notes.md --name Ada` or `ce notes.md --name Ada`.
3. Open the printed Human URL. It carries a one-time token. That exchange sets `commonroom_session` and redirects to `/`. A request without the token, or with a non-loopback `Host`, does not set the cookie. The session ingress handles bearer-token access.
4. Invite or join a participant through the room flow, making sure the invitation is not mistaken for a raw transport address.

## Gotchas
- The owner cookie is on the human page, not on the session ingress. `--host` accepts `127.0.0.1`, `localhost`, or `::1`. `::1` binds `[::1]`.
- Room mode defaults to review. See [room-mode](./room-mode.md) before using `--mode baton`.
- A failed dial does not spend an invitation; a successful join does.
- `--ephemeral` keeps metadata in a temporary directory without moving the markdown file.

## Verify
- Run `cargo test --workspace --locked` and the room-specific parity test when changing workspace behavior.
- Check that the markdown file remains the authoritative substrate after writes or proposals.

## Debug
- If the room does not bind, inspect the CLI flags and port configuration.
- If permission errors occur, verify bearer-token or owner-cookie identity before reviewing the proposal or session logic.

## Update Scaffold
- [ ] Review `.mex/ROUTER.md` and ensure current project state still matches the room lifecycle.
- [ ] Update `context/architecture.md` and/or `context/setup.md` if the room model changes.
- [ ] If this workflow exposed a new failure mode, add or refine a pattern in `INDEX.md`.
