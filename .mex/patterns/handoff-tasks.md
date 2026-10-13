---
name: handoff-tasks
description: Create a durable handoff, poll or subscribe to its task, and recover the result after a disconnect or restart.
triggers:
  - an agent asks another participant to finish work asynchronously
  - debug tasks/get, tasks/cancel, or subscriptions/listen
  - a client missed a notification and needs the authoritative result
edges:
  - target: ../context/architecture.md
    condition: when the engine, sidecar, or MCP boundary is unclear
  - target: workspace-events.md
    condition: when the question is the room event stream rather than a task
  - target: INDEX.md
    condition: when this pattern needs to be updated or cross-referenced
last_updated: 2026-10-12
---

# Handoffs and execution tasks

## Context
The engine is the only writer. A handoff is the collaboration record. A task is the MCP execution record. Events are the recovery log. Approval is a later decision with the `approve` capability. Live `subscriptions/listen` frames are hints and are not replayed.

## Steps
1. Create the handoff with `handoff_create` or `POST /workspace/handoff/create`. Pass an idempotency key when a retry must not create a second task.
2. If the client declared `io.modelcontextprotocol/tasks`, keep the returned `taskId`. Otherwise read the task id from the tool result and poll with the domain tools.
3. The worker calls `handoff_accept`, then `handoff_complete`. `tasks/get` and `handoff_get` return the stored result after a restart.
4. Subscribe with `subscriptions/listen` and `notifications.taskIds` only for tasks that session can read. The acknowledgement lists the ids the server accepted.
5. After a disconnect, call `tasks/get` or `ce task --participant`. Use `handoff_events` for the gap. Open a new listen for anything newer.

## Gotchas
- A task id is not a bearer token. Unauthorized and unknown ids return the same `unknown task` error.
- `tasks/cancel` stops work that is not already terminal. A finished result stays finished.
- Terminal task rows expire 24 hours after their last update. The handoff keeps the result text.
- `GET /mcp` and `resources/subscribe` do not carry `notifications/tasks`.
- Revision `2025-11-25` is rejected.

## Verify
- Restart the process after accept and before complete, then read the task.
- Complete once, complete again, and confirm the result text did not change.
- Subscribe as a participant who is not the owner or assignee and confirm the task id is absent from the acknowledgement.
