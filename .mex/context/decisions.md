---
name: decisions
description: Architectural decisions, the rationale behind them, and any superseded choices.
edges:
  - target: architecture.md
    condition: when understanding why the system is structured this way
  - target: stack.md
    condition: when checking whether a runtime choice is consistent with project direction
  - target: setup.md
    condition: when verifying operational assumptions and supported environments
last_updated: 2026-10-12
---

# Architectural Decisions

## 2026-10-12 — Markdown remains the source of truth
- Rationale: The room is defined around a human-editable markdown file, and the sidecar database is support metadata rather than a separate system of record.
- Impact: All room state, proposals, and approval artifacts must stay consistent with the document and its versions.
- Status: Active

## 2026-10-12 — Single writer through `WorkspaceEngine`
- Rationale: The engine owns the mutex and all writes to the markdown file and SQLite metadata, which avoids split-brain writes and race conditions.
- Impact: HTTP handlers and MCP tools share the same engine API instead of creating parallel write paths.
- Status: Active

## 2026-10-12 — Protocol identifiers are fixed and user-visible
- Rationale: The project intentionally exposes `commonroom`, `commonroom://join/`, `commonroom_session`, and `.commonroom.db` as stable wire identifiers.
- Impact: Changing these strings is a protocol break and should be treated as a deliberate migration.
- Status: Active

## 2026-10-12 — Optional transport plugin model
- Rationale: The runtime supports loopback by default and optional Tailcat transport without making vendor-specific concepts part of the core crates.
- Impact: The CLI may select a transport plugin, while `ce-core`, `ce-http`, and `ce-protocol` remain vendor-neutral.
- Status: Active
