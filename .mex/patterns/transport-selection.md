---
name: transport-selection
description: Diagnose loopback vs. Tailcat behavior, transport availability, and invitation routing issues.
triggers:
  - choose a transport for a room
  - debug `transport_unavailable`
  - inspect invitation or dialing failures
edges:
  - target: ../context/architecture.md
    condition: when deciding whether a transport boundary is local or external
  - target: ../context/decisions.md
    condition: when validating the vendor-neutral transport choice
  - target: ../ROUTER.md
    condition: when current project state or known issues need to be refreshed
last_updated: 2026-10-12
---

# Transport selection

## Context
The runtime supports loopback by default and optional Tailcat transport. The transport layer is separate from the workspace engine and is not responsible for authorization decisions.

## Steps
1. Decide whether loopback or a plugin-backed transport is required.
2. If using Tailcat, ensure the binary is installed and reachable before opening the room or invitation.
3. Verify the transport description and the invitation flow before assuming that a listener is live.
4. If a connect attempt fails, confirm whether the problem is local listener availability, a stale invitation, or a transport mismatch.

## Gotchas
- A missing or crashed plugin is surfaced as `transport_unavailable` rather than as a public network listener.
- The core crates avoid vendor naming, so the transport choice remains implementation-specific.
- A transport address is not the same as an invitation; an invitation string carries the session semantics.

## Verify
- Run the CLI parity and transport-related tests, then validate the actual room flow with loopback first.
- Confirm that the transport crate and the CLI remain in sync on naming and plugin selection semantics.

## Debug
- If a session cannot be reached, check if the plugin is installed and functioning.
- If the room opens but the dial fails, verify the invitation string, transport endpoint description, and redaction logic.

## Update Scaffold
- [ ] Re-check `.mex/ROUTER.md` for current transport assumptions or gaps.
- [ ] Update `context/architecture.md` and/or `context/decisions.md` if the transport boundary changes.
- [ ] Add a pattern if a new plugin scenario or a public route requirement appears.
