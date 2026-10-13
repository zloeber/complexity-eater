---
name: conventions
description: Naming, layout, error handling, testing, and review patterns used by the codebase.
edges:
  - target: architecture.md
    condition: when comparing architectural boundaries to implementation structure
  - target: decisions.md
    condition: when validating whether a new change aligns with established design decisions
  - target: patterns/INDEX.md
    condition: before writing a new code or workflow pattern
last_updated: 2026-10-12
---

# Conventions

## Code organization
- Each crate has a focused responsibility: protocol, core runtime, transport, HTTP, and CLI.
- Modules stay close to their runtime responsibility, and the workspace is split by feature area rather than by UI or service layer.
- The core engine and HTTP handlers share the same methods, so route code does not create a second write path.

## Naming and API style
- Rust code uses standard idiomatic naming (`WorkspaceEngine`, `Session`, `Proposal`, `Lease`, `Access`).
- Error handling is centralized via the protocol error model (`CommonroomError` and helpers like `invalid_request`, `unauthorized`, and `stale_version`).
- Capability- and permission-related code uses normalized capability sets rather than ad hoc flags.

## Error and validation patterns
- Code checks both request limits and authorization at the HTTP boundary.
- `ce_core` returns structured protocol errors instead of leaking raw panics or transport details.
- Stale proposals and external file edits are explicitly surfaced as user-visible protocol states.

## Testing and verification
- The repo favors targeted Rust tests plus workspace-wide validation commands.
- In addition to unit/integration checks, a parity test covers the CLI and proposal lifecycle flow.
- Logging through `tracing` is intentionally lightweight but visible at operation boundaries.

## Review and workflow guidance
- Treat Markdown as the persisted artifact and look for sidecar metadata parity when reviewing changes.
- When modifying protocol semantics or permission logic, review both the engine and the HTTP/MCP surfaces together.
- Prefer small, surgical changes and preserve compatibility with the existing protocol identifiers.
