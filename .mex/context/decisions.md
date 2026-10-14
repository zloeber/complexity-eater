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
last_updated: 2026-10-14
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

## 2026-10-14 — A proposal replaces its region unless the caller opts into the whole document
- Rationale: Leasing a heading and then writing `change.content` over the entire file dropped every other region. Callers that mean the whole document can say so.
- Impact: `target` is the section when `change.section` is absent. `change.mode` of `document`, or a target of `workspace` or `*`, replaces the file. A `change.section` that resolves to a different path is `invalid_request`. A later commit rebases a proposal whose region does not overlap the edit and conflicts only on overlap. Bare titles that match more than one heading return `invalid_region` with `details.candidates`.
- Status: Active

## 2026-10-14 — The host session is bound to the printed loopback URL
- Rationale: Setting `commonroom_session` on every `GET /` let a non-loopback `Host` or a cross-site page receive the host capability.
- Impact: The owner listener allows `127.0.0.1`, `localhost`, and `[::1]` with the bound port. The Human URL includes a one-time token. Only that exchange sets the cookie, then redirects to `/`. Mutating owner routes reject `Sec-Fetch-Site: cross-site` and any `Origin` other than the loopback owner URL. `--host ::1` binds `[::1]`. Anything else still refuses a non-loopback bind.
- Status: Active

## 2026-10-14 — Room mode is review or baton, stored beside the document
- Rationale: Some rooms want a single writer for a short stretch without turning the default invitation into an approver. The markdown file stays the artifact either way.
- Impact: `ce serve --mode review|baton` defaults to review. Baton duration is capped (`--baton-max-minutes`, default 15; zero is refused). The holder writes one region with `workspace_write`. Other checkouts and writes see `baton_held`. Reads stay open. `workspace_propose` remains the advisory path and still needs approval to commit. Mode changes and revoke require `admin`. A baton commit records `baton_id` on `change.committed`. Revert appends a new version. Default invitations do not gain `approve`, `invite`, or `admin`.
- Status: Active

## 2026-10-14 — Room settings layer from TOML, and a baton grant can refuse further renewals
- Rationale: Flags alone cannot travel with a markdown room, and an unlimited renewal lets one participant hold the baton until everyone else gives up.
- Impact: `config.toml` in the platform config directory, then `<substrate>.commonroom.toml`, then `--config`, then an explicit flag. `[baton]` holds `max_minutes`, `default_minutes`, and `max_renewals`. A missing renewal cap stays unlimited. `0` refuses renewals. A later renewal returns `baton_renewal_limit`. Releasing the grant starts a new one. Unknown keys fail at load. The engine stores the resolved numbers; it does not grow a plugin trait for collaboration addons.
- Status: Active

## 2026-10-14 — Release provenance is the version tag
- Rationale: Dispatching the binary workflow on `main` recorded `refs/heads/main`, so a verifier pinning the tag ref failed.
- Impact: After semantic-release tags `vX.Y.Z`, the dispatch uses that tag ref. The install page documents `gh attestation verify` next to `SHA256SUMS`.
- Status: Active
