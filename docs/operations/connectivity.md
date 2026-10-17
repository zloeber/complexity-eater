---
title: Connectivity
---

`ce ui` opens a management page on this machine. It does not open a room, and it does not replace `ce serve`, `ce join`, or `ce mcp`.

```sh
ce ui
ce ui --host 127.0.0.1 --port 0
ce ui --no-browser
```

The default bind is `127.0.0.1`. `--port 0` asks the operating system for a free port and the command prints the URL it actually bound. The URL carries a one-time token. Opening it sets an HttpOnly, SameSite=Strict cookie named `ce_ui_session` and redirects to a clean path. Ctrl+C stops the page. A tunnel started from the page is a child of that process and stops with it.

`127.0.0.1`, `localhost`, and `::1` are the only accepted binds. Anything else, including `0.0.0.0`, is refused. The page is not reachable from another machine.

## What is already local

Document review, anchored questions, and session handoff run in the room process. They use the session ingress and the sidecar. They do not need Tailcat, and they do not need a tunnel. Start the room with `ce serve notes.md`. Agents on the same machine use the printed invitation:

```sh
ce mcp --invite 'commonroom://join/...' --name "Agent"
```

The metadata database is `<file>.commonroom.db` beside the markdown file. Stopping a tunnel does not delete that file. A dropped connection does not discard a committed version, an answer, or a completed handoff.

Hosted coordination is not implemented. The page can store an `https://` URL as a note. It cannot authenticate to a hosted service, and it will not report that service as ready. See [Hosted rooms](../architecture/hosted-rooms.md).

A portable offline handoff package is not implemented. In-room handoffs stay in the sidecar.

## Remote access

Cross-network use needs a path a remote agent can actually dial: a tunnel, a private-network plugin, or some other reachable ingress. Tailcat is one optional plugin. It is not part of the collaboration protocol. A missing plugin is `transport_unavailable`. The process does not open a public listener instead.

The management page is never the tunnel origin. The origin is the room's session ingress, `http://127.0.0.1:<port>`, and only after you confirm. A reachable ingress is not a participant. Workspace identity is still the invitation. Default invitations grant `read`, `observe`, `propose`, `checkout`, and `commit`. They do not grant `approve`, `invite`, or `admin`. Revoke an invitation from the Access section, or end a participant with `ce remove-participant`.

`Cf-Access-Jwt-Assertion` and `Cf-Access-Authenticated-User-Email` are not read. A client that can reach the ingress still has to join.

### Managed tunnel

The managed adapter is `cloudflared`, run as a child of `ce ui`. This page does not install it, does not call the Cloudflare API, does not change DNS, and does not install an operating-system service.

1. Install `cloudflared` from Cloudflare's documented downloads. Reload the page. The page reports whether the binary is on `PATH` and, when `cloudflared --version` succeeds, the version.
2. Quick tunnel. After you confirm, start runs `cloudflared tunnel --url http://127.0.0.1:<ingress> --no-autoupdate`. Traffic goes through Cloudflare to a `trycloudflare.com` hostname. That hostname is not a private network. Anyone who learns it can open a connection. Treat it as a secret. It is not an invitation. Diagnostics redact it.
3. Existing named tunnel. Create it yourself with `cloudflared tunnel login`, `cloudflared tunnel create NAME`, and `cloudflared tunnel route dns NAME HOST`. The login certificate stays in cloudflared's directory. Do not paste it, an API token, or an account key into this page. Start then runs `cloudflared tunnel run --url http://127.0.0.1:<ingress> NAME`. DNS and Cloudflare Access policies remain in the Cloudflare account. This application cannot see whether the hostname registered.

The room must already be listening on the ingress port. If it is not, start is refused and nothing is published. Stop ends the child. Disable clears the local preference and stops the child. A named tunnel, its DNS record, and the login certificate remain until you delete them with cloudflared or the Cloudflare dashboard. Quick-tunnel hostnames stop when the child stops.

This binary does not verify TLS certificates. A TCP connection to port 443, or a URL printed by cloudflared, is not certificate verification and is not workspace authorization. Plan limits and pricing are Cloudflare's.

`ce mcp` dials the invitation's transport hint. A quick-tunnel URL is not that hint. The supported cross-machine dialer in this version is still the optional Tailcat plugin. The tunnel publishes the session ingress for an HTTPS client. It does not, by itself, teach `ce mcp` a new transport.

### Private network

Tailcat is optional. The page can remember that you want it. It does not install or start the plugin. Publish with:

```sh
ce serve notes.md --tailcat
```

The plugin can relay through its provider, and that provider can see that a connection exists. A connected plugin is not an authorized participant. A two-machine Tailcat session is not part of the automated test suite.

### Custom endpoint

Not available. The room process refuses every non-loopback bind, so an administrator hostname in front of a public listen address is not an adapter here.

## Checks

The connectivity test reports each fact separately:

| Check | Meaning |
| --- | --- |
| Local process | This management process is running. |
| Local API | The management API answered on loopback. |
| Adapter | Whether cloudflared is installed, and whether the child is running. A private-network preference is not a running plugin. |
| Remote endpoint | Whether cloudflared has printed a hostname, or the named-tunnel process is up. Loopback is not a remote endpoint. |
| TLS | Not checked. This binary does not verify certificates. |
| Authentication | Not a workspace login. Hosted authentication is not implemented. |
| Authorization | Not granted by the tunnel. An invitation is required. |
| Collaboration | Supported in the local room. Not implemented for a hosted service. |

A green or "pass" line is only that check. Remote access configured does not mean an agent is authorized.

## Diagnostics

The Diagnostics section copies a redacted report. It omits invitation secrets, session ids, bearer tokens, Cloudflare API tokens, and quick-tunnel hostnames. Do not add those to the report by hand.

## Failure and recovery

| What you see | What to do |
| --- | --- |
| `cloudflared is not installed` | Install it yourself and reload. The page will not download it. |
| `nothing is listening on the loopback origin` | Start `ce serve` first. The ingress port is the origin, not the management port. |
| `explicit confirmation is required` | The remote action was refused. Confirm it in the page. Nothing was exposed. |
| `cloudflared exited` | Read the redacted lines. The sidecar is still beside the markdown file. Start the tunnel again after the room is listening. |
| Hosted mode stays unsupported | There is no hosted backend in this version. Use a local room. |
| A remote agent cannot dial the tunnel URL with `ce mcp` | Use Tailcat for the supported cross-machine invitation, or an HTTPS client that already speaks the session ingress. The tunnel did not become an invitation. |
