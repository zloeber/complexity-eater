---
name: complexity-eater-review
description: Use when someone asks you to review the changes, say what needs my approval, answer what do you think, accept this proposal, give feedback, review this passage, or show the review UI or review interface. Comments and answers do not edit the shared markdown, and approval is separate from committing. Not for joining a room, hosting one, or handing work to another agent.
---

# Review a Complexity Eater room

Questions, answers, and comments are sidecar rows. The markdown file changes only through a proposal or a baton write, as the `commonroom` skill describes.

## When to use

Use this for feedback, a second read, an answer, or the shared review view. Examples: "Review the changes and tell me what needs my approval.", "What do you think of this proposal?", "Please accept this proposal.", "Can you show me the review UI?"

## When not to use

- Joining an invitation or editing a region is `commonroom`.
- Opening the room is `complexity-eater-adopt`.
- Asking another agent to go do the work is `complexity-eater-handoff`. Leaving the file before a room exists is `complexity-eater-drop`. Use this skill for the feedback that should be recorded.
- "Summarize this document" with no room and no review request is ordinary reading. Do not open a room for it.
- Installing `ce` is `complexity-eater-agent`.

## Prerequisites

A running room and a session. `read` to inspect. `propose` to create a question, answer, or comment. A default invitation includes `propose`, and `propose` also covers `answer` and `comment`. `approve` to accept or reject a proposal, to close a question, or to mark the review complete. A default invitation does not include `approve`. The host can approve from the owner page.

## Shortest successful path

1. Read `review_context` or `GET /workspace/review`.
2. If the person wants the review UI and the client declared `io.modelcontextprotocol/ui` with MIME type `text/html;profile=mcp-app` on the request, call `review_open`. The host then reads `ui://commonroom/review/1`.
3. Otherwise use the text tools: `review_question_create`, `review_answer`, and `review_comment`.
4. A document change is `workspace_propose`. Wait until it is approved. Commit later with `workspace_commit` and `proposal_id`.

## Decision points

Answering a question is not approving a proposal. `review_answer` records a response. `approve_reject` decides that question only. It does not approve a document change.

Reviewing a proposal is not committing it. `workspace_review` with `approve` set requires the `approve` capability. "Please accept this proposal" is that call, and only when the person named the proposal and the session can approve. If it cannot, say so and leave the proposal open.

"What do you think" is feedback. Answer or comment. Do not approve and do not commit.

A comment or an answer does not change the markdown.

## Success verification

Read `review_context` again. A saved answer includes `question_id`, the value, and `source.passage` when the anchor resolves. `workspace_proposals` shows `approved` only after `workspace_review`. The document version advances only after `workspace_commit`. `review_export` returns JSON or Markdown and does not write the file.

## Recovery

`source.resolution` of `stale` or `ambiguous` was not attached to a different passage. Read the region again. After a disconnect, call `review_context`. A `commonroom://workspace/review` notification is a hint and does not contain the answer. A notification is a hint. `stale_version` means you write a new proposal. Do not retry the same body.

`review_status` of `complete` or `returned` requires `approve`. New questions and answers stop until an admin sets it back to `open`.

## Security

Do not send HTML, scripts, or any other UI field. An unknown field is `invalid_request`. The `note` on `review_open` is shown beside the document and is not stored. Do not put an invitation or a session id in it. The review view does not commit. Do not treat the view as permission to approve.

## Fallback

Without the UI extension, `review_open` is a short text brief and has no `_meta.ui`. Use `review_context` and the answer and comment tools. `review_section` is for the app view. It still requires `read`. The owner page shows the same board. `ce ui` is the management page, not this review.
