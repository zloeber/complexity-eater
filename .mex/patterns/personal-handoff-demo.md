---
name: personal-handoff-demo
description: Deploy and verify the manager-to-employee markdown handoff Worker without turning it into a room.
triggers:
  - change the personal handoff demo
  - deploy the handoff Worker
  - enable Cloudflare Artifacts for a handoff
edges:
  - target: ../context/architecture.md
    condition: when the Worker starts to look like a second room engine
  - target: ../context/stack.md
    condition: when the Worker runtime or Artifacts dependency changes
  - target: handoff-tasks.md
    condition: when the change is an engine handoff, MCP task, or listen subscription
  - target: INDEX.md
    condition: when this pattern needs to be updated or cross-referenced
last_updated: 2026-10-15
---

# Personal handoff demo

## Context
`demo/handoff` is one Worker and one Durable Object. A sender pastes markdown. A recipient redeems a one-use invitation. A simulated profile interprets the document. The original is stored once. Engine handoffs and MCP tasks are a different record. See [handoff-tasks](./handoff-tasks.md).

## Steps
1. Edit the Worker under `demo/handoff/src` and the page under `demo/handoff/public`.
2. Run `npm test` in `demo/handoff`.
3. Deploy with `npx wrangler deploy` from that directory.
4. Run `python3 scripts/verify-round-trip.py` against the workers.dev URL. Give `curl` and Python a browser `User-Agent`, or Cloudflare returns `1010`.
5. Open the page, create a handoff, and redeem the invitation in a second browser profile.

## Gotchas
- Do not name Cloudflare in `ce-core`, `ce-http`, or `ce-protocol`.
- Do not put the invitation code or session token in a query string. The code belongs in the URL fragment, and the token belongs in `sessionStorage`.
- The Artifacts binding is commented out. This account returned `10403` on deploy and `10004` on the Artifacts API. Leave the UI saying Durable Object storage until `GET /api/health` reports `cloudflare-artifacts`.
- Uncomment `[[artifacts]]` only after a paid Workers plan can deploy it. The namespace is `complexity-eater`. Git tokens stay on the Worker.
- A later revision must not write `original.md`.

## Verify
- `npm test` covers access, the spent invitation, and the sealed original.
- `scripts/verify-round-trip.py` runs the same flow against the deployed Worker.
- The recipient page shows a simulated reading, and the sender page shows the return without changing the original text.
