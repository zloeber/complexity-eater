---
name: context-drop
description: Start a local context drop, read the watch receipt, and close it without treating pickup as approval.
triggers:
  - leave a markdown file for another agent to read
  - ce drop, drop_close, or the pickup window
  - a second participant is refused or the watch expires
edges:
  - target: ../ROUTER.md
    condition: when starting a session or refreshing project state
  - target: ../context/architecture.md
    condition: when the watch, the room child, and the engine boundary are unclear
  - target: handoff-tasks.md
    condition: when a room is already running and the next step is asking the other agent to work
  - target: public-website.md
    condition: when a page might claim it created the drop
  - target: INDEX.md
    condition: when this pattern needs to be updated or cross-referenced
last_updated: 2026-10-17
---

# Context drop

## Context
`ce drop` is a parent watch on the machine that has the file. It spawns the current executable as the room child. The child is an ordinary room. The pickup window is not the later session. The public site publishes `skills/commonroom/DROP.md` and does not start the watch.

## Steps
1. If a room is already running for the file, stop and use `complexity-eater-handoff`. Do not start a second drop.
2. Run `ce drop <file> --name <name>`. Defaults are `--for 60`, `--pickups 1`, and `--idle 15`. `--for` is at most 10080. `--idle 0` disables idle shutdown.
3. Read stdout as two JSON lines. The first is `started` (`pid`, `invite`, `human`). The second is `picked_up`, `expired`, or `closed`. The parent exits 0, 2, or 3 for those three. Stderr still prints `Human:` and `Invite:`.
4. After `picked_up`, leave the child up. Reads and proposals follow `commonroom`. Asking the other agent to do work follows `complexity-eater-handoff`.
5. Close with `ce drop close --workspace <file>`, `drop_close`, or `POST /workspace/drop/close`. The host may close. A session may close only after its pickup row exists.

## Gotchas
- A pickup is the first participant `workspace_read` or `review_section`. Join does not pick up. A summary does not pick up. The owner page does not pick up. A session with capability `*` does not pick up.
- After `picked_up` the room stays up. The deadline and the pickup count only stop new pickups.
- Idle starts at the first pickup, not at process start. `--ephemeral` removes only the temporary sidecar directory.
- If the parent dies before a terminal event, the child exits. After `picked_up`, the parent exiting does not stop the child.
- Answering is not approval. The drop does not approve or commit.
- A join-only session cannot close. Close is not `admin`.
- The watch is local. Do not put the invitation in a ticket or on the website.
- Tailcat is optional and needs agreement. A tunnel is not the watch and not authorization. A missing plugin is `transport_unavailable`.
- Hosted workspace provisioning is not implemented.
- Core crates stay vendor-neutral.

## Verify
- `python3 scripts/skill_coverage.py` routes a leave-this request to `complexity-eater-drop` and a send-this-context request to that skill and `complexity-eater-handoff`.
- Engine tests, once `drop_close` and `POST /workspace/drop/close` are in the crates: a summary inserts no pickup, the first `workspace_read` does, a second participant at `--pickups 1` gets `drop_closed`, and a joined session with no pickup row cannot close.

## Debug
- Exit 2 is `expired` with zero pickups. Exit 3 is `closed` before any pickup. Exit 0 means the child is still the room.
- `drop_closed` carries `details.reason` of `expired`, `exhausted`, or `closed`, and no document body.
- A new join after the deadline is `invitation_expired`. A session that already picked up keeps its `session_id`.

## Update Scaffold
- [ ] Refresh `.mex/ROUTER.md` if the watch stops being a local parent process.
- [ ] Update `context/architecture.md` if pickup stops being a document read.
