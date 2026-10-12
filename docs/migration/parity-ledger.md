# Parity ledger

Source of truth: the Python tree at `github.com/zloeber/commonroom` (local checkout used during the port). Target: this repository. Status values are `verified`, `implemented`, `partial`, `planned`, or `excluded`.

`verified` means a Rust test exercises the behavior. `implemented` means the code path exists and a neighboring test covers the same module, but this row has no dedicated assertion. `partial` means a slice is missing or only the failure modes are tested. `planned` means the reference did not ship it either, or it is a future Complexity Eater deployment. `excluded` means it was a documented aspiration in the source, not a working feature, and it was not ported.

The suite that backs the `verified` rows is `cargo test --workspace`. It passed on macOS (darwin 25, aarch64) against the locked dependency graph: protocol 4, engine and markdown 9, transport 7, HTTP 3, CLI library 4, CLI parity 7.

## Summary

| ID | Capability | Status | Evidence |
| --- | --- | --- | --- |
| F-CLI | `ce` commands, help, version, exit codes | verified | `help_and_version_name_the_product`, `serve_prints_a_ready_snapshot_without_the_ingress_address` |
| F-CLI-IMPLICIT | A path as the first argument means `serve` | implemented | `cli::run` rewrites argv. No separate test |
| F-WS | Create, read, rewrite, restart a workspace | verified | `workspace_create_read_modify_and_restart` |
| F-WS-EXTERNAL | An outside edit of the markdown file wins | verified | `summary_and_changes_are_bounded_and_external_edits_win` |
| F-LEASE | Checkout, visibility, conflict, renew, release | verified | `lease_visibility_between_participants` |
| F-PROPOSE | Propose, review, commit, stale base | verified | `proposal_review_and_commit`, `stale_commit_returns_structured_conflict`, `call_joins_proposes_and_rejects_a_second_join` |
| F-INVITE | Hash, expiry, revoke, spend, legacy plaintext | verified | `invite_expired_revoked_spent_and_hashed`, `legacy_plaintext_invite_is_replaced_on_redemption` |
| F-JOIN | Remote join, second join rejected, bad URI | verified | `join_rejects_malformed_and_unsupported_invitations`, `call_joins_proposes_and_rejects_a_second_join`, `byte_client_can_join_the_ingress` |
| F-DIAL | MCP dialer joins once and reuses the session | verified | `dialer_joins_once_and_a_restart_reuses_the_session` |
| F-DIAL-FAIL | Failed dial does not spend the invitation | verified | `failed_dial_does_not_spend_the_invitation` |
| F-HTTP | Discovery, cookie host, agent denial, remove path | verified | `ingress_requires_a_session_and_discovery_is_public`, `owner_cookie_reviews_and_agent_cannot_bypass` |
| F-MCP | Discover, tool filter, owner port has no `/mcp` | verified | `mcp_discovery_hides_secrets_and_filters_tools` plus the owner 404 assertion |
| F-MCP-HEADERS | Header mismatch and version negotiation | implemented | `ce-http/src/mcp.rs`. No dedicated test |
| F-LOOP | Loopback connect, timeout, large payload | verified | `loopback_round_trip_timeout_and_reconnect`, `loopback_large_concurrent_payloads` |
| F-TAILCAT | Missing binary, early exit, failed forward | verified | `missing_binary_is_unavailable`, `exit_after_address_is_not_ready`, `forward_failure_kills_the_child` |
| F-TAILCAT-E2E | Two machines over Tailcat | partial | Not run. Same gap as the Python repo's `docs/remote-acceptance.md` |
| F-REDACT | Addresses and secrets stay out of diagnostics | verified | `diagnostics_strip_the_endpoint`, `status_hides_the_transport_address_and_history_hides_the_secret`, `banner_hides_the_endpoint_and_names_the_product` |
| F-VENDOR | Domain crates do not name transport vendors | verified | `core_crates_do_not_name_transport_vendors` |
| F-GUI | Human page and ingress page | verified | Browser loaded `http://127.0.0.1:8765/`, showed workspace `idea` at version 1, loopback without an address, host Ada, the outline, and Approve/Reject after a proposal. Ingress HTML test checks that the word `session` is absent |
| F-LIMITS | Body size, rate limit, list bounds | implemented | Enforced in `ce-http` and `ce-core`. No dedicated denial test |
| F-SKILL | Skill text and `skill://commonroom/SKILL.md` | implemented | Served by HTTP and MCP. Commands in the file say `ce` |
| F-HOSTED | Accounts, billing, tenancy, admin console | planned | Not in the Python product. See `docs/roadmap.md` |
| F-ORG | Org hub, Docker image, public profiles | excluded | Present only as later slices in the source design notes. Not a working feature |

