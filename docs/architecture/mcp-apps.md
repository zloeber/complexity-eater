---
title: MCP Apps review
---

`review_open` is the first MCP Apps view. It lets a compatible host show the document, an agent note, and a draft response. Text clients receive the same short brief and keep using the existing tools.

The extension is `io.modelcontextprotocol/ui`. This server accepts MCP `2026-07-28` only and does not implement `initialize`. A host declares the extension on the request, under `_meta["io.modelcontextprotocol/clientCapabilities"].extensions`, the same place it declares tasks. The extension object must include `mimeTypes` containing `text/html;profile=mcp-app`. A missing declaration, or a declaration without that MIME type, leaves `review_open` as an ordinary tool with no `_meta.ui`.

When the declaration matches, `review_open` carries `_meta.ui.resourceUri` of `ui://commonroom/review/1`. The host reads that URI with `resources/read`. The template is static HTML embedded in the binary. It is not listed in `resources/list` and it is not a subscribable workspace resource, so the live catalog does not change. `ttlMs` is 0. The URI suffix is the revision: a new template uses a new URI. The resource declares `prefersBorder: true` and no CSP domains, so the host keeps the restrictive default. The iframe does not dial the room. It talks to the host, and the host proxies `tools/call`.

`review_open` text is a version, section titles, open-question count, and pending-proposal count. With the extension, `structuredContent` adds ids for sections, questions, proposals, and an optional handoff. The document body is not in that result. `review_section` is marked `visibility: ["app"]`. Its model-visible text is a pointer. The bytes are in `structuredContent`. A host that ignores visibility can still call it, and the call still requires `read`. Visibility is not authorization.

The view keeps three regions. The document comes from `review_section`. The agent note is the tool argument `note`, shown beside the document and not written anywhere. Answers, comments, and clarification questions are drafts until a submit button calls `review_answer`, `review_comment`, or `review_question_create`. Those calls use the existing engine methods. The view has no commit button and no handoff approval. A missing `handoff_id` does not block the document review. A stale approved proposal still returns `stale_version` from `workspace_commit` and does not replace the file.

The owner page and `ce ui` are unchanged. This template is not the management page, and a tunnel does not publish it.
