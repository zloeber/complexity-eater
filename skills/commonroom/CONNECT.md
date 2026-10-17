---
name: complexity-eater-connect
description: Diagnose an agent connect failure, a connection failed error, or why Tailcat or a Cloudflare tunnel did not help. Check the invitation, the host, loopback, and permissions before changing anything. A tunnel is reachability, not authorization. Complexity Eater's cloud does not host rooms. Do not rotate an invitation unless the person asks.
---

# When another agent cannot connect

Find which layer failed. Do not mint a new invitation, revoke a session, or start a tunnel until you know which one it is and the person asks you to change it.

## When to use

Use this when a connection failed, another agent cannot get in, or the person asks about Tailcat, a tunnel, or hosting on Complexity Eater's cloud. Examples: "Why can't the other agent connect?", "The connection failed.", "Install Tailcat.", "Host this on Complexity Eater's cloud."

## When not to use

- They want a room created. That is `complexity-eater-adopt`. Leaving a file for another agent to read is `complexity-eater-drop`. A tunnel is not that watch.
- They have a valid invitation and want to join. That is `commonroom`.
- They want feedback recorded. That is `complexity-eater-review`.
- Installing `ce` itself is `complexity-eater-agent`. Installing Tailcat is optional and starts here only so you can explain it. Do the install from the agent skill after they agree.

## Prerequisites

The host process, the invitation text, and the transport hint inside it. `workspace_diagnostics` names the plugin and mode. It does not reveal the address. `ce ui` is available for the management page. `cloudflared` and `tailcat` are optional programs this repository does not install for you.

## Shortest successful path

1. Leave the invitation as it is. Do not rotate or recreate an invitation until the person asks.
2. Confirm the host is still running. A stopped `ce` process is not a bad invitation.
3. Read the error code. Branch on `error`, not the message.
4. Match the hint. Loopback works only on the host machine. A `tailcat` hint needs `tailcat` on the joining machine. A missing plugin with `--tailcat` exits `transport_unavailable` and does not open a public listener.
5. A failed dial does not spend the invitation. `invitation_redeemed` means it was already used. `invitation_expired` and `invitation_revoked` need a new invitation, and only if the person asks.

## Decision points

`unauthorized` means the call had no session. `permission_denied` means the session lacks a capability. A default invitation does not grant approve, invite, or admin. That is not a transport failure.

A tunnel provides connectivity, not authorization. From `ce ui`, a Cloudflare quick or named tunnel is a `cloudflared` child started only after the person confirms. The origin is the room's session ingress, never the management port. A quick-tunnel hostname is not an invitation. ce mcp still dials the invitation's transport hint. The supported cross-machine dialer in this version is the optional Tailcat plugin. A two-machine Tailcat session is not part of the automated test suite.

Complexity Eater's cloud does not host rooms. Hosted workspace provisioning is not implemented. A local management UI is not a hosted workspace.

## Success verification

The joining agent completes `POST /session/join` once and `workspace_status` returns a version. `workspace_diagnostics` shows the plugin you expected. A tunnel test on the management page reports a running child and still says a connection is not a workspace session.

## Recovery

Reconnect with the same `session_id` until the session expires. Do not join a second time on a spent invitation. After a disconnect, read `workspace_history` or the resource again. A notification is a hint. Stopping `cloudflared` does not delete the markdown file, the sidecar, a named tunnel, or the person's Cloudflare login.

## Security

Do not paste the invitation, a bearer token, a Cloudflare API token, or a quick-tunnel hostname into a ticket or this website. The management page does not install `cloudflared`, call the Cloudflare API, or store a tunnel token. `Cf-Access-*` headers are not session identity. Installing Tailcat needs the person's agreement. Use an install method from the Tailcat install document. Do not invent a package name.

## Fallback

Same-machine work uses loopback and needs neither Tailcat nor a tunnel. Without the management page, `ce serve` on loopback is still the room. Offline handoff packages are not implemented. There is no hosted room to switch on.
