---
name: complexity-eater-drop
description: Leave this markdown as a context drop, or send this context to another agent, when no room is running yet. Hold the local watch until the file is read. Not for a room that already exists, for approving a proposal, or for a hosted workspace.
---

# Leave a context drop

A drop is a Complexity Eater room whose pickup window is separate from the later session. You leave a markdown file for another agent. The harness starts the room in the background and holds a local watch until the file is read or the window ends with no read. After a read, the same process stays up for ordinary room work. This is not a hosted workspace and not an offline package. `ce serve` does not change. The room still serves only `skill://commonroom/SKILL.md`.

## When to use

Use this when no room is running and the person wants another agent to read the file. Examples: "Leave this markdown for my coworker to pick up.", "Start a context drop for this file.", "Send this context to another agent."

## When not to use

- If a room is already running, use complexity-eater-handoff instead of starting another drop.
- They want to open a live room and stay with it. That is `complexity-eater-adopt`.
- They pasted an invitation. That is `commonroom`. After a pickup, reads and proposals are `commonroom`.
- They want a proposal accepted or committed. Answering is not approval. The drop does not approve or commit.
- "Summarize this document" with no other agent is ordinary reading. A summary does not pick up.
- Installing `ce` is `complexity-eater-agent`. A failed connection is `complexity-eater-connect`.

## Prerequisites

`ce` on `PATH`. The markdown file is on this machine. Say what will be shared before you start. The file becomes the room artifact, and the invitation is a secret. Hosted workspace provisioning is not implemented. Tailcat is optional and needs the person's agreement. A tunnel is not the watch and not authorization.

## Shortest successful path

```text
ce drop <file> --name <name> [--for <minutes>] [--pickups <n>] [--idle <minutes>] [--ephemeral] [--tailcat] [--json]
```

Defaults are `--for 60`, `--pickups 1`, and `--idle 15`. `--for` is at least 1 and at most 10080. `--pickups` is at least 1. `--idle 0` disables idle shutdown. `--ephemeral` deletes the temporary sidecar on exit and leaves the markdown file. Add `--tailcat` only after the person agrees. A missing plugin is `transport_unavailable` and does not open a public listener.

Stdout is two JSON lines and nothing else. The started line has `pid`, `invite`, and `human`. The second line is `picked_up`, `expired`, or `closed`.

```json
{"event":"started","pid":12345,"invite":"commonroom://join/…","human":"http://127.0.0.1:…/?token=…"}
{"event":"picked_up","participant":"Ada","at":"…","pickups":1,"pickups_remaining":0}
```

The receipt omits the invitation and the document body. Stderr prints the existing `Human:` and `Invite:` banner. The parent exits 0 on `picked_up`, exits 2 on `expired`, and exits 3 on `closed`.

A pickup is the first participant `workspace_read` or `review_section`. Joining does not pick up. A summary does not pick up. The owner page does not pick up. A session whose capabilities include `*` does not pick up. A second read by the same session does not pick up again. The engine records the pickup row before those bytes are returned.

After `picked_up` the room stays up. The time limit and pickup count only stop new pickups. They do not stop a session that already has a pickup row.

## Decision points

"Send this context to another agent" can match this skill and `complexity-eater-handoff`. If a room is already running, use complexity-eater-handoff instead of starting another drop. This skill applies when no room exists yet. Neither skill approves a proposal.

`--idle 0` disables idle shutdown. Idle timing starts at the first pickup, not at process start. It resets when a room event is appended or a participant document read succeeds. When that idle duration passes, the child exits and the markdown file stays.

Either the host or a session that already picked up may `ce drop close --workspace <file>` or call `drop_close`. `POST /workspace/drop/close` is the same close. A join-only session cannot. Any other session gets `permission_denied`. Close is not `admin`.

Same machine: omit `--tailcat`. Another machine, and the person agreed: add `--tailcat`. Do not start a tunnel to carry the watch.

## Success verification

Stdout has a `started` line and then one terminal line. The parent exits 0 on `picked_up`, exits 2 on `expired`, and exits 3 on `closed`. After `picked_up` the room stays up. `pickups_remaining` is how many new sessions may still read. A second participant at `--pickups 1` is refused with `drop_closed` and `details.reason` of `exhausted`, and the body is absent.

## Recovery

`expired` means the window ended with zero pickups and the child exited. Start another drop only if the person still wants one. `closed` means it was closed before any pickup. If the parent dies before a terminal event, the child exits and does not leave a room behind. After `picked_up`, the parent exiting does not stop the child. A session that already picked up keeps its `session_id`. Reconnecting does not need a new invitation. A new join still fails. A new join after the deadline returns `invitation_expired`. A new join after the count is spent returns `drop_closed` with `details.reason` of `exhausted`. A session with no pickup row that reads after the window closed gets `drop_closed` and no body. `details.reason` is `expired`, `exhausted`, or `closed`.

## Security

The watch is local. Do not put the invitation in a ticket or on the website. Do not give out the owner URL. The watch pipe is an inherited pipe, not a published socket, not a tunnel, and not an invitation. A tunnel is not the watch and not authorization. Answering is not approval. The drop does not approve or commit. A default invitation grants `read`, `observe`, `propose`, `checkout`, and `commit`. It does not grant `approve`, `invite`, or `admin`.

## Fallback

If a room is already running, use complexity-eater-handoff instead of starting another drop. Loopback is the fallback when the person declines Tailcat. Hosted workspace provisioning is not implemented. There is no hosted drop to open instead. Without MCP, `ce drop close --workspace <file>` still asks the running drop to shut down.
