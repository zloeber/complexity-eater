# Migration decisions

The Python Commonroom tree is the behavioral reference. These are the places the Rust product differs, and the places it deliberately does not.

## Names

| Surface | Value | Why |
| --- | --- | --- |
| Product title, banner, paste text | Complexity Eater | The new product name |
| CLI | `ce` | The new command name, including the agent paste block and the skill's commands |
| Invitation scheme | `commonroom://join/` | Existing invitations must still decode |
| Protocol name and discovery `name` | `commonroom` / `Commonroom` | Clients and MCP server info already match this |
| Cookie | `commonroom_session` | Browsers and tests already use this name |
| Database suffix | `.commonroom.db` | Existing workspace sidecars open without a migration |
| Skill URI and frontmatter name | `skill://commonroom/SKILL.md`, `commonroom` | Skill catalogues already use this URI |
| Dialer state directory | `$XDG_STATE_HOME/commonroom/sessions` or `~/.local/state/commonroom/sessions` | A restarted agent must find the session the Python dialer wrote |
| HTTP `Host` on the byte client | `commonroom` | Matches the reference client |
| Page theme key | `ce-theme` | User-facing localStorage. Not a wire field |

`CE_WORKSPACE` is accepted ahead of `COMMONROOM_WORKSPACE`. `CE_TAILCAT` is accepted ahead of `COMMONROOM_TAILCAT`. `CE_SKILL` is accepted ahead of `COMMONROOM_SKILL`. The older variables still work.

## Behavior kept

* SQLite schema, including `participants.left_at`, `invites.capabilities`, `invites.invite_id`, `invites.token_hash`, and `sessions.invite_id`.
* Invitation secrets stored as sha256 hex. The secret is returned once, inside the URI.
* Legacy rows with a null `token_hash` are hashed on redemption.
* Version numbers only increase. A stale commit returns `stale_version`, `current_version`, and a diff. The proposal is marked `conflicted`.
* External edits to the markdown file become the next version.
* Default agent capabilities exclude `approve`, `invite`, and `admin`. `admin` and `*` cover every capability.
* Session ids are `ses_` plus url-safe random data. Other ids are `prefix_` plus 16 hex characters.
* Timestamps use `+00:00` with microsecond precision so SQL text comparisons of expiry stay ordered.
* Invite expiry is `< now`. Session and lease expiry are `<= now`.
* Diagnostics omit `address`, `secret`, `token`, `base_url`, and `pid`. Events do not record invitation secrets or session ids.
* A dead transport pid makes `current_transport` return nothing, and the next invitation falls back to a local hint.
* Connect failure does not try the next invitation hint and does not spend a use. A missing plugin is skipped.
* The dialer reuses a stored session when `GET /workspace/status` returns 200 for the same workspace. If the session is dead it deletes the file and stops. It does not join again.

## Small, explicit differences

* Malformed JSON is `invalid_request` with HTTP 400. The Python stack sometimes returned a framework 422.
* Stored JSON blobs are compact. Readers parse JSON, so spacing is not significant. Proposal size checks are therefore slightly looser than Python's spaced encoding.
* `ce serve --port 0` binds an ephemeral owner port and prints that port. The reference server expected a concrete port.
* Tailcat is linked into this binary through `ce-transport`. The domain crates still do not name it. Selecting it remains a CLI flag.
* Engine methods take a `session` argument. HTTP and MCP always pass it. In-process calls without a session are trusted, matching the reference engine.

## Not claimed

A two-machine Tailcat acceptance test was not part of the Python automated suite (`docs/remote-acceptance.md` there) and is not part of this one. Failure modes of a missing binary, a process that exits after printing an address, and a forward that never opens a local port are tested.
