---
title: Architecture
---

The [system diagram](diagram.md) is the picture of a running room. This page is the crate and request map.

Complexity Eater is one binary, `ce`, built from a Cargo workspace. The crates are boundaries that already exist in the running product. Empty crates were not added.

| Crate | Owns |
| --- | --- |
| `ce-protocol` | Error codes, capabilities, limits, invitation codec, discovery and protocol documents |
| `ce-core` | Workspace engine, SQLite metadata, markdown regions, skill loading, platform paths |
| `ce-transport` | Loopback and Tailcat plugins, the connection trait, invitation dialing |
| `ce-http` | Axum routes, the human page, the session ingress, MCP `2026-07-28` |
| `ce-cli` | `ce` command, room process, byte HTTP client, stdio MCP dialer |

`ce-core`, `ce-http`, and `ce-protocol` do not name transport vendors. A test walks those source trees and fails if they contain `tailcat`, `libp2p`, `iroh`, or `webrtc`. The CLI may name Tailcat because that is how a person selects the plugin. The transport crate implements it.

## Request path

```text
ce serve / ce join / ce mcp / browser
        |
        v
   ce-cli or ce-http
        |
        v
   WorkspaceEngine  (one mutex, explicit transitions)
        |
        +--> markdown file
        +--> <file>.commonroom.db
```

HTTP handlers and MCP tools call the same engine methods. The page is a projection of `/workspace/room`. It is not a second write path. Successful changes append a workspace event; the [event model](events.md) is how the page and MCP subscribers hear about it. Handoffs and their execution tasks use the same engine and the same sidecar; see [Tasks and handoffs](tasks.md).

## Two listeners

`ce serve` binds both on `127.0.0.1`.

* Owner port. `GET /` sets `commonroom_session`. The cookie is HttpOnly, SameSite=Strict, Path=/.
* Ingress port. Bearer sessions only. `POST /mcp` is mounted here. The owner port returns 404 for `/mcp`.

A transport plugin may publish the ingress. It does not publish the owner port. A non-loopback bind is refused with `refusing non-loopback bind; use a transport plugin for remote access`.

## Replaceable edges

These are real interfaces, used by the code that ships:

* `Transport`: `listen`, `connect`, `accept`, `send`, `receive`, `close`, `describe`, `capabilities`, and `expose` when a plugin publishes an already-listening port.
* `Connection`: a byte stream. The HTTP client speaks HTTP/1.1 over it with `Host: commonroom`. The transport does not decide authorization.
* SQLite is the current metadata store. The engine is the only writer. A future hosted store would implement the same operations; the domain types do not mention a vendor.

Identity, capability checks, lease conflicts, and commit approval stay in the engine. Possession of a socket or an invitation string is not membership.

## What is intentionally not abstracted

Billing, tenancy, license checks, and an admin console are not interfaces yet. Adding unused traits would pretend those products exist. The roadmap says where they attach: beside the engine, not inside each handler.
