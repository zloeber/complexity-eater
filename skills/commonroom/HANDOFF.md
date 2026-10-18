---
name: complexity-eater-handoff
description: Hand this task to another agent, ask a coworker's agent to do the work, or pick up a conversation and resume a handoff after a disconnect. Say what context is shared and what completion means. A handoff does not import an arbitrary chat and does not prove the recipient finished. Not for approving a proposal or installing ce.
---

# Hand work to another agent

A handoff is a sidecar record in a room that already exists. It is not a copy of either agent's private memory. Each agent keeps its own user, tools, and permissions.

## When to use

Use this when the person wants to hand this task to another agent, ask a coworker's agent for work or a perspective, or pick up a conversation and resume after a disconnect. Examples: "Hand this task off to another agent.", "Ask my coworker's agent to review this.", "Help me pick up a conversation from another agent session."

## When not to use

- The room does not exist yet. Leaving the markdown for another agent to read is `complexity-eater-drop`. Opening a live room and staying with it is `complexity-eater-adopt`. Come back to this skill after a room is running.
- They pasted an invitation and want to join. That is `commonroom`.
- They want the feedback recorded as questions and answers. That is `complexity-eater-review`. A handoff can point at that work. It does not replace it.
- "Please accept this proposal" is approval, not a handoff.
- "Summarize this document" with no other agent is ordinary reading.
- Installing `ce` is `complexity-eater-agent`.

## Prerequisites

A session with `propose` to create a handoff. A default invitation has `propose`. The worker needs `commit` to `handoff_accept` and `handoff_complete`. A default invitation has `commit`. `approve` is a later decision and is not included by default. The other agent needs its own invitation. Do not share yours.

Say what will be shared before you create it: the summary, the constraints, and any artifact refs you pass. Those refs are paths or ids the room already knows. This does not import an arbitrary conversation history.

## Shortest successful path

1. `handoff_create` with a summary. Pass an idempotency key when a retry must not create a second task. `POST /workspace/handoff/create` is the same operation.
2. If the client declared `io.modelcontextprotocol/tasks`, the result may be a task handle. The task id is also inside the handoff. The task id is not a credential.
3. The other participant calls `handoff_accept`, does the work, then `handoff_complete` with the result.
4. You read it with `handoff_get` or `tasks/get`. `ce handoff --id <id> --workspace <file>` shows the same retained result on the host.

## Decision points

Feedback the room should keep is `complexity-eater-review`. The handoff summary can ask for that feedback. Completing the handoff does not approve a proposal and does not change the markdown.

A handoff is not proof that the recipient finished. `handoff_complete` is the recipient's record. `handoff_approve` is a separate decision and requires `approve`.

## Success verification

`handoff_get` shows the status and, after completion, the result text. `handoff_events` lists the retained events. A second complete does not change the result text. The markdown version is unchanged unless someone also committed a proposal.

## Recovery

After a disconnect, call `handoff_get`, `tasks/get`, or `ce task --id <id> --participant <id>`. `handoff_events` is the recovery log. `subscriptions/listen` is a live hint and does not replay what you missed. A notification is a hint. Terminal task rows expire 24 hours after their last update. The handoff keeps the result text. Restarting the room does not drop a completed result.

## Security

Do not put an invitation, a session id, or a private transcript into the summary or the result. A task id does not authorize a later call. `tasks/cancel` stops work that is not already terminal. The participant must own the task or be assigned.

## Fallback

Without the tasks extension, `handoff_create` returns the handoff in the tool result. Poll with `handoff_get`. Offline handoff packages are not implemented. There is no import of another product's chat history.
