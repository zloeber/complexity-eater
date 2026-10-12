---
title: Threat model
---

Complexity Eater assumes the people and agents in a room do not all trust each other, and that a transport provider can see that a connection exists. It does not assume a global operator. Local use has no account, no telemetry, and no required network.

## Assets

* The markdown substrate and its history.
* Invitation secrets and session ids.
* The host's approval and the right to remove a participant.
* The metadata database beside the file.

## Boundaries

An invitation names capabilities, an expiry, and a use count. Redeeming it creates a participant and a session. The session is what later requests present. The invitation secret is not a bearer token for workspace routes.

The owner page and the session ingress are different sockets. The owner cookie is not set on the ingress. A plugin is allowed to forward the ingress only.

`admin` and `*` may approve, invite, remove, and overwrite the document. A default agent cannot. HTTP reads the actor from the cookie or the `Authorization` bearer. A field in the JSON body cannot pick a different actor.

Committing a proposal requires `commit` and an approved proposal. Review requires `approve` and a proposal that is still pending. A stale base version does not land; the proposal becomes `conflicted` and the caller receives `stale_version`.

## What the transport is allowed to know

The plugin sees its own endpoint because it has to dial. Diagnostics, events, and MCP discovery do not echo that address, the invitation secret, or the session id. Logs from the HTTP adapter record the path, the status code, and the duration. They do not record the `Authorization` header or the body.

Tailcat is a subprocess. This binary does not implement Tailcat's cryptography. A missing or crashed `tailcat` process is `transport_unavailable`. It is not a fallback to an open listener.

## Abuse cases covered by tests

* Spent, expired, revoked, and legacy plaintext invitations.
* A second join after a successful redeem.
* A dial that never connects, which must leave `used_count` at 0.
* An agent raw commit and an agent review, both denied.
* A reader path hidden from non-admin status.
* Transport address stripped from diagnostics, history, banners, and MCP discovery.
* A non-loopback bind refused.
* Core crates forbidden from naming transport vendors.
* Session files written with mode `0600` under a `0700` directory on Unix.
* MCP tools filtered by capability. `workspace_review` is absent from a default agent tool list.

Header mismatch, oversized `Content-Length` (413 `limit_exceeded`), and the per-bearer rate limit are implemented in the HTTP adapter. They do not yet have a dedicated test.

## Out of scope for the local product

A malicious host can read the substrate; they already have the file. A person who can run code as the host user can read the sidecar database. Tenant isolation matters for a future multi-tenant host and is not a property of a single-user process.

No phone-home, no central identity, and no license check runs in `ce serve`.
