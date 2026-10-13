---
title: Tasks and handoffs
---

A handoff is work one participant asks another to do. A task is the execution record for that work. An event says the record changed. Approval of the result is a separate decision. These are not one status field.

The markdown file is still the artifact. Handoffs, tasks, and events live in the sidecar. A task id is not a bearer token. The session is.

## Lifecycles

| Record | States |
| --- | --- |
| Handoff | `open` → `accepted` → `result_ready` → `approved` or `rejected`. `cancelled` ends it earlier. |
| Task | `working` → `input_required` → `working` → `completed`. `failed` and `cancelled` are terminal. |
| Event | Append-only row in the sidecar ring. It is not a delivery receipt. |
| Approval | `handoff_approve` and `handoff_reject` require `approve` and a handoff in `result_ready`. They do not rewrite the task. |

Task statuses are the MCP Tasks statuses. There is no extra `accepted` task state. Accepting a handoff leaves the task `working` and records who is assigned.

## What is stored

A task has an id, owner, optional assignee, operation `handoff.execute`, status, timestamps, a 24 hour TTL, a 5 second poll hint, a correlation id, schema version `1`, and, when they exist, a result, a JSON-RPC error, or outstanding input requests. The result copied onto the handoff is what remains after the task row expires.

Creation is idempotent for a participant plus `idempotency_key`. The second call returns the original handoff and task. A second completion does not replace the result.

At most 32 tasks may be `working` or `input_required` in one room. Summaries are capped at 2,000 characters, constraints at 8,000, and a result at 256 KiB. Artifact references are stored and are not fetched.

## MCP

Discovery advertises `io.modelcontextprotocol/tasks` for protocol `2026-07-28`. The client declares that extension on the request, under `_meta["io.modelcontextprotocol/clientCapabilities"].extensions`. A missing declaration is JSON-RPC `-32021`.

`handoff_create` returns `resultType: "task"` when the client declared the extension, and an ordinary tool result when it did not. The task id is durable either way. `tasks/get`, `tasks/update`, and `tasks/cancel` are the protocol methods. `Mcp-Name` is the task id. Domain tools (`handoff_accept`, `handoff_complete`, `handoff_get`, `handoff_events`, `handoff_approve`) stay request/response.

`tasks/get` returns `resultType: "complete"` plus the task. Unknown and unauthorized task ids both come back as `-32602` and the message `unknown task`. `tasks/update` answers `inputRequests` created by `handoff_request_input`. `tasks/cancel` acknowledges with an empty result. If the task is already terminal, the status stays as it was. If it is not, this server marks it `cancelled` before the acknowledgement.

`subscriptions/listen` is a long-lived `POST /mcp` response with `Content-Type: text/event-stream`. The first frame is `notifications/subscriptions/acknowledged`. The server omits notification types it will not send. Tool, prompt, and resource catalogs do not change, so `toolsListChanged`, `promptsListChanged`, and `resourcesListChanged` are omitted. `resourceSubscriptions` are the same workspace URIs as `resources/subscribe`, and only for a session that can `read`. `taskIds` are included only when `tasks/get` would succeed for that session.

Later frames are `notifications/resources/updated` or `notifications/tasks`, each carrying `io.modelcontextprotocol/subscriptionId`. Closing the response, or sending `notifications/cancelled` for that request id, ends the subscription. A lagged subscriber is dropped. Nothing from the gap is replayed on the next listen.

`resources/subscribe` and `GET /mcp` still work for clients that already use them. They do not carry task notifications.

## Recovery

1. Authenticate with a session that still has the needed capability.
2. Read the handoff, `tasks/get`, or `GET /workspace/task`. That is the authority.
3. Open `subscriptions/listen` again if you want live hints.
4. After a gap, read the handoff events or `workspace_history`. Do not treat the listen stream as a cursor.

The event row and the state change commit in one SQLite transaction. Live delivery happens after that commit and does not wait for a client. A client that was disconnected, and a process that restarted, still see the sidecar. The ring keeps the newest 2,000 events. Older ids are gone; the handoff and, until TTL, the task are not.

Terminal task rows are deleted one TTL after their last update. The handoff keeps the result text. A non-terminal task older than one TTL from creation is marked `failed` with `task expired`, and an open handoff is cancelled. Active work inside the TTL is not deleted.

## Deployment

One room process is the writer. The sidecar is `<file>.commonroom.db`. Live subscribers are in that process. Opening the same database from a second writer is not a supported cluster. There is no broker and no webhook sender. Another machine uses the transport plugin to reach this process; it does not get a second copy of the task table.

`ce handoffs`, `ce handoff`, `ce handoff-events`, `ce task`, and `ce task-cancel` call the same engine methods as the HTTP routes and the MCP tools. `ce task` requires `--participant` because a task id alone is not authorization. Listing handoffs is the local operator reading the room file, the same way `ce proposals` does.

## Compatibility

| Client | This server |
| --- | --- |
| MCP `2026-07-28` without the tasks extension | Tools and `resources/subscribe` behave as before. `handoff_create` returns the handoff in the tool result. `tasks/get` is `-32021`. |
| MCP `2026-07-28` with the tasks extension | `handoff_create` may return a task handle. `tasks/get`, `tasks/update`, `tasks/cancel`, and `subscriptions/listen` are available. |
| MCP `2025-11-25` | Rejected. This server never implemented `initialize` or `Mcp-Session-Id`. |
| HTTP and `ce` | Same engine methods. No second business logic. |

Protocol `2026-07-28` is still the only accepted `MCP-Protocol-Version`. There is no separate MCP SDK in this repository. The JSON-RPC adapter that already checked headers and versions learned the tasks methods and the listen stream. It does not implement a second framing stack.
