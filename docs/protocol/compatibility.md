---
title: Protocol
---

The protocol name is `commonroom` and the version is `"1"`. A numeric `1` is also accepted on invitations.

## Invitation

The copyable artifact is `commonroom://join/<base64url json>`. The JSON object contains `protocol`, `version`, `transport`, `workspace_id`, `secret`, `expires_at`, and `capabilities`. Keys are sorted. The raw JSON must be at most 8192 bytes.

`transport` is a hint: `type` (`loopback` or `tailcat` or `local`), `address`, `port`, `encrypted`, `mode` (`private` or `local`). The hint is how to dial. It is not the authorization.

Decoding a version other than `"1"` returns `unsupported_version` from the CLI. A broken token returns `invalid_request`.

## HTTP

Errors are JSON `{error, message, details}` plus recovery fields at the top level when they apply: `base_version`, `current_version`, `expected_version`, `diff`, `changes`, `retry_after_seconds`, `lease_id`, `proposal_id`, `region`.

| Code | Status |
| --- | --- |
| `unauthorized` | 401 |
| `permission_denied` | 403 |
| `invitation_expired`, `invitation_revoked`, `invitation_redeemed` | 403 |
| `unsupported_version` | 400 |
| `session_expired` | 401 |
| `lease_conflict` | 409 |
| `stale_version` | 409 |
| `proposal_not_found` | 404 |
| `proposal_conflict` | 409 |
| `invalid_region` | 400 |
| `version_not_found` | 404 |
| `transport_unavailable` | 503 |
| `limit_exceeded` | 429 or 413 for an oversized body |
| `invalid_request` | 400 |

`GET /workspace/observe` is a server-sent event stream for a session with `observe`. Send `Last-Event-ID` or `since_event_id` to resume. The stream replays events still in the sidecar ring, then pushes new ones. `event: resync` means the cursor fell off the ring or the subscriber lagged; read `/workspace/room` again. Heartbeats are comment lines. The retry hint is 5 seconds. A revoked session ends the stream. Authenticated HTTP requests are limited to 600 per minute per bearer. The observe stream counts as one request. List endpoints accept 1 to 200 items.

The byte client used by `ce join`, `ce call`, and `ce mcp` sends HTTP/1.1 with `Host: commonroom`. Request/response calls use `Connection: close` and read until `Content-Length`. `ce mcp` also opens `GET /mcp` as a server-sent event stream and writes MCP notifications to stdout.

## MCP

Only revision `2026-07-28` is accepted, and only on the session ingress.

* `POST /mcp` handles the call. `GET /mcp` is an event stream only when `Accept` includes `text/event-stream`, the bearer is valid, and `MCP-Protocol-Version` is `2026-07-28`. Any other GET, and DELETE, return 405. The owner port returns 404.
* A notification (`id` missing or null) on POST returns 202 and an empty body. There is no `initialize` handshake. Server-to-client notifications are the GET stream, not a second response body on POST.
* `MCP-Protocol-Version` must equal the body `_meta["io.modelcontextprotocol/protocolVersion"]` and must equal `2026-07-28`.
* `Mcp-Method` must equal the method. `Mcp-Name` must equal `params.name` or `params.uri` when the method is `tools/call`, `prompts/get`, or `resources/read`.
* A header mismatch is HTTP 400, JSON-RPC `-32020`, message `HeaderMismatch`.
* An unsupported version is HTTP 400, JSON-RPC `-32022`, message `UnsupportedProtocolVersionError`.
* An unknown method is HTTP 404, JSON-RPC `-32601`.
* `Origin`, when present, must be `127.0.0.1` or `localhost`, otherwise 403.

Tool permission failures are HTTP 200 results with `isError: true`. They are not JSON-RPC errors. The text is the same error object the HTTP API returns.

Discovery reports `cacheScope: public`, `supportedVersions: ["2026-07-28"]`, `resources.subscribe: true`, and the skills extension. It does not include invitation secrets, session ids, or transport addresses. It does not advertise list-changed notifications.

`resources/subscribe` and `resources/unsubscribe` take a resource URI. The session needs `read`. Subscribable URIs are `commonroom://workspace/status`, `commonroom://workspace/document`, `commonroom://workspace/proposals`, `commonroom://workspace/leases`, and `commonroom://workspace/participants`. A change notifies with `notifications/resources/updated` and the URI. The client rereads the resource. The notification is a hint: clients disconnect, some clients never subscribe, and a missed hint is recovered by an ordinary read or tool call. A notification does not authorize the next operation, and a commit does not wait for one.

Workspace mutations do not emit `notifications/tools/list_changed`, `notifications/prompts/list_changed`, or `notifications/resources/list_changed`. Progress messages and MCP tasks are not implemented. A normal mutation is one request and one response.

## Persistence

The markdown file is the artifact. The sidecar `<file>.commonroom.db` holds participants, sessions, invitations, leases, proposals, events, and versions. The event table is a ring of the newest 2000 rows. It is the replay buffer for `workspace.observe`, not a bus you can tail from another host. Live subscribers live in the room process and are gone when the process exits.

Timestamps are RFC 3339 with a numeric offset (`+00:00`) and microsecond precision.
