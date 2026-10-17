---
name: ui-connectivity
description: Open the local management page, tell a tunnel from an invitation, and keep hosted mode honest.
triggers:
  - add or change `ce ui`
  - configure cloudflared or a remote adapter
  - explain why a remote agent cannot join
edges:
  - target: ../context/architecture.md
    condition: when the management page and the room process need to stay separate
  - target: transport-selection.md
    condition: when the room still needs loopback or an optional plugin
  - target: ../ROUTER.md
    condition: when current project state needs the connectivity gap refreshed
last_updated: 2026-10-17
---

# Management page and connectivity

## Context
`ce ui` is a loopback page for the local instance. The room is still `ce serve`. Hosted coordination is not implemented. A tunnel or Tailcat can make an ingress reachable. Neither one joins a workspace.

## Steps
1. Bind only `127.0.0.1`, `localhost`, or `::1`. Refuse every other host, including `0.0.0.0`.
2. Keep credentials out of `connectivity.toml`, diagnostics, and logs. cloudflared's login certificate stays in its own directory.
3. Require an explicit confirmation before enabling a remote adapter or starting `cloudflared`.
4. Point a tunnel at the room's session ingress. Never publish the management port.
5. Report hosted mode, custom endpoints, and offline packages as unavailable. Do not mark a tunnel pass as authorization.
6. Leave review, questions, and handoffs on the existing engine. They already run without a plugin.

## Gotchas
- A quick-tunnel hostname is a Cloudflare route, not a private network and not an invitation.
- `ce mcp` still dials the invitation transport hint. An HTTPS tunnel URL is not that hint.
- Stopping the child does not delete a named tunnel, DNS record, or the markdown sidecar.
- `Cf-Access-*` headers are not session identity.

## Verify
- `cargo test -p ce-cli --test ui`
- `cargo test -p ce-cli review_questions_and_handoff_survive_without_a_plugin`
- `cargo test -p ce-http provider_identity_headers_do_not_authenticate`

## Update Scaffold
- [ ] Refresh `.mex/ROUTER.md` if the hosted gap or the tunnel lifecycle changes.
- [ ] Keep vendor names out of `ce-core`, `ce-http`, and `ce-protocol`.
