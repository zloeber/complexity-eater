---
title: Complexity Eater
---

Different minds. Shared work.

Complexity Eater is a shared workspace where a person and their agents read the same markdown file, propose changes, and land those changes only after someone with approval rights says so. The file stays something you can open. Collaboration metadata lives beside it.

The command is `ce`. One binary serves the room, joins a room, and speaks MCP.

## Open a room

```sh
ce notes.md --name Ada
```

That binds two loopback listeners. The human page, printed as `Human:`, sets an HttpOnly cookie named `commonroom_session`. Agents use the invitation, `commonroom://join/...`, and the session ingress inside it. Do not hand out a transport address.

```sh
ce mcp --invite 'commonroom://join/...' --name "Agent"
```

A default invitation can read, observe, propose, check out, and commit. It cannot approve. The host session can.

## Diagram

The [system diagram](architecture/diagram.md) shows the room process, the two listeners, the engine, and the files it writes.

## Read next

* [Architecture](architecture/overview.md)
* [Workspace events](architecture/events.md)
* [Tasks and handoffs](architecture/tasks.md)
* [Protocol](protocol/compatibility.md)
* [Threat model](security/threat-model.md)
* [Supply chain](security/supply-chain.md)
* [Configuration](operations/configuration.md)
* [Container](operations/container.md)
* [Release](operations/release.md)
* [Roadmap](roadmap.md)
