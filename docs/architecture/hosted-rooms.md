---
title: Hosted rooms
---

This is a proposal. Hosted room provisioning is not implemented. Room-launch contract version 1, in `website/contracts/room-launch.v1.json`, marks hosted mode `not_implemented` and sets `provider_endpoint` to null. The public site at `website/` must not grow a control that claims a room was created. `ce ui` shows the same gap and will not report a hosted endpoint as ready.

## Recommendation

Run a remote instance of the existing `ce` binary, one room process per isolated host. Publish the session ingress with the Tailcat plugin, or with another transport that implements the same boundary. Leave the owner port on loopback. Do not port the Rust core to another language to fit a hosting platform.

A shared supervisor is the second step, after one remote process has been operated on purpose. A Cloudflare Workers and Durable Objects implementation of the room is a second protocol stack. It is not the first host.

The personal handoff demo is that kind of adjacent service, and it is not a room. One Worker coordinates a manager-to-employee markdown handoff. The engine still owns room approval. See [Personal handoff](personal-handoff.md).

## Comparison

| Option | Idle cost | Concurrent rooms | Storage | Sockets | Cleanup | Portability |
| --- | --- | --- | --- | --- | --- | --- |
| One remote `ce` process per room | The machine stays up until the room is destroyed. A small always-on VM is commonly a few dollars to a few tens of dollars per month. Confirm the provider's current price. `ce serve` does not scale to zero. | One room per process. The container profile allows 512 MB, so the machine should be at least that large. | The markdown file and `<file>.commonroom.db` on a volume the operator controls. | The process holds SSE and MCP connections. The VM stays running while anyone is connected. | Stop the process and delete the volume. Repeating that is safe. | The same binary and the same invitation codec. Moving a room is copying the file and the sidecar. |
| Shared host, one process per room | One machine's floor, split across rooms. | Limited by RAM. Several rooms need several gigabytes plus overhead. | One directory per room. Still SQLite beside the markdown file. No shared database role. | Same as a single process, multiplied. A supervisor restarts a crashed room without touching the others. | Stop one process and delete its directory. A bug in the supervisor can cross rooms, which a single process cannot. | Still the Rust binary. The new code is the supervisor, and it stays beside the engine. |
| Workers and Durable Objects | Duration billing is cheaper for idle rooms only after a rewrite. A Worker that proxies to the binary still pays for that process. | A Durable Object can hold WebSockets. It cannot run the current Axum and Tokio process. | Durable Object storage is a different store from the sidecar. | Workers can terminate long-lived streams on their own schedule. The room today assumes a process that stays up. | Platform deletion is idempotent. The protocol behavior would be a reimplementation, so cleanup tests would start over. | Low. Two implementations of protocol version `"1"` will drift. |

## Room creation and ownership

An operator starts `ce serve` on `127.0.0.1` inside the host. The host session holds capability `*`. This phase adds no account, team, or billing record. The public website and the room origin stay separate. A page view is not a session.

## Invitations and sessions

Keep `commonroom://join/` and protocol version `"1"`. The defaults that already ship stay the defaults: 60 minutes, at most 7 days, and one use. A failed dial does not spend the invitation. Redeeming it creates a participant and a session. The invitation secret is not a bearer token for later workspace calls. Sessions default to 8 hours and last at most 24 hours.

Possession of a room identifier or a URL is not authorization. Live capabilities stay out of query strings, referrers, analytics, and ordinary logs.

## Owner and agent permissions

The owner cookie `commonroom_session` is HttpOnly, SameSite=Strict, and Path=/. It is set on the owner port. A transport plugin publishes the ingress only. Agents present `Authorization` bearers on that ingress. `POST /mcp` stays there, and the only accepted MCP revision is `2026-07-28`.

A default invitation grants `read`, `observe`, `propose`, `checkout`, and `commit`. It does not grant `approve`, `invite`, or `admin`. The engine checks the session on every mutation. A JSON body cannot choose the actor. A task id is not a credential.

## Transport and failure

Remote participants need a plugin. If it is missing or it crashes, the result is `transport_unavailable`. The process refuses a non-loopback bind. A hosted start that cannot publish the ingress should fail and leave no public listener. There is no silent fallback.

A two-machine Tailcat session has not been run in this repository's test suite. Remote reach depends on that unproven path.

## Expiration, revocation, and destruction

Invitations and sessions expire on their own. `ce remove-participant` ends one session. Destroying the room is stopping the process and deleting its volume. That sequence must be safe to repeat. When the process is gone, every session in that sidecar is gone. Keep the markdown file if it is evidence. Do not reuse a leaked invitation.

## Persistence and retention

The markdown file is the artifact. The sidecar holds participants, sessions, invitations, leases, proposals, events, and versions. The event table keeps the newest 2000 rows. It is a replay buffer for the room process, not a bus another host can tail.

Hosted v1 should not add a second database. Retention is the life of the volume. `--ephemeral` deletes the temporary sidecar directory on exit and leaves the markdown file. That is the wrong mode for a room a person expects to open again. Deleting a directory or a cloud volume does not, by itself, promise that the disk blocks were wiped. Read the provider's erasure claim separately.

The room does not execute markdown, scripts, or agent-generated code.

## Quotas and abuse

The limits that already ship stay in force: 1 MiB request bodies, 256 KiB proposals, 32 active sessions, 32 active tasks, and 600 authenticated requests per minute per bearer. A supervisor should also cap restarts and simultaneous connections. Invitation redeem is the unauthenticated entry, and it stays single-use.

Without accounts, a public create-room control would be an open provisioning API. Do not add one. The first hosted rooms are started by an operator who already controls the host.

## Audit and incidents

HTTP logs record the path, the status code, and the duration. They omit the `Authorization` header and the body. Diagnostics omit invitation secrets, session ids, and transport addresses. A hosted operator may ship those redacted lines to a store they control. Local mode still logs only to the process.

On a suspected leak, stop the process, preserve the markdown file, and treat the invitations from that sidecar as gone. There is no central account to disable.

## Version compatibility

Clients, the binary, and a future hosted service have to agree on invitation version `"1"` and MCP `2026-07-28`. Anything else returns `unsupported_version`. The room-launch contract version is a website contract. Version 1 describes local commands and an explicit hosted gap. A later contract can name a provider only after that provider exists. Old invitations are not rewritten. They fail when they expire, or when they are spent or revoked.

## Unresolved assumptions

* Tailcat between two machines is implemented and not covered by the automated suite.
* Who may start a hosted room, and how abuse is noticed, needs an operator. This proposal does not add accounts.
* Cloud volume deletion is not a cryptographic wipe unless the provider says so.
* A future owner page needs its own origin and a trusted-host list. `complexityeater.com` is the public site, not a room.
* The container reaches loopback with host networking on Linux. Docker Desktop on macOS does not. A hosted image needs a Linux network namespace.
* CSRF protection for a future browser owner page has to stay with SameSite cookies or an explicit origin check. The marketing site must not be that origin.
