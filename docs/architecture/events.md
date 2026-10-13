---
title: Workspace events
---

A room has one writer: the workspace engine. After a successful domain change is stored, the engine appends one event and wakes in-process subscribers. The markdown file and the sidecar remain the authority. Events tell a listener that something changed. They are not a second copy of the document, and they are not a command channel.

## Envelope

Schema version is `1`. Each event has:

| Field | Meaning |
| --- | --- |
| `event_id` | Monotonic id from the sidecar. This is the cursor. |
| `event_type` | One name from the taxonomy below. |
| `schema_version` | Always `1` for events this process writes. |
| `workspace_id` | The room. An id is not a credential. |
| `created_at` | RFC 3339, microsecond precision, numeric offset. |
| `version` | Workspace version when the change has one. |
| `actor_id` | Participant id when the change has one. |
| `payload` | Small metadata: ids, status, target, summary. |

The summary is capped at 240 characters. Payloads drop invitation secrets, session ids, document bodies, proposal change bodies, and transport addresses. A listener that needs the document or the proposal reads it back.

## Taxonomy

The names are the ones the engine already used. They are finer than a single "updated" event where the room has a real transition.

| Change | Event |
| --- | --- |
| Sidecar bootstrap | `workspace.created` |
| Process start or stop | `workspace.started`, `workspace.stopped` |
| Participant enters or leaves | `participant.joined`, `participant.left` |
| Session issued or expired | `session.created`, `session.expired` |
| Markdown version appended, including an external edit of the file | `change.committed` |
| Proposal created | `proposal.created` |
| Review | `proposal.approved`, `proposal.rejected` |
| Commit of an approved proposal | `proposal.committed` |
| Stale proposal or a newer commit | `proposal.conflicted`, `proposal.superseded` |
| Lease taken or renewed | `lease.acquired` |
| Lease released or expired | `lease.released`, `lease.expired` |
| Invitation created or revoked | `invite.created`, `invite.revoked` |
| Handoff opened, accepted, finished, approved, rejected, or cancelled | `handoff.created`, `handoff.accepted`, `handoff.result_ready`, `handoff.approved`, `handoff.rejected`, `handoff.cancelled` |
| Execution task created or changed | `task.created`, `task.updated` |

A denied call does not emit the success event for that call. A second release of a lease, or a second revoke of an invitation, does not emit another event. `proposal.conflicted` is recorded because the proposal status did change; `change.committed` is not.

There is no `workspace.updated` event. Version changes are `change.committed`.

## Ordering and durability

One mutex covers a mutation and the event insert. `event_id` order is the mutation order for that process. Subscribers can see duplicates and must ignore an id less than or equal to their cursor.

Proposal, lease, and session changes insert the event after the row change, still under the engine mutex. A crash between those two statements can leave a change without an event. Handoff and task changes write the row and the event in one SQLite transaction, then publish. Live delivery is after that commit. A failed insert does not wake subscribers.

The sidecar keeps the newest 2000 events. Older ids are gone. That ring is the only replay. It is not a durable log you can ship to another machine.

## Fanout

Subscribers are bounded channels, 64 events by default. Publication uses a non-blocking send and does not wait on the subscriber, so a slow reader cannot stall a commit. When a subscriber's buffer is full, the next receive is `lagged`. The buffered events are discarded. The caller loads current state from the engine and continues from the latest id it has stored.

Dropping a subscriber removes it. One subscriber's lag does not affect another.

## Browser stream

`GET /workspace/observe` is a server-sent event stream. It requires the session cookie or bearer and the `observe` capability. The owner page uses it. The session ingress does not gain an MCP route from this endpoint, and the owner port still does not serve `/mcp`.

The cursor is the `Last-Event-ID` header, or `since_event_id` when the header is absent. The stream replays retained events after that id, then waits on the fanout. Each event is:

```text
id: 12
data: {"event_id":12,"event_type":"proposal.created",...}

```

A gap in the ring, or a lagged subscriber, is:

```text
event: resync
data: {"reason":"gap"}

```

`reason` is `gap` or `lagged`. The page then reloads `/workspace/room`. Heartbeat comments arrive about every 15 seconds, and the same pass checks that the session is still valid. A revoked or expired session ends the stream. `retry: 5000` tells the browser to reconnect after five seconds.

The page applies an event once, by id. A gap or a `resync` reloads the room. Proposal, lease, participant, and document events refresh those sections. The invitation text already on the page is left alone. If the stream errors, the page says `reconnecting` or `offline` and reloads the room every 30 seconds until the stream opens again. Buttons still call the same HTTP routes.

People listed on the page are participants who have not left. The list is membership, not a live presence signal.

## MCP

Discovery advertises `resources.subscribe: true` and the tasks extension for protocol `2026-07-28`. It does not advertise list-changed notifications. Tool, prompt, and resource lists do not change when the markdown changes. Task notifications are documented with [handoffs](tasks.md).

Workspace resources, for a session that can `read`:

| URI | Body |
| --- | --- |
| `commonroom://workspace/status` | Status. The path is omitted without `admin`. |
| `commonroom://workspace/document` | Markdown, capped at 1 MiB. |
| `commonroom://workspace/proposals` | Proposals. |
| `commonroom://workspace/leases` | Active leases. |
| `commonroom://workspace/participants` | Members who have not left. |

`skill://commonroom/SKILL.md` is readable and does not accept a subscription.

`resources/subscribe` and `resources/unsubscribe` take `{ "uri" }`. The session needs `read`. An unknown or oversized URI is rejected and does not reveal other rooms. A URI is not authorization; the session is.

`GET /mcp` with `Accept: text/event-stream` and `MCP-Protocol-Version: 2026-07-28` is the server-to-client stream, on the session ingress only. A GET without that Accept header is still 405. The owner port does not mount `/mcp`.

A notification is the standard JSON-RPC message, with no id:

```text
event: message
data: {"jsonrpc":"2.0","method":"notifications/resources/updated","params":{"uri":"commonroom://workspace/proposals"}}

```

The server sends that when a subscribed resource changes, and again for each current subscription when the stream connects. The client rereads the resource. The notification does not include the document or the proposal body, and it does not authorize the next call.

`ce mcp` opens this stream beside its request connection and writes those notifications to stdout. A client that never subscribes, or never reads the stream, still uses tools and resources the way it did before.

If a stream falls behind, it is closed. The next `GET /mcp` receives the current resource subscriptions as update hints. `subscriptions/listen` starts with an acknowledgement and does not replay. Nothing is queued for a client that is gone. A successful commit does not wait for a client to see the notification.

## What this is not

A notification is not progress for the request in flight. Ordinary proposal and lease mutations stay request/response. Task status is a separate record, described in [Tasks and handoffs](tasks.md). `notifications/tasks` is a hint that the task row changed. `tasks/get` and the handoff are the authority. `subscriptions/listen` does not replay events from the ring.

There is no broker and no hosted event log. A process restart keeps the sidecar ring and loses the in-memory subscribers. After a restart, read the resources, the handoff, or the task.
