---
name: workspace-events
description: Subscribe to live room updates, reload on resync, and keep the event stream bounded and non-authorizing.
triggers:
  - browser or MCP client watches the room for updates
  - debug an SSE or resource-subscription issue
  - verify a change notification after a write or proposal
edges:
  - target: ../context/architecture.md
    condition: when understanding how the engine fans events out to the HTTP and MCP surfaces
  - target: ../ROUTER.md
    condition: when current project state should reflect a live notification or resync issue
  - target: INDEX.md
    condition: when this pattern needs to be updated or cross-referenced
last_updated: 2026-10-14
---

# Workspace events and subscriptions

## Context
The engine is the only writer. After a successful mutation it records the state change, inserts one normalized event, and fans it to subscribers. Browser streams and MCP resource subscriptions are notification hints; they read the authoritative workspace state back rather than acting as a second write path.

## Steps
1. Verify the mutation completed in the engine and inserted a `change.*` or `proposal.*` event.
2. For the browser stream, use `GET /workspace/observe` with `Last-Event-ID` or `since_event_id`. If a gap or lag is detected, reload `/workspace/room` instead of trusting partial state.
3. For MCP resource hints, subscribe with `resources/subscribe` or `subscriptions/listen` on a read-capable session and expect `notifications/resources/updated`. Task status uses `subscriptions/listen` with `taskIds` and is recovered with `tasks/get`, not by replaying the stream. See [handoff-tasks](./handoff-tasks.md).
4. Re-read the authoritative resource after the notification; do not treat the notification as authorization or as a complete record of the body.
5. If the stream falls behind, reconnect and resubscribe; the next connection receives the current subscriptions as hints.

## Gotchas
- A notification is not a write and is not progress for an in-flight request.
- Event payloads omit secrets, document bodies, proposal change bodies, and transport addresses.
- Slow readers are dropped; the next connection sees `resync` and rebuilds state from the engine.
- A stale proposal or a newer document commit can surface `proposal.conflicted` or `proposal.superseded` without a general “updated” event.
- Baton grants, renewals, releases, expiry, and revocation are `baton.*` events. A mode switch is `room.mode_changed`. A baton commit is still `change.committed`, and its payload includes `baton_id`.

## Verify
- Confirm event ordering through the monotonic `event_id` and the engine’s write mutex.
- Exercise the browser stream or the MCP subscribe path with a small mutation and check that the notification arrives.
- Run the project tests and parity checks after changing the event or resource flow.

## Debug
- If subscribers stop seeing updates, check whether the subscriber buffer is full or the session has expired or been revoked.
- If a resource update does not fire, confirm the session has `read` and the URI is one of the subscribable workspace resources.
- If the stream shows `resync`, reload `/workspace/room` or re-subscribe rather than assuming the writer failed.

## Update Scaffold
- [ ] Review `.mex/ROUTER.md` and confirm current project state mentions live subscription behavior.
- [ ] Update `context/architecture.md` if the event fanout boundary or resource set changed.
- [ ] Add a new pattern only if another subscription edge case emerges.
