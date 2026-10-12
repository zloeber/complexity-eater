# Protocol compatibility

The protocol name is `commonroom` and the version is `"1"`. A numeric `1` is accepted on invitations because the reference coerced versions with `str(version)`.

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

`GET /workspace/observe` is a server-sent event stream. Authenticated requests are limited to 600 per minute per bearer. The retry hint is 5 seconds. List endpoints accept 1 to 200 items.

The byte client used by `ce join`, `ce call`, and `ce mcp` sends HTTP/1.1 with `Host: commonroom` and `Connection: close`, and reads until `Content-Length`.

## MCP

Only revision `2026-07-28` is accepted, and only on the session ingress.

* `POST /mcp` handles the call. `GET` and `DELETE` return 405. The owner port returns 404.
* A notification (`id` missing or null) returns 202 and an empty body. There is no `initialize` handshake.
* `MCP-Protocol-Version` must equal the body `_meta["io.modelcontextprotocol/protocolVersion"]` and must equal `2026-07-28`.
* `Mcp-Method` must equal the method. `Mcp-Name` must equal `params.name` or `params.uri` when the method is `tools/call`, `prompts/get`, or `resources/read`.
* A header mismatch is HTTP 400, JSON-RPC `-32020`, message `HeaderMismatch`.
* An unsupported version is HTTP 400, JSON-RPC `-32022`, message `UnsupportedProtocolVersionError`.
* An unknown method is HTTP 404, JSON-RPC `-32601`.
* `Origin`, when present, must be `127.0.0.1` or `localhost`, otherwise 403.

Tool permission failures are HTTP 200 results with `isError: true`. They are not JSON-RPC errors. The text is the same error object the HTTP API returns.

Discovery reports `cacheScope: public`, `supportedVersions: ["2026-07-28"]`, and the skills extension. It does not include invitation secrets, session ids, or transport addresses.

## Persistence

The markdown file is the artifact. The sidecar `<file>.commonroom.db` holds participants, sessions, invitations, leases, proposals, events, and versions. Opening an existing Python sidecar uses that schema, including the columns added during the Commonroom security work. There is no separate import step.

Timestamps are RFC 3339 with a numeric offset (`+00:00`) and microsecond precision, matching Python's `datetime.isoformat()`.
