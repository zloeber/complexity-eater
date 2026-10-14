---
name: commonroom
description: Join a Complexity Eater room as a participant. Observe the room, lease a region, propose a change, and commit only after review.
---

# Complexity Eater

This is a temporary room around something a person already has on their computer. You are a participant. The artifact stays on the host. You do not get a copy to overwrite.

An invitation is a secret. It looks like:

```text
commonroom://join/<opaque>
```

That string is the capability. It is not a transport address. Treat it like a password. It expires and can be revoked.

## Connect

Decode the invitation. Inside the opaque blob is JSON with:

- `protocol` and `version`
- `transports`: hints for how to reach the room
- `transport`: the first hint
- `workspace_id`
- `secret`: present this when you join; it is not a session
- `expires_at`
- `capabilities`

Use the first hint whose transport plugin you have. A hint's `type` is the plugin name (`loopback`, or a private rendezvous plugin the host started). Dial it, then speak HTTP to the commonroom protocol on that connection.

A successful dial does not admit you to the room. Join next.

If the host gave you the `ce` command, dial and join, then exit so you can keep working:

```bash
ce join 'commonroom://join/...' --name '<your name>' --kind agent --json --no-hold
```

Keep `session_id`. Each later operation dials again. Do not join again.

```bash
ce call 'commonroom://join/...' GET /workspace/status --session '<session_id>'
ce call 'commonroom://join/...' POST /workspace/propose --session '<session_id>' --body '<json>'
```

A hint whose `type` is `tailcat` needs the `tailcat` program on your machine. If it is missing, tell your human and wait for approval before installing it. Do not turn that address into a URL, and do not guess a different endpoint.

When your host can launch a local program, prefer MCP. Add this server and leave it running:

```text
command: ce
args: ["mcp", "--invite", "commonroom://join/..."]
```

That process dials the invitation and speaks MCP. It joins once. Read the `commonroom` skill from the server with `skills/list` and `skills/get` when the host supports `io.modelcontextprotocol/skills`. Otherwise read the `workspace_onboarding` prompt. This file does not install or activate itself.

If you cannot launch that server, use `ce join` and `ce call` below.

A failed dial does not spend the invitation. Joining spends one use. Reconnect with the same `session_id` until the session expires. A second join on a spent invitation returns `invitation_redeemed`. Anyone who can read the invitation can redeem it. There is no recipient binding.

## Discover

On the connection, before you change anything:

1. `GET /.well-known/agent-workspace`
2. `GET /protocol`
3. `GET /skills/commonroom`

Prefer those documents over a remembered API. They list operations, error codes, and the transport plugin entry point group. They do not contain the artifact.

## Join

```http
POST /session/join
Content-Type: application/json

{"invite": "<secret>", "name": "<your name>", "kind": "agent", "transport": "<hint type>"}
```

Send the returned `session_id` after that:

```http
Authorization: Bearer <session_id>
```

Your capabilities are the ones in the invitation. Asking for more does nothing. When the session expires, join again with an invitation that is still valid.

## Work in this order

HTTP operations use dots (`workspace.propose`). MCP tools use underscores (`workspace_propose`). They call the same engine.

1. `workspace_status` — version, heading count, and `mode` (`review` or `baton`)
2. `workspace_summary`
3. `workspace_regions` for a page of full paths (`depth`, `under`, `cursor`)
4. `workspace_read` for one region. A read with no section is a short excerpt (`truncated`, `continuation`, `next_offset`)
5. In `review` mode: `workspace_checkout`, then `workspace_propose`, wait until `approved`, then `workspace_commit` with `proposal_id`
6. In `baton` mode: `workspace_baton_request`, then `workspace_write` while you hold it. Release with `workspace_baton_release`
7. `workspace_release` when you drop a lease

Do this. Do not read the whole artifact and write it back.

`workspace_commit` requires `proposal_id`. It does not accept a raw document. A raw overwrite is a separate host action.

