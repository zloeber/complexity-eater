# Migration report

Complexity Eater is the Commonroom product in Rust. The Python repository was not modified. This binary is the implementation to build and run.

## What was compared

The reference behavior was taken from the Python modules `engine`, `http`, `mcp`, `cli`, `serving`, `dialer`, `client`, `gui`, and `transport`, and from their tests. The Rust suite re-executes those scenarios against `ce` rather than shelling out to Python. CI does not install Python.

`cargo test --workspace --locked` is the parity run. It passed: 4 protocol tests, 9 engine and markdown tests, 7 transport tests, 3 HTTP tests, 4 CLI library tests, and 7 CLI end-to-end tests. `cargo clippy --all-targets -- -D warnings` is clean.

A browser opened the owner page from `ce serve`, saw the workspace, the host, the outline, and Approve/Reject after an agent proposal. The button calls the same review route the HTTP tests already exercise.

## Verified equivalent

* Workspace create, edit, restart, and external file edits.
* Leases, proposal review, commit, and stale-version conflicts.
* Invitation hashing, expiry, revocation, single use, and legacy plaintext redemption.
* Loopback dialing, including a payload of 200 KiB and a connect timeout.
* Tailcat failure modes: missing binary, exit during startup, forward that never opens a port.
* Human cookie session, public discovery, agent denial of raw commit and review.
* MCP discovery, capability-filtered tools, and the absence of `/mcp` on the owner port.
* `ce join`, `ce call`, and `ce mcp` against a live room, including session reuse and a rejected second join.
* Redaction of transport addresses and invitation secrets from status, history, banners, and discovery.

## Exceptions

| Item | Impact | Resolution |
| --- | --- | --- |
| Two-machine Tailcat | Remote encrypted rooms are unproven beyond the subprocess failure tests | Keep the Python acceptance note. Run it on two machines before calling remote transport done |
| MCP header mismatch, 413 bodies, rate limit | Code matches the reference; a regression would not fail CI | Add denial tests before a release tag |
| Implicit `ce <file>` | Same function as `ce serve`, not separately asserted | Covered by reading `cli::run` |
| Skill byte-for-byte digest vs the Python file | The skill text was updated so commands say `ce` | Intentional. URI and skill name stayed `commonroom` |
| Malformed JSON status | 400 `invalid_request` instead of a framework 422 | Recorded in `decisions.md` |
| Hosted service, billing, tenancy | Not in the reference product | Roadmap only |

## Cutover

Rust is the implementation in this repository. The Python tree remains the historical reference until someone who runs both declares the Tailcat exception closed and archives that repo. Do not delete Python workspace files. Point agents at `ce` using the paste block a room already prints.
