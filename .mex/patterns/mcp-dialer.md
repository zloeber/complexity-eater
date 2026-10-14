---
name: mcp-dialer
description: Validate Identity, `ce mcp`, and session ingress behavior when working with MCP clients.
triggers:
  - use the MCP client path
  - debug `ce mcp` or JSON-RPC failures
  - verify session ingress routing
edges:
  - target: ../context/architecture.md
    condition: when confirming the difference between owner port and session ingress
  - target: ../context/setup.md
    condition: when validating local CLI startup and invocation steps
  - target: ../ROUTER.md
    condition: when project state changed after a session or transport fix
last_updated: 2026-10-14
---

# MCP dialer

## Context
The project has a second process, `ce mcp`, that speaks JSON-RPC over stdin/stdout and dials `commonroom://join/` to reach the session ingress. The owner port does not serve `/mcp`.

## Steps
1. Prepare a valid room invitation and the target workspace.
2. Run `ce mcp --invite 'commonroom://join/...' --name "Agent"`.
3. Confirm that the client forwards HTTP/1.1 with `Host: commonroom` to the session ingress.
4. Validate that the connection is authorized by the bearer token, not by a JSON body field.

## Gotchas
- MCP revision `2026-07-28` is the only accepted revision.
- The session ingress is different from the owner port; the owner cookie does not grant access there.
- Failure to dial does not spend a one-use invitation; success does.
- An oversized body is `limit_exceeded` (JSON-RPC `-32000`) with `data.limit` and `data.size`. `workspace_commit` requires `proposal_id`. Tool schemas use underscore names such as `workspace_propose`.

## Verify
- Check the route and handler definitions in `ce-http` when debugging 401/403 behavior.
- Confirm the protocol contract and transport path in `ce-protocol` and `ce-transport` before changing session semantics.

## Debug
- If a client cannot connect, inspect whether the invitation is valid and not yet redeemed.
- If the connection reaches the server but fails authorization, verify the bearer token and session expiry.

## Update Scaffold
- [ ] Check whether `.mex/ROUTER.md` needs a note about session ingress behavior or a new failure mode.
- [ ] Update the relevant architecture or setup context if the client contract changes.
- [ ] Add or refine a pattern when a new MCP edge case is discovered.
