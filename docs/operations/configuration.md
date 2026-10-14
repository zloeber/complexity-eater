---
title: Configuration
---

`ce` does not require a config file. Flags and environment variables are enough for a local room.

## Workspace

The substrate path is the positional argument to `ce serve`, or `--workspace` on the local commands. The default file name is `workspace.md`.

`CE_WORKSPACE` overrides that default. If it is unset, `COMMONROOM_WORKSPACE` is used.

The metadata database is `<substrate>.commonroom.db` next to the file, unless `--ephemeral` is set. Ephemeral mode creates a temporary directory, stores the database there, and deletes that directory on exit. The markdown file is not deleted.

`--ephemeral` is the way to try a room without leaving a sidecar behind.

## Process

| Flag | Meaning |
| --- | --- |
| `--host` | `127.0.0.1` and `localhost` bind `127.0.0.1`. `::1` binds `[::1]`. Anything else is refused. |
| `--port` | Owner page port. `0` chooses an ephemeral port and prints it. |
| `--name` | Host participant name. Defaults to `USER` or `USERNAME`, then `host`. |
| `--tailcat` | Publish the ingress with the Tailcat plugin. |
| `--transport` | Plugin name. Wins over `--tailcat` when both are set. |
| `--json` | Structured output for commands that support it. |
| `--ephemeral` | Temporary metadata database. |
| `--mode` | `review` (default) or `baton`. Baton lets the holder write without approval. |
| `--baton-max-minutes` | Cap on one grant or renewal. Default 15. Zero is refused. |
| `--baton-default-minutes` | Grant when a request omits a duration. Must be at most the max. |

`CE_TAILCAT` and `COMMONROOM_TAILCAT` select the Tailcat executable. The default name is `tailcat` on `PATH`.

`CE_SKILL` and `COMMONROOM_SKILL` point at a skill markdown file. Otherwise `ce` looks beside the executable, then `skills/commonroom/SKILL.md` in the working directory, then the text compiled into the binary.

`RUST_LOG` controls tracing. The default filter is `warn`. Lines go to stderr so they do not corrupt MCP stdout.

The container image sets `HOME`, `TMPDIR`, `CE_WORKSPACE`, and the `XDG_*` directories under `/var/lib/ce`. That layout is in the [container page](container.md).

## Platform directories

These are for a future config file and for local data that is not a workspace sidecar. The workspace database does not move into them.

| OS | Config | Data |
| --- | --- | --- |
| macOS | `~/Library/Application Support/ce` | `~/Library/Application Support/ce` |
| Linux | `$XDG_CONFIG_HOME/ce` or `~/.config/ce` | `$XDG_DATA_HOME/ce` or `~/.local/share/ce` |
| Windows | `%APPDATA%/ce` | `%LOCALAPPDATA%/ce` |

The dialer does not use those directories. It stores sessions at `$XDG_STATE_HOME/commonroom/sessions/<sha256 of the invitation secret>` or `~/.local/state/commonroom/sessions/...`. The directory is mode `0700` and the file is mode `0600` on Unix.

## Limits

Request body 1 MiB. Proposal 256 KiB. Handoff result 256 KiB. 32 active sessions. 32 active tasks (`working` or `input_required`). Task TTL 24 hours. 2000 events retained. Lease default 15 minutes, maximum 120. Invitation default 60 minutes, maximum 7 days. Session default 8 hours, maximum 24 hours. Authenticated rate limit 600 requests per minute. List size 1 to 200. `tasks/get` suggests a 5 second poll. Polling faster than that is still subject to the room rate limit. The listen stream does not add a second quota.
