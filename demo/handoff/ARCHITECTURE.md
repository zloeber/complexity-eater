# How the handoff demo sits next to the room

`ce serve` is still the room. One process, one markdown file, one SQLite sidecar, and the `WorkspaceEngine` as the only writer. Invitations, approval, and leases stay there. This demo does not move that work into Git and does not run a second copy of the engine on Cloudflare.

The demo is the cross-environment handoff the local room cannot serve by itself. `ce serve` binds loopback. Two browsers on two machines need a public coordinator. That coordinator is one Worker, `complexity-eater-handoff`, and one Durable Object, `HandoffCoordinator`.

## Request path

1. The sender pastes markdown. The Worker checks the body size and the two demo profiles, then stores `original.md` once.
2. A sender session token comes back in the response body and is kept in `sessionStorage`. It is not put in the URL.
3. The sender asks for an invitation. The Worker stores only the SHA-256 of the code. The browser puts the code in the URL fragment. A fragment is not sent as a query string. Redeeming it spends the invitation and returns a recipient session.
4. The recipient's first open runs the mock agent for the recipient profile and stores that JSON beside the original. A preview for another profile is computed and discarded.
5. A question, a response, and the sender's acknowledgement are later revisions. Each write refuses a patch that includes `original.md`. The stored hash of the original is checked after the write.
6. The sender's page polls until the return appears. The return is interpreted with the sender's profile, so Avery sees a decision and a date rather than Jordan's task list.

Anonymous reads are `401` whether or not the id exists. A session for a different handoff is `404`. The recipient cannot invite. The sender cannot submit the response. A query string that carries `token`, `invite`, `code`, or `markdown` is `400`.

## Persistence

The intended document store is Cloudflare Artifacts: one repository per handoff, in the namespace `complexity-eater`. The Worker would create the repo, push the original with `isomorphic-git`, and read history through the Artifacts binding. Git credentials would stay on the Worker.

This account cannot use that binding. Deploy returned `10403` (paid Workers plan required). The Artifacts REST API returned `10004`. The running Worker therefore stores the same files and revision messages in the Durable Object's SQLite. The health JSON says `persistence: cloudflare-durable-object` and `artifacts: unavailable`. When the binding is uncommented in `wrangler.toml` and deploy succeeds, those fields switch to `cloudflare-artifacts` and the git path runs. Until then the UI does not call the revisions Artifacts commits.

## What the mock agent is

Avery, Jordan, and Riley are fixed profiles: role, style, and the order in which they want the document explained. The interpreter splits the markdown on headings and lists. It does not call a provider, and it does not read anything that was not in the handoff. Every interpretation has `simulated: true` and `agent: "mock"`.

## What this is not

It is not a hosted room, an account system, or a GitHub-shaped product. Engine handoff rows and MCP tasks are unchanged. `ce-core`, `ce-http`, and `ce-protocol` do not name Cloudflare.