### The change object

`workspace_propose` and `workspace_write` take:

```json
{"content": "## Plan\n\n- the new text\n", "section": "Doc/Plan"}
```

`content` is required. It replaces the target region, including that heading, and leaves every other region in place. `section`, when you send it, must name the same region as `target`. A bare title that matches more than one heading returns `invalid_region` and `details.candidates`. Use the full path from `workspace_regions`.

Whole-file replacement is explicit: `change.mode` is `document`, or `target` is `workspace`. The review page shows that as a whole-document replacement.

### Room mode

`review` is the default: propose, then someone with `approve` accepts, then commit. Unrelated regions can land together. A second proposal on a region that changed returns `stale_version`.

`baton` lets the holder write with `workspace_write` and no approval. Request `workspace_baton_request` with optional `minutes` and `reason`. A request above the room cap is clamped; the result's `expires_at` is the real deadline. If someone else holds it, you get `baton_held` with `details.holder` and `details.expires_at`. Renew with `workspace_baton_renew`. An expired baton returns `baton_expired` and does not change the file. A write without a baton returns `baton_required`. Reads stay open. `workspace_propose` still works as an advisory path for people who do not hold the baton.

Only the host (`admin`) can switch modes or revoke the baton. Switching back to `review` revokes the baton, and `workspace_write` then returns `mode_mismatch`. A default invitation cannot do either.

`workspace.observe` and `workspace.history` tell you what the room did while you waited. `workspace.diagnostics` tells you which transport plugin is listening. It does not reveal the endpoint.

If this host offers `resources.subscribe`, subscribe to `commonroom://workspace/proposals` or `commonroom://workspace/document` and treat `notifications/resources/updated` as a hint to read that resource again. The notification is not the document and it is not approval. If you cannot subscribe, keep using the tools. After a disconnect, read the resource or call `workspace_history`.

## Handoffs

`handoff_create` asks another participant to do a piece of work. If you declare `io.modelcontextprotocol/tasks` on the request, the result may be a task handle (`resultType: "task"`). If you do not, the tool result is the handoff itself and a task id is still inside it. The task id is not a credential.

The other participant calls `handoff_accept`, then `handoff_complete` with the result. You read it with `tasks/get` or `handoff_get`. `handoff_events` is the recovery log. `subscriptions/listen` with that `taskId` is a live hint and does not replay what you missed. Approval is `handoff_approve`, and only a session with `approve` can do it. Completing the task does not approve the result.

## Regions

Markdown regions are ATX heading paths such as `Architecture` or `Architecture/Goals`. A heading needs a space after the hashes. Text inside a fenced code block is not a heading. Setext headings (`===` / `---`) are not regions. Checkout `section:Architecture` or the full path. Read that section instead of the file. If a name matches more than one heading, pass the full path.

## Leases

A lease expires (about 15 minutes) and everyone can see it. `lease_conflict` means someone else holds that region. Read `workspace.leases`, pick another region, or wait. Renew with `workspace.renew` while you still hold it. Release when you stop.

## Conflicts

`stale_version` means the room moved. Do not retry the same body. Read `current_version`, `diff`, and `changes`. Look again, then write a new proposal.

Other codes you can act on: `unauthorized`, `permission_denied`, `invitation_expired`, `invitation_revoked`, `invitation_redeemed`, `unsupported_version`, `session_expired`, `proposal_conflict`, `invalid_region`, `limit_exceeded`, `baton_held`, `baton_required`, `baton_expired`, `mode_mismatch`, `transport_unavailable`.

`limit_exceeded` includes `details.limit` and `details.size`. `invalid_region` for an ambiguous name includes `details.candidates`.

Branch on the `error` field. Do not scrape the message.

## Etiquette

- Observe before you write.
- Lease the smallest region you need.
- Propose a summary a human can review in the room.
- Do not approve, invite, or administer unless the session says you can.
- `workspace.leave` when you are finished, and release any lease you still hold.
