# Complexity Eater

Different minds. Shared work. One native binary, `ce`, serves a room around a markdown file. People and their agents read that file, propose changes, and land a change only after a participant with approval rights says so. The markdown file stays the artifact. Collaboration metadata lives beside it.

Hosted accounts, billing, tenancy, and an admin console are not implemented. Do not add unused traits for them. The roadmap says where they attach: beside the engine, not inside each handler.

## Runtime

`ce serve`, or `ce <file>`, opens a room. `start_room` refuses any bind other than loopback. It listens on two `127.0.0.1` sockets:

* Owner port. `GET /` sets `commonroom_session` (HttpOnly, SameSite=Strict, Path=/). This is the host session, and it can approve.
* Session ingress. Bearer tokens only. `POST /mcp` is mounted here and nowhere else. MCP revision `2026-07-28` is the only accepted version.

A transport plugin may publish the ingress. It does not publish the owner port. With no plugin, the invitation describes loopback. `--tailcat` or `--transport tailcat` shells out to the `tailcat` binary. A missing or crashed plugin is `transport_unavailable`, not a public listener.

`ce mcp` is a second process. It speaks JSON-RPC on stdin and stdout, dials `commonroom://join/`, and forwards HTTP/1.1 with `Host: commonroom` to the ingress.

HTTP handlers and MCP tools call the same `WorkspaceEngine` methods. The human page is a projection of `/workspace/room`. It is not a second write path. The engine is one mutex and the only writer of the markdown file and the SQLite sidecar. An external edit of the markdown file wins and becomes a new version.

Identity comes from the bearer token or the owner cookie. A JSON body cannot choose the actor. Possession of a socket or an invitation string is not membership. Joining spends one use. A failed dial does not.

The host session created by `ce serve` holds capability `*`. A default invitation grants `read`, `observe`, `propose`, `checkout`, and `commit`. It does not grant `approve`, `invite`, or `admin`. Review requires `approve`. Committing a proposal requires `commit` and status `approved`. A raw document overwrite requires `admin`. A stale base version returns `stale_version` and marks the proposal `conflicted`.

Leases default to 15 minutes and last at most 120. `--ephemeral` keeps the sidecar in a temporary directory and deletes that directory on exit. The markdown file stays where it is.

## Crates

| Crate | Owns |
| --- | --- |
| `ce-protocol` | Error codes, capabilities, limits, invitation codec, discovery and protocol documents |
| `ce-core` | Workspace engine, SQLite, markdown regions, skill loading, platform paths |
| `ce-transport` | Loopback and Tailcat plugins, the connection trait, invitation dialing |
| `ce-http` | Axum routes, the human page, the session ingress, MCP |
| `ce-cli` | The `ce` binary, the room process, the byte HTTP client, the stdio MCP dialer |

`ce-core`, `ce-http`, and `ce-protocol` must not name transport vendors. A test walks those trees and fails if they contain `tailcat`, `libp2p`, `iroh`, or `webrtc`. The CLI may name Tailcat because that is how a person selects the plugin. The transport crate implements it.

`Transport` is `listen`, `connect`, `accept`, `send`, `receive`, `close`, `describe`, `capabilities`, and `expose` when a plugin publishes an already-listening port. `Connection` is a byte stream. The transport does not decide authorization.

## Protocol identifiers

These strings are the wire protocol. Leave them in place:

* Protocol name `commonroom`, invitation prefix `commonroom://join/`, MCP server name `Commonroom`
* Cookie `commonroom_session`
* Sidecar suffix `.commonroom.db`
* Skill URI `skill://commonroom/SKILL.md` and `skills/commonroom/SKILL.md`
* HTTP `Host: commonroom`
* Dialer sessions under `commonroom/sessions`
* Fallback environment variables `COMMONROOM_WORKSPACE`, `COMMONROOM_TAILCAT`, and `COMMONROOM_SKILL`

User-facing docs describe Complexity Eater and these identifiers. They do not narrate a previous product. `docs/migration/` is historical and is excluded from the published site. Do not link it from the docs site or the README.

## Documentation site

GitHub Pages builds `docs/` with Jekyll (`.github/workflows/pages.yml`). The published set is the architecture notes, the system diagram, the protocol, the threat model, operations, and the roadmap.

The illustrated diagram is an Archify architecture diagram. The candidate is `.archify/architecture-complexity-eater-20261011-210511/candidate.json`. The page embeds `docs/architecture/system.html`. When the runtime shape changes, regenerate that diagram from the source and replace `system.html`. Repository links in the candidate are local-only because `origin` is SSH.

## Releases

Pushes to `main` run CI (`.github/workflows/ci.yml`). After that CI succeeds, semantic-release (`.github/workflows/release.yml`) reads conventional commit prefixes and tags `vX.Y.Z` when one applies:

| Prefix | Bump |
| --- | --- |
| `feat!:` or a `BREAKING CHANGE:` footer | major |
| `feat:` | minor |
| `fix:` or `perf:` | patch |

`docs:`, `test:`, `refactor:`, `chore:`, `ci:`, `build:`, and `style:` do not tag. Until a version tag exists, the first successful CI on `main` publishes the `Cargo.toml` workspace version through `scripts/publish-initial-release.zsh`. The release commit updates `Cargo.toml` and `Cargo.lock` through `scripts/set-workspace-version.zsh`. Use `chore(release):` for that commit so it does not tag again.

The tag build (`.github/workflows/release-binaries.yml`) produces `ce` for Linux x86_64, Linux aarch64, macOS aarch64, and macOS x86_64, then attaches the archives and `SHA256SUMS` to the GitHub release. Semantic-release creates the tag with `GITHUB_TOKEN`, which does not itself start another workflow, so the release workflow dispatches the binary workflow. A tag pushed by a person still starts that workflow directly.

## Checks

```sh
task check    # fmt, clippy, tests
task test
task e2e      # CLI join, MCP dialer, proposal commit
task release  # cargo build --release --locked --bin ce
```

CI also runs `cargo audit`. A two-machine Tailcat session is not part of the automated suite. Do not describe remote Tailcat as proven.

The toolchain is stable Rust, pinned in `rust-toolchain.toml`. Supported development platforms are macOS and Linux.
