# Personal-agent handoff demo

A manager pastes markdown their agent already wrote. An employee opens a one-use invitation. The employee's simulated agent turns that document into work, questions, and a draft reply. The reply goes back through the same handoff. The original markdown stays sealed.

The running demo is one Cloudflare Worker:

https://complexity-eater-handoff.zloeber.workers.dev

It does not replace `ce serve`. Room approval, leases, and the markdown sidecar stay in the Rust engine.

## What is live

| Piece | Running demo |
| --- | --- |
| Coordination | One Worker and one Durable Object. Sessions and invitations are checked there. |
| Documents | The sealed original and each later revision live in that Durable Object. |
| Agents | Deterministic demo profiles. Output is marked simulated. No model provider is called. |
| Artifacts | Not enabled on this Cloudflare account. The git path is in the Worker and stays off until the binding deploys. |

Deploying the Artifacts binding returned API `10403`: a paid Workers plan is required. `wrangler artifacts namespaces list` returned `10004` Access denied. The page says Durable Object storage. It does not say the documents are Artifacts commits.

## Prerequisites

- Node.js 22 or newer
- A Cloudflare account logged in with `npx wrangler login`
- Workers permission to deploy a Durable Object

The Rust toolchain is not required for this demo.

## Run the tests

```sh
cd demo/handoff
npm install
npm test
```

`npm test` covers profile interpretation, invitation and session checks, rejection of credentials in the query string, and the rule that a later revision cannot replace `original.md`.

## Deploy

```sh
cd demo/handoff
npx wrangler login
npx wrangler deploy
```

The workers.dev URL is printed at the end of the deploy. API calls from `curl` or Python need a browser `User-Agent`. Cloudflare answers the default client with error `1010`.

Check the live service:

```sh
curl -sS -A "Mozilla/5.0" https://complexity-eater-handoff.zloeber.workers.dev/api/health
python3 scripts/verify-round-trip.py
```

`verify-round-trip.py` creates a handoff, rejects an anonymous read, spends an invitation, previews a second profile without storing it, sends a response, and checks that the original text is unchanged.

## Clean start

Open the workers.dev URL. Leave the sender as Avery Chen and the recipient as Jordan Hale. The sample proposal is already in the box. Choose **Create handoff**. That is a new room. **New handoff** starts another one. Closing the browser drops the session tokens, which live in `sessionStorage`.

To delete every handoff stored in the Durable Object, set a secret and call the reset route. The secret is not in the repository.

```sh
printf '%s' "$DEMO_RESET_TOKEN" | npx wrangler secret put DEMO_RESET_TOKEN
curl -sS -A "Mozilla/5.0" -X POST \
  -H "X-Demo-Reset: $DEMO_RESET_TOKEN" \
  https://complexity-eater-handoff.zloeber.workers.dev/api/demo/reset
```

Without the secret, that route is `404`.

## Turn Artifacts on

When the account can use Artifacts, uncomment the `[[artifacts]]` block in `wrangler.toml` and deploy again. The namespace is `complexity-eater`. Each handoff then becomes one repository. The Worker creates it, pushes the original with `isomorphic-git`, and later commits the interpretation, questions, response, and acknowledgement. Git tokens stay on the Worker. The health check reports `persistence: cloudflare-artifacts` only after `ARTIFACTS.list` succeeds.

## Walkthrough

The five-minute script is [DEMO.md](DEMO.md). How the Worker sits next to the Rust room is [ARCHITECTURE.md](ARCHITECTURE.md).
