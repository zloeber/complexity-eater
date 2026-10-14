---
title: Configuration
---

`ce` does not require a config file. A TOML file overrides the built-in defaults, and an explicit flag overrides the file.

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
| `--baton-max-renewals` | Renewals allowed on the current grant. Zero refuses renewals. Omitted means unlimited. |
| `--config` | TOML file applied after the user and workspace files. |

`CE_TAILCAT` and `COMMONROOM_TAILCAT` select the Tailcat executable. The default name is `tailcat` on `PATH`.

`CE_SKILL` and `COMMONROOM_SKILL` point at a skill markdown file. Otherwise `ce` looks beside the executable, then `skills/commonroom/SKILL.md` in the working directory, then the text compiled into the binary.

`RUST_LOG` controls tracing. The default filter is `warn`. Lines go to stderr so they do not corrupt MCP stdout.

The container image sets `HOME`, `TMPDIR`, `CE_WORKSPACE`, and the `XDG_*` directories under `/var/lib/ce`. That layout is in the [container page](container.md).

## Config file

Settings load in this order. A later layer replaces a key it sets and leaves the rest alone.

1. Built-in defaults.
2. The user file, `config.toml` in the platform config directory below, when that file exists.
3. `<substrate>.commonroom.toml` beside the markdown file, when that file exists. `notes.md` reads `notes.md.commonroom.toml`.
4. `--config path`. A missing path is an error. The automatic files are optional.
5. A flag passed on the command line. A flag left at its default does not wipe the file.

```toml
mode = "review"
host = "127.0.0.1"
port = 8000

[baton]
max_minutes = 15
default_minutes = 15
max_renewals = 2
```

`mode` is `review` or `baton`. `[baton]` is the collaboration addon. `max_renewals` counts extensions of the current grant, including a second `workspace_baton_request` from the holder. After the cap, renew returns `baton_renewal_limit` until the holder releases it or it expires. The next request is a new grant. Omit `max_renewals` for no cap. `0` refuses every renewal. An unknown key is an error, so a typo does not look like it applied.

`transport` may name a plugin. Omit it for loopback. `--transport` and `--tailcat` win over the file. `--ephemeral` stays a flag because it chooses where this process keeps the sidecar.

## Platform directories

The user config file lives in the config directory. Workspace metadata does not move into these directories.

| OS | Config | Data |
| --- | --- | --- |
| macOS | `~/Library/Application Support/ce` | `~/Library/Application Support/ce` |
| Linux | `$XDG_CONFIG_HOME/ce` or `~/.config/ce` | `$XDG_DATA_HOME/ce` or `~/.local/share/ce` |
| Windows | `%APPDATA%/ce` | `%LOCALAPPDATA%/ce` |

The dialer does not use those directories. It stores sessions at `$XDG_STATE_HOME/commonroom/sessions/<sha256 of the invitation secret>` or `~/.local/state/commonroom/sessions/...`. The directory is mode `0700` and the file is mode `0600` on Unix.

## Limits

Request body 1 MiB. Proposal 256 KiB. Handoff result 256 KiB. 32 active sessions. 32 active tasks (`working` or `input_required`). Task TTL 24 hours. 2000 events retained. Lease default 15 minutes, maximum 120. Invitation default 60 minutes, maximum 7 days. Session default 8 hours, maximum 24 hours. Authenticated rate limit 600 requests per minute. List size 1 to 200. `tasks/get` suggests a 5 second poll. Polling faster than that is still subject to the room rate limit. The listen stream does not add a second quota.
