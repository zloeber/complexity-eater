---
title: Review canvas
---

A room is the review session. The markdown file stays the source. Questions, answers, comments, and the review status live in the sidecar. A proposed edit is still a proposal: it does not change the file until someone with `approve` accepts it and someone with `commit` lands it.

The human page is that review. It is a projection of `/workspace/room`. Agents use the same engine through HTTP or MCP. The page does not invent a second write path, and it does not require a Git repository.

## Open it

```sh
ce examples/review.md --name Ada
```

Open the printed `Human:` URL. The source is on the left. Questions and proposed changes are on the upper right. The response for the selected question is on the lower right. On a narrow window the same three panes are tabs: Document, Questions, and Respond.

An empty room has no questions yet. The host can ask one from the page, or an agent can call `review_question_create`. A default invitation can ask, answer, and comment. It cannot approve a proposal or complete the review.

## Contract

A question has `question_id`, `document_id` (the workspace id), `kind`, `prompt`, `options`, `required`, `status`, `anchor`, `created_by`, and `responses`. The anchor stores the region path, the quote, a short prefix and suffix, and the document version it was created against.

`kind` is one of `text`, `single`, `multi`, `yes_no`, and `approve_reject`. `single` and `multi` require two to eight unique options. The other kinds reject options. Unknown fields are `invalid_request`. The engine does not accept HTML, scripts, or any other UI definition.

Status is `open`, `answered`, `needs_clarification`, `resolved`, or `rejected`. An answer appends a response and sets `answered`. Earlier responses stay on the question. `resolved` and `rejected` require `approve`. `approve_reject` records a decision on that question. It does not approve a document proposal.

Creating a question requires the quote to occur once in the named region. A later edit that keeps that quote in the region stays `resolved`. A quote that still occurs once somewhere else is `moved`, and `resolved_region` names the heading that now contains it. A missing quote is `stale`. More than one match is `ambiguous`. Stale and ambiguous anchors are not attached to a different passage.

`review_context` returns the board, the latest answer for each answered question, the passage when the anchor still resolves, and the proposals. `review_export` with `format` `json` or `markdown` returns a body and does not write the file.

The review itself is `open`, `complete`, or `returned`. `complete` and `returned` require `approve` and block new questions, answers, and comments. Reopening requires `admin`.

## Capabilities

| Action | Capability |
| --- | --- |
| Read the board, the passage, and an export | `read` |
| Create or update a question | `propose` |
| Answer | `answer`, or `propose` |
| Comment | `comment`, or `propose` |
| Close or reopen a question | `approve` to resolve, reject, or reopen a closed question |
| Complete or return the review | `approve` |
| Reopen the review | `admin` |
| Accept a document proposal | `approve`, then `commit` to land it |

`answer` and `comment` can be granted without `propose`. A session with only those capabilities can respond and cannot create questions or document proposals. A default invitation is unchanged: `read`, `observe`, `propose`, `checkout`, and `commit`. `propose` covers asking, answering, and commenting so that invitation can take part. It still cannot approve.

## HTTP and MCP

| HTTP | MCP tool |
| --- | --- |
| `GET /workspace/review` | `review_context` |
| `GET /workspace/review/export?format=json` | `review_export` |
| `POST /workspace/review/questions` | `review_question_create` |
| `POST /workspace/review/questions/update` | `review_question_update` |
| `POST /workspace/review/answer` | `review_answer` |
| `POST /workspace/review/comment` | `review_comment` |
| `POST /workspace/review/question/status` | `review_question_status` |
| `POST /workspace/review/status` | `review_status` |

MCP tools call the same engine methods. `review_answer` is listed for a session with `answer` or `propose`. `review_comment` is listed for `comment` or `propose`. A missing question is `question_not_found`. A quote that is not in the region, a bad kind, or a value that is not one of the options is `invalid_request`. A read-only session that posts an answer is `permission_denied`.

Subscribe to `commonroom://workspace/review` when the session can `read`. The notification is a hint to read the resource again. It does not include the answer or the document. After a disconnect, call `review_context`.

## What this page does not do

The reading view covers headings, lists, tables, fenced code, quotes, links, and emphasis. The source view is the original markdown. Editing the file still goes through a proposal, or through the baton when the room is in baton mode. Those controls are under Room.

The hosted handoff at `demo/handoff` is a separate Worker for a pasted document between two demo profiles. It is not this canvas, and it does not share the sidecar. Mock profiles there are simulated.

The page is compiled into `ce`. It is not a second frontend, and it does not load a block editor or a CRDT.
