---
name: review-canvas
description: Ask an anchored question, record an answer, and read it back without changing the markdown file.
triggers:
  - add or change review questions, answers, or comments
  - the human page should show a passage and a response
  - an agent needs structured feedback for a region
edges:
  - target: ../context/architecture.md
    condition: when the engine, sidecar, or page boundary is unclear
  - target: proposal-lifecycle.md
    condition: when the follow-up is a document change rather than an answer
  - target: mcp-apps.md
    condition: when the same review is opened inside an MCP Apps host
  - target: INDEX.md
    condition: when this pattern needs to be updated or cross-referenced
last_updated: 2026-10-17
---

# Review canvas

## Context
The room is the review session. Questions, responses, and comments are sidecar rows. The markdown file changes only through the existing proposal or baton path. `propose` may also answer and comment. `answer` or `comment` alone cannot create a question or a proposal. Closing a question or completing the review requires `approve`.

## Steps
1. Create a question with `review_question_create` or `POST /workspace/review/questions`. `kind` is `text`, `single`, `multi`, `yes_no`, or `approve_reject`. `region` and `quote` are required unless `parent_id` copies the parent anchor. The quote must occur once in that region.
2. Answer with `review_answer` or `POST /workspace/review/answer`. The value has to match the kind. Each answer appends a response.
3. Read `review_context`. The latest answer includes `question_id` and `source.passage` when the anchor resolves. `stale` and `ambiguous` do not name a guessed passage.
4. A document change is `workspace_propose`. Confirm the file bytes are unchanged until commit.
5. Subscribe to `commonroom://workspace/review` only as a hint. After a disconnect, call `review_context` again.

## Gotchas
- Unknown JSON fields are rejected, including any UI or script payload.
- `approve_reject` does not approve a proposal.
- A completed or returned review rejects new questions, answers, and comments. Reopening requires `admin`.
- The human page is the owner page. The hosted handoff Worker is a different program and does not store these rows.
- Default invitations did not gain a new capability. `propose` covers answer and comment.

## Verify
- `cargo test --locked -p ce-core -p ce-http -- review`
- Restart the engine between the answer and `review_context`, and confirm the file is unchanged after `workspace_propose`.
