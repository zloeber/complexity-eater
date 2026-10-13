---
name: architecture
description: Major components and how the room, engine, transport, and HTTP/MCP surfaces connect.
edges:
  - target: ../ROUTER.md
    condition: when starting a session or checking project navigation
  - target: stack.md
    condition: when verifying runtime and dependency choices
  - target: conventions.md
    condition: when validating project structure or review expectations
last_updated: 2026-10-12
---

# Architecture

## Components
- `ce-cli` owns the room lifecycle and CLI entrypoints (`ce serve`, `ce join`, `ce mcp`, `ce invite`, etc.).
- `ce-core` owns the `WorkspaceEngine`, SQLite sidecar schema, permissions, proposals, leases, and markdown region logic.
- `ce-http` exposes the human page, session ingress, MCP endpoint, and the HTTP routes consumed by the CLI and agents.
- `ce-transport` implements loopback and optional Tailcat transports and the connection abstraction used when sending frames between peers.
- `ce-protocol` defines the shared error contracts, capability model, invitation encoding, and protocol documents.

## Primary data flow
1. The room is opened around a markdown file and a SQLite metadata sidecar named `<file>.commonroom.db`.
2. `ce-cli` creates or joins a workspace and calls into `WorkspaceEngine` for reads, writes, proposals, and approvals.
3. HTTP handlers in `ce-http` authenticate a bearer token or `commonroom_session` cookie, then delegate to the same engine methods used by MCP.
4. Proposals are stored in SQLite with a base version, then reviewed and committed only after authorization.
5. Successful mutations append an event in the sidecar and fan out to browser SSE and MCP resource subscribers; the stream may resync if a cursor is lost or the consumer falls behind.
6. The markdown file remains the authoritative artifact; external edits are treated as newer versions and can conflict with stale proposals.

## External boundaries
- The owner port serves the human page and cookie-based ownership context.
- The session ingress is separate and accepts bearer tokens; `POST /mcp` is mounted only there.
- Invitations are encoded with the `commonroom://join/` scheme and may point to loopback or a discovered transport.
- A transport plugin is optional; a missing plugin surfaces as `transport_unavailable` rather than a public listener.

## Important invariants
- Authorization is derived from identity, not from JSON body values.
- The engine is a single mutex-backed writer for the workspace state and markdown file.
- Core crates intentionally avoid transport-vendor naming to keep the protocol and engine transport-agnostic.
