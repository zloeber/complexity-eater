---
name: architecture
description: Major components and how the room, engine, transport, and HTTP/MCP surfaces connect.
edges:
  - target: ../ROUTER.md
    condition: when starting a session or checking project navigation
  - target: stack.md
    condition: when verifying runtime and dependency choices
  - target: conventions.md
    condition: when validating project structure or review expectations
  - target: ../patterns/handoff-tasks.md
    condition: when changing handoffs, MCP tasks, or listen subscriptions
  - target: ../patterns/personal-handoff-demo.md
    condition: when changing the manager-to-employee handoff demo, its Worker, or its Artifacts path
  - target: ../patterns/public-website.md
    condition: when changing the public website, the agent entry, or the room-launch contract
  - target: ../patterns/proposal-lifecycle.md
    condition: when changing how a proposal replaces a region or rebases
  - target: ../patterns/room-mode.md
    condition: when changing review or baton mode, or the owner session gate
  - target: ../patterns/review-canvas.md
    condition: when changing anchored questions, answers, or the human review page
  - target: ../patterns/mcp-apps.md
    condition: when changing the MCP Apps review view or ui:// resources
  - target: ../patterns/context-drop.md
    condition: when changing a context drop, the pickup window, or drop close
last_updated: 2026-10-17
---

# Architecture

## Components
- `ce-cli` owns the room lifecycle and CLI entrypoints (`ce serve`, `ce ui`, `ce drop`, `ce join`, `ce mcp`, `ce invite`, etc.). `ce ui` is a loopback management page. It stores connectivity preferences beside the user config and does not write the markdown file. `ce drop` is a parent watch that spawns the same executable as the room child.
- `ce-core` owns the `WorkspaceEngine`, SQLite sidecar schema, permissions, proposals, leases, and markdown region logic.
- `ce-http` exposes the human page, session ingress, MCP endpoint, and the HTTP routes consumed by the CLI and agents.
- `ce-transport` implements loopback and optional Tailcat transports and the connection abstraction used when sending frames between peers.
- `ce-protocol` defines the shared error contracts, capability model, invitation encoding, and protocol documents.

## Primary data flow
1. The room is opened around a markdown file and a SQLite metadata sidecar named `<file>.commonroom.db`.
2. `ce-cli` creates or joins a workspace and calls into `WorkspaceEngine` for reads, writes, proposals, and approvals.
3. HTTP handlers in `ce-http` authenticate a bearer token or `commonroom_session` cookie, then delegate to the same engine methods used by MCP.
4. Proposals are stored in SQLite with a base version, then reviewed and committed only after authorization. Commit splices `change.content` into the leased region. A whole-document write requires `change.mode` of `document` or a target of `workspace` or `*`. If the base version moved and the proposal's region does not overlap what changed, the same replacement is applied to the current file. Overlap marks the proposal conflicted.
5. Successful mutations append an event in the sidecar and fan out to browser SSE and MCP subscribers. Handoff and task writes commit the event in the same SQLite transaction. A listen stream may drop; clients recover with `tasks/get` or the handoff record.
6. The markdown file remains the authoritative artifact; external edits are treated as newer versions and can conflict with stale proposals.

## External boundaries
- The owner port serves the human page. It accepts only a loopback `Host`. The printed Human URL carries a one-time token; exchanging it sets `commonroom_session` and redirects to a clean URL. Mutating owner routes reject a foreign `Origin` and `Sec-Fetch-Site: cross-site`.
- Regions are CommonMark ATX headings. A fenced code block and a hash that is not a heading are not regions. Replacing a region is a byte splice of that heading's span.
- Room mode is `review` or `baton`, stored in sidecar metadata. Review is the approval path. Baton grants one holder a time-boxed direct write. `baton_max_renewals`, when set, stops that holder from extending the same grant. Switching back to review revokes the holder. The owner page shows the mode, a proposal diff, and recent baton commits with a revert that appends a new version.
- `ce-cli` loads room settings from TOML before it calls the engine. The engine stores the resolved mode and baton limits. It does not parse the file.
- The session ingress is separate and accepts bearer tokens; `POST /mcp` is mounted only there.
- Invitations are encoded with the `commonroom://join/` scheme and may point to loopback or a discovered transport.
- A transport plugin is optional; a missing plugin surfaces as `transport_unavailable` rather than a public listener.
- `demo/handoff` is one Cloudflare Worker and one Durable Object for a pasted markdown handoff between two demo profiles. It checks sessions itself and stores the sealed original plus later revisions. It does not write the room markdown file. Artifacts commits are used only when that binding is deployed. It does not store review questions.
- Review questions, responses, and comments are sidecar rows. The human page reads them from `/workspace/room`. An answer does not write the markdown. Proposed edits remain proposals. See [the review canvas pattern](../patterns/review-canvas.md).
- `review_open` serves the same review through MCP Apps when the client declares `io.modelcontextprotocol/ui`. The template is `ui://commonroom/review/1` inside `ce-http`. Text clients get a short brief. See [the MCP Apps pattern](../patterns/mcp-apps.md).
- `ce drop` holds a local watch until the first participant `workspace_read` or `review_section`, or until the window ends with no read. After `picked_up` the child stays up. The deadline and the pickup count only stop new pickups. Close is `drop_close` or `POST /workspace/drop/close`, and only for the host or a session that already picked up. See [the context drop pattern](../patterns/context-drop.md).
- The public website in `website/` is static and separate. It prepares local `ce` commands from room-launch contract version 1, copies the commonroom skills (including the context drop skill) and the `workspace_onboarding` prompt from `skills/commonroom/`, and does not upload a workspace, start a drop, or provision a hosted room. `https://complexityeater.com/agent` opens with a scenario index and is the markdown procedure an agent follows to install `ce`, optionally install `tailcat`, and save the skills it uses to open or join a room.

## Important invariants
- Authorization is derived from identity, not from JSON body values.
- The engine is a single mutex-backed writer for the workspace state and markdown file.
- A drop pickup is a document read. Join, summary, and the owner page do not pick up. Closing the pickup window does not stop a session that already picked up. Answering is not approval.
- Core crates stay vendor-neutral.
