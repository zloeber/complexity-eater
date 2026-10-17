---
name: mcp-apps
description: Serve the review view to an MCP Apps host without a second writer.
triggers:
  - change review_open, review_section, or the ui:// template
  - negotiate io.modelcontextprotocol/ui
  - a host iframe should show the document beside an agent note
edges:
  - target: ../context/architecture.md
    condition: when the host, MCP adapter, and engine boundary is unclear
  - target: review-canvas.md
    condition: when the follow-up is a question, answer, or comment
  - target: handoff-tasks.md
    condition: when a handoff id is attached to the brief
  - target: INDEX.md
    condition: when this pattern needs to be updated or cross-referenced
last_updated: 2026-10-17
---

# MCP Apps review

## Context
The room stays the writer. `review_open` is a read tool. A host that declares `io.modelcontextprotocol/ui` with MIME type `text/html;profile=mcp-app` on `_meta["io.modelcontextprotocol/clientCapabilities"].extensions` receives `_meta.ui.resourceUri` of `ui://commonroom/review/1`. Every other client receives the text brief and no UI metadata.

## Steps
1. Keep the declaration on the request. This server does not implement `initialize`.
2. Serve the template from `resources/read`. Leave it out of `resources/list` and out of the subscribable catalog.
3. Put section titles and ids in the brief. Put document bytes in `review_section`'s `structuredContent`.
4. Mark `review_section` with `visibility: ["app"]` and still require `read`.
5. Submit through `review_answer`, `review_comment`, or `review_question_create`. Do not add a commit control to the template.

## Gotchas
- An extension object without the HTML MIME type is text-only.
- `note` is displayed by the view from the tool arguments. The server does not store it.
- A missing handoff id does not fail the brief.
- The URI suffix is the cache revision. Bump it when the HTML changes.
- The template must not name an external origin or write the document with `innerHTML`.

## Verify
- `cargo test --locked -p ce-http -- review_app_is_optional_and_does_not_write_the_file`
