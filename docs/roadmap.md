---
title: Roadmap
---

Local rooms and peer rooms are the product that exists. The items below are ordered so a hosted offering can be added without rewriting collaboration. None of them are required to run `ce`.

## Now

1. Run Tailcat between two machines and record what held.
2. Add tests for MCP header mismatch, oversized bodies, and the rate limit.

## Next, still local

3. Package `ce` for Homebrew. Tag builds already publish Linux and macOS binaries.
4. Keep the human page as a projection of the engine. Improve it only when a workflow cannot be done with `ce` or the page today. Handoffs and MCP tasks are sidecar records. The page does not render them yet. Live notifications stay hints; there is no shared broker in local mode.

## Self-hosted

5. Run the same engine behind a loopback or Tailcat ingress that an organization operates. Storage stays a database they control. Start with the existing SQLite file, then a store that implements the same operations.
6. Identity stays pluggable. The first self-hosted identity is the invitation and session model that already ships. An organizational login is an additional way to mint a session, not a replacement for capability checks.

The public website in `website/` is a static front door. It does not provision rooms. A comparison of later hosting shapes is in [Hosted rooms](architecture/hosted-rooms.md).

## Hosted

7. Accounts and workspace ownership. A person keeps their agent identity. An organization's grant is a separate capability on a workspace, not a merge of the two.
8. Tenant isolation: one workspace's sessions, files, and events are not readable by another tenant's database role.
9. Audit retention that can ship events to the operator's store. Local mode still logs only to the process, with secrets redacted.
10. Billing and entitlements as a layer in front of workspace creation. The engine does not grow scattered license checks.
11. An admin surface for members and revoked invitations. It calls the same engine methods as `ce`.

Explicitly later, and not started: a public directory of rooms, a Docker-only distribution as the primary install, and any requirement that `ce serve` phone home.
