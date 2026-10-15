---
title: Personal handoff
---

The personal-agent handoff is a demo Worker beside the room. It is not a hosted `ce serve`, and it does not move approval into Git.

A sender pastes markdown their own agent already produced. The Worker gives the recipient a one-use invitation. A simulated recipient agent reads only that markdown, using a demo profile, and returns a summary, the work, the open questions, and a draft reply. The original file is stored once. Later revisions are questions, the response, and the sender's acknowledgement.

The running service is one Worker and one Durable Object:

https://complexity-eater-handoff.zloeber.workers.dev

Sessions and invitations are checked in the Worker. On this Cloudflare account the documents live in the Durable Object. Cloudflare Artifacts is the intended history store: one repository per handoff, written with `isomorphic-git`, with git tokens kept on the Worker. Deploying that binding returned API `10403` because the account is on the Workers free plan. The Artifacts API returned `10004`. The page reports Durable Object storage until the binding deploys.

The walkthrough, deploy steps, and the limit on what the Worker will claim are in `demo/handoff/`.

`ce serve` still binds loopback. The engine remains the only writer of a room's markdown file and sidecar. Default invitations still omit `approve`. The demo profiles are not room participants, and a handoff id is not a bearer token.
