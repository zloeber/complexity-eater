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
  - target: ../patterns/handoff-tasks.md
    condition: when the handoff and task split needs the operational steps
last_updated: 2026-10-13
---

# Architectural Decisions

## 2026-10-12 — Handoffs, tasks, and events stay separate records
- Rationale: A collaboration request, its execution, the hint that it changed, and human approval are different decisions. MCP task statuses are the protocol's statuses, not a second copy of the handoff lifecycle.
- Impact: Task rows and handoff rows live in the sidecar. `subscriptions/listen` does not replay. `tasks/get` and the handoff are authoritative. There is no broker and no second MCP stack.
- Status: Active

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

## 2026-10-13 — Supply-chain controls sit beside the release, not in the engine
- Rationale: Binary, image, and dependency hardening should use the existing Taskfile, lockfile, and GitHub release workflow. Room authorization stays in the engine.
- Impact: Rust 1.99.0 is pinned. `cargo deny` enforces the license and source policy. Release archives are scanned before upload. The container is distroless, runs as UID 65532, and uses host networking because `ce` only binds loopback. Attestations record the workflow and commit. They are not a claim that the bytes are free of vulnerabilities.
- Status: Active
