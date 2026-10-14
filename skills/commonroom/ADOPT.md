---
name: complexity-eater-adopt
description: First-time adoption of Complexity Eater on the host machine. Install ce, open a local room, and hand a joining agent the commonroom skill. This does not create a hosted room.
---

# Adopt Complexity Eater

You are helping a person use Complexity Eater for the first time. You are on the machine that will host the room. The public website does not create a room, and you must not claim that it did.

The participation rules are a different document: the `commonroom` skill. Read that before you edit the shared markdown file. This file is only the first-run setup.

## Install

Use a published archive from the install page, or build `target/release/ce` with `task release`. The toolchain is pinned in `rust-toolchain.toml`. `ce --version` prints the package version.

## Open the room

```bash
ce notes.md --name "Host"
```

That is the same as `ce serve notes.md --name "Host"`. The process binds `127.0.0.1` twice. The human page, printed as `Human:`, sets the cookie `commonroom_session`. Give the joining agent the `Invite:` line, not that URL.

`--ephemeral` deletes the temporary sidecar directory when the process exits. The markdown file stays. `--tailcat` publishes the session ingress only when the `tailcat` program is installed. If it is missing, `ce` exits with `transport_unavailable` and does not open a public listener.

## Invite

```bash
ce invite --workspace notes.md
```

The default invitation grants `read`, `observe`, `propose`, `checkout`, and `commit`. It does not grant `approve`, `invite`, or `admin`. It expires in 60 minutes and works once. A failed dial does not spend it.

## Hand it to the other agent

Tell that agent to read the commonroom skill before editing:

- In the room, `skill://commonroom/SKILL.md`
- Before it is connected, `https://complexityeater.com/skills/commonroom/SKILL.md`

The room also has an MCP prompt named `workspace_onboarding`. The same text is `https://complexityeater.com/prompts/workspace-onboarding.txt`.

Prefer the paste block the running `ce` process prints. It contains the real invitation. Leave that MCP server running. Do not put the invitation in a public page, a query string, or a ticket.

## Approval

The host session can approve. A default agent invitation cannot. Propose changes. Approval stays with the workspace owner.
