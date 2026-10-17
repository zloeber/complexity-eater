---
name: router
description: Session bootstrap and navigation hub. Read at the start of every session. Contains project state, routing, and the GROW contract.
edges:
  - target: context/architecture.md
    condition: system design, integrations, component relationships
  - target: context/stack.md
    condition: technologies, libraries, tech decisions
  - target: context/conventions.md
    condition: writing or reviewing code, project patterns
  - target: context/decisions.md
    condition: architectural choices or “why is it built this way”
  - target: context/setup.md
    condition: first-time setup or running the project
  - target: patterns/INDEX.md
    condition: starting any concrete task — check for a matching pattern first
last_updated: 2026-10-17
---

# Session Bootstrap

Read .mex/AGENTS.md if you have not already. Then read this file fully before any other work.

## Current Project State
- The repo is a Rust workspace with five crates: `ce-protocol`, `ce-core`, `ce-transport`, `ce-http`, and `ce-cli`.
- The source of truth remains the markdown file; the sidecar SQLite database stores versions, proposals, leases, and events.
- The core runtime is the `WorkspaceEngine` in `ce-core`, while HTTP and MCP routes live in `ce-http`.
- The default integration points are loopback listeners plus optional `tailcat` transport support from the CLI.
- Live room updates are fanned through browser SSE, MCP resource subscriptions, and `subscriptions/listen`. Task and handoff state is durable in the sidecar. Listen streams are hints and are not replayed.
- Current validation paths are `task check`, `task test`, `task e2e`, `task release`, `task supply-chain`, `task image-verify`, and `task website:check`.
- The public website lives in `website/` and is a static front door for `complexityeater.com`. It does not call the workspace engine. It publishes the commonroom skill, the adoption skill, and the `workspace_onboarding` prompt from `skills/commonroom/`. `https://complexityeater.com/agent` is the agent install procedure, generated from `skills/commonroom/AGENT.md` and the current release manifest. GitHub Pages still publishes `docs/`. Hosted room provisioning is not implemented. The comparison is `docs/architecture/hosted-rooms.md`, and the local launch contract is `website/contracts/room-launch.v1.json`.
- Rust 1.99.0 is pinned. Release archives and the distroless image are scanned before publication. The container runs as UID 65532 and uses host networking because the process binds loopback only. The binary workflow is dispatched on the version tag so provenance records `refs/tags/vX.Y.Z`.
- A proposal replaces the leased region. Whole-document replacement is explicit: `change.mode` of `document`, or a target of `workspace` or `*`. A commit rebases an in-flight proposal whose region does not overlap the change, and conflicts only on overlap.
- The owner listener sets `commonroom_session` only for a loopback `Host` and only after the one-time token in the printed Human URL. Mutating owner routes reject a foreign `Origin`.
- The room mode is `review` by default. `baton` lets the holder write one region directly, with a time cap. Mode, holder, and the cap are on `workspace_status`. A room can cap renewals of one grant. Status, region lists, and unscoped reads stay bounded. An oversized body surfaces as `limit_exceeded`.
- Room settings can come from TOML. The user file is `config.toml` in the platform config directory. `<substrate>.commonroom.toml` overrides it, `--config` overrides both, and an explicit CLI flag wins. Omitted flags keep the file value.
- `demo/handoff` is a Cloudflare Worker and one Durable Object for the manager-to-employee markdown handoff. It is not a hosted room. The live page is `https://complexity-eater-handoff.zloeber.workers.dev`. Documents are in the Durable Object because Artifacts returned `10403` and `10004` on this account. The Artifacts git path runs only when the binding is deployed. Mock profiles are simulated. Room approval stays in the engine.
- `ce ui` is a loopback management page. It does not open a room. Hosted coordination stays unimplemented. A Cloudflare quick or existing tunnel is an optional `cloudflared` child started only after confirmation, and it is not workspace authorization. Tailcat remains optional. Offline handoff packages are not implemented.
- The human page is the review canvas. Questions, responses, and comments are sidecar rows anchored to a region quote. They do not change the markdown. A document edit is still a proposal. `propose` also covers `answer` and `comment`. Default invitation capabilities are unchanged. `commonroom://workspace/review` is the MCP hint. The Worker demo does not store these rows.

## Behavioural Contract
1. Start here every session.
2. Before a concrete task, open `patterns/INDEX.md`. If a matching pattern exists, follow it.
3. Load only the `context/` and `patterns/` files indicated by edges and the task. Do not load the whole scaffold.
4. After meaningful work, execute GROW before considering the task complete.

## GROW
- Ground — what actually changed?
- Record — surgically update Current Project State and any affected `context/` files. Preserve frontmatter. Never delete decisions; mark them superseded.
- Orient — if this was a new or non-obvious task type, create or refine a pattern and update `INDEX.md`.
- Write — bump `last_updated` on every file you touched. Keep edges useful and preferably bidirectional.

# Notes

This project is intentionally small, opinionated, and protocol-defined. The architecture favors a single writer, explicit permissions, and a markdown substrate that remains readable outside the app.
