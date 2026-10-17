| Pattern | Use when |
|---------|----------|
| [room-bootstrap](./room-bootstrap.md) | Starting a room, opening a markdown substrate, and confirming the owner/session split is behaving as expected. |
| [proposal-lifecycle](./proposal-lifecycle.md) | Creating, reviewing, approving, or committing a proposal against the current workspace version. |
| [room-mode](./room-mode.md) | Switching a room between review and baton, or checking the owner session gate. |
| [mcp-dialer](./mcp-dialer.md) | Running the MCP client, verifying the session ingress, or debugging `ce mcp` JSON-RPC behavior. |
| [transport-selection](./transport-selection.md) | Selecting loopback vs. Tailcat, handling `transport_unavailable`, or diagnosing invitation dialing issues. |
| [release-check](./release-check.md) | Preparing a release, verifying binary output, or checking that the workspace is build-ready. |
| [supply-chain](./supply-chain.md) | Changing the toolchain pin, dependency policy, container profile, scanners, SBOMs, or release attestations. |
| [workspace-events](./workspace-events.md) | Watching live room changes through SSE or MCP subscriptions, handling resyncs, and debugging notification fanout. |
| [handoff-tasks](./handoff-tasks.md) | Creating a handoff, polling or subscribing to its task, and recovering the result after a disconnect or restart. |
| [review-canvas](./review-canvas.md) | Asking an anchored review question, saving an answer, and reading it back without changing the markdown file. |
| [personal-handoff-demo](./personal-handoff-demo.md) | Changing the manager-to-employee markdown handoff Worker, its mock profiles, or its Artifacts path. |
| [public-website](./public-website.md) | Changing the static public site, its Cloudflare deploy, or the room-launch contract. |
| [ui-connectivity](./ui-connectivity.md) | Opening `ce ui`, configuring a tunnel, or separating reachability from workspace authorization. |
