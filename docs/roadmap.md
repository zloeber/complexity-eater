# Roadmap

Local rooms and peer rooms are the product that exists. The items below are ordered so a hosted offering can be added without rewriting collaboration. None of them are required to run `ce`.

## Now

1. Close the Tailcat exception with a two-machine run, then record it in the parity ledger.
2. Add tests for MCP header mismatch, oversized bodies, and the rate limit.
3. Tag a release with checksums for the platforms CI builds.

## Next, still local

4. Package `ce` for Homebrew and a GitHub release asset. One binary per OS and architecture that CI can produce.
5. Keep the human page as a projection of the engine. Improve it only when a workflow cannot be done with `ce` or the page today.

## Self-hosted

6. Run the same engine behind a loopback or Tailcat ingress that an organization operates. Storage stays a database they control. Start with the existing SQLite file, then a store that implements the same operations.
7. Identity stays pluggable. The first self-hosted identity is the invitation and session model that already ships. An organizational login is an additional way to mint a session, not a replacement for capability checks.

## Hosted

8. Accounts and workspace ownership. A person keeps their agent identity. An organization's grant is a separate capability on a workspace, not a merge of the two.
9. Tenant isolation: one workspace's sessions, files, and events are not readable by another tenant's database role.
10. Audit retention that can ship events to the operator's store. Local mode still logs only to the process, with secrets redacted.
11. Billing and entitlements as a layer in front of workspace creation. The engine does not grow scattered license checks.
12. An admin surface for members and revoked invitations. It calls the same engine methods as `ce`.

Explicitly later, and not started: a public directory of rooms, a Docker-only distribution as the primary install, and any requirement that `ce serve` phone home.