## Protocol and stored shapes

| Item | Contract | Rust |
| --- | --- | --- |
| Invitation URI | `commonroom://join/` plus sorted JSON, max 8192 bytes | `ce-protocol` encode/decode. Test `invitation_round_trip_sorts_keys_and_keeps_the_prefix` |
| Error object | `{error, message, details}` plus recovery fields | `CommonroomError::to_value` |
| Capabilities | `read`, `observe`, `propose`, `checkout`, `commit`, `approve`, `invite`, `admin`, `*` | `star_and_admin_cover_every_capability`, `legacy_permission_names_map_to_capabilities` |
| SQLite sidecar | `<file>.commonroom.db`, WAL, the Python tables and added columns | `ce-core` schema. Opened by every engine test |
| Events | Cap 2000. No invitation secret in the payload | `summary_and_changes_are_bounded_and_external_edits_win`, history CLI test |
| MCP revision | `2026-07-28` only | `MCP_PROTOCOL_VERSION` |
| Session file | sha256(secret) under `commonroom/sessions` | `ce-cli` dialer. Reuse is asserted by the dialer test |

## Feature notes

### F-CLI

Commands: `serve`, `status`, `invite`, `join`, `call`, `participants`, `leases`, `proposals`, `history`, `diff`, `mcp`, `remove-participant`. Source: `commonroom/cli.py`. Interface: CLI. Errors print indented JSON on stdout and exit 1. Dialer errors print compact JSON on stderr. `call` exits 0 on HTTP 2xx and 1 otherwise.

### F-WS

Source: `commonroom/engine.py`. The file is the artifact. Versions only increase. Restart reloads the sidecar. Security: the path must be a file, not a directory.

### F-PROPOSE

Source: engine proposal methods and `POST /workspace/propose`, `/workspace/proposal/review`, `/workspace/commit`. An agent commits `proposal_id` only after approval. A stale base returns 409 `stale_version` and marks the proposal `conflicted`. The end-to-end test joins, proposes, reviews in-process as the host, commits through `ce call`, and checks the markdown file.

### F-HTTP

Source: `commonroom/http.py`, `commonroom/gui.py`. Owner cookie is not in the HTML. Ingress HTML does not contain the word `session`. Discovery and `/protocol` and `/skills/commonroom` are public. Workspace routes require a session.

### F-MCP

Source: `commonroom/mcp.py`. Tools are filtered by capability. A default agent sees `workspace_propose` and does not see `workspace_review`. Discovery is public and does not contain the invite token. Skill list returns `skill://commonroom/SKILL.md`.

### F-LOOP and F-TAILCAT

Source: `commonroom/transport`. Loopback is the default room transport. Tailcat shells out to the `tailcat` binary. This crate does not reimplement Tailcat cryptography.

### F-GUI

The pages are compiled into `ce-http` as `owner.html` and `ingress.html`. Title is Complexity Eater. Theme key is `ce-theme`. Paste command is `ce`. A browser session against `ce serve` rendered the room, the host, the outline, and Approve/Reject once an agent proposal existed.
