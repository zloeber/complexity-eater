---
name: complexity-eater-adopt
description: Open a collaboration room around a local markdown file, start a room, invite another person or their agent, or open the local management page or management UI. Say what is shared and that approval stays with the host. Not for joining an invitation you were given or for a Complexity Eater hosted workspace.
---

# Open a Complexity Eater room

You are on the machine that has the markdown file. The public website does not create a room, and you must not claim that it did. This file does not create a hosted room.

Editing after you are connected is the `commonroom` skill. Leaving the file for another agent to read, then holding a local watch, is `complexity-eater-drop`. Feedback is `complexity-eater-review`. Sending work to another agent already in a room is `complexity-eater-handoff`. A failed connection is `complexity-eater-connect`.

## When to use

Use this to start a room, open a collaboration room, invite a person or their agent, or open the local management page. Examples: "Start a room for this document.", "Open a collaboration room for this.", "Invite my coworker.", "Open the local management UI."

## When not to use

- `ce` is missing. Follow `https://complexityeater.com/agent` (`complexity-eater-agent`) and come back.
- The person already handed you `commonroom://join/…`. That is `commonroom`.
- They want to leave the markdown for another agent and not stay with the room. That is `complexity-eater-drop`. If a room is already running, use `complexity-eater-handoff` instead of starting another drop.
- They asked to review, approve, or commit. Hosting is this skill. The decision is `complexity-eater-review` or `commonroom`.
- They asked for Complexity Eater's cloud. Hosted workspace provisioning is not implemented.

## Prerequisites

`ce` on `PATH`. The markdown file stays on this machine. Say what will be shared before you start: the file's text becomes the room artifact, the sidecar `<file>.commonroom.db` holds versions and proposals, and the invitation is a one-use secret. Install `tailcat` only when a participant is on another machine, and only after the person agrees.

## Shortest successful path

```bash
ce notes.md --name "Host"
```

That is the same as `ce serve notes.md --name "Host"`. The process binds `127.0.0.1` twice. The human page, printed as `Human:`, sets the cookie `commonroom_session`. Give the joining agent the `Invite:` line, not that URL.

Another invitation for a room that is already running:

```bash
ce invite --workspace notes.md
```

The default invitation grants read, observe, propose, checkout, and commit. It does not grant approve, invite, or admin. It expires in 60 minutes and works once. A failed dial does not spend it.

Tell the joining agent to read `skill://commonroom/SKILL.md` after it connects, or `https://complexityeater.com/skills/commonroom/SKILL.md` before that. The room also has the `workspace_onboarding` prompt. The same text is `https://complexityeater.com/prompts/workspace-onboarding.txt`. Prefer the paste block the running process prints. Leave that MCP server running.

The local management page is a different process and does not open a room:

```bash
ce ui
```

## Decision points

Same machine: omit `--tailcat`. Another machine, and the person agreed: `ce notes.md --name "Host" --tailcat`. `--ephemeral` deletes the temporary sidecar directory when the process exits. The markdown file stays. A missing `tailcat` with `--tailcat` exits `transport_unavailable` and does not open a public listener.

A Cloudflare tunnel is optional and is started later from `ce ui` after a confirmation. It is not this invitation. `ce mcp` still dials the invitation's transport hint.

## Success verification

The process printed `Human:` and `Invite:`. `ce status --workspace notes.md` shows the file. The joining agent can read `workspace_status` and cannot approve.

## Recovery

If the process exits, start it again on the same file. The sidecar remains beside the file unless you used `--ephemeral`. A failed dial does not spend the invitation. `invitation_redeemed` means you create another with `ce invite`. Do not reuse a spent string.

## Security

Do not put the invitation in a public page, a query string, or a ticket. Do not give out the owner URL. The host session can approve. A default agent invitation cannot. Propose changes. Approval stays with the workspace owner. `ce ui` does not publish the owner port.

## Fallback

Building from source is the fallback when the person asks for it. The toolchain is pinned in `rust-toolchain.toml`. `task release` writes `target/release/ce`. Loopback is the fallback when they decline `tailcat`. There is no hosted room to open instead.
