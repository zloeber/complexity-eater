# Release and upgrade

## Build

```sh
task release
```

That runs `cargo build --release --locked --bin ce`. The release profile strips symbols, uses thin LTO, and one codegen unit. The artifact is `target/release/ce`.

`ce --version` prints the workspace package version. Checksums are the operator's responsibility until a tagged GitHub release exists. A source checkout plus `Cargo.lock` is the reproducible input. System libraries that Rust links dynamically, such as the platform C library, remain platform dependencies. This binary does not need Python, a virtualenv, or a second service.

CI on Ubuntu runs formatting, clippy with warnings denied, `cargo test --locked`, a release build, and `cargo audit`.

## Upgrade

Workspace files do not need a migration to move from Python Commonroom to this binary.

* Keep the markdown file where it is.
* Keep the `.commonroom.db` sidecar beside it. The schema is the same, including the hashed invitation columns.
* Existing `commonroom://join/` invitations still decode. A spent or expired invitation still fails.
* Agent session files under `commonroom/sessions` are reused when the session is still valid.
* Skill URIs stay `skill://commonroom/SKILL.md`. Update the agent's command from `commonroom` to `ce`. The paste block printed by a room already uses `ce`.

Cookie names and protocol identifiers were not renamed, so a browser or client that already speaks Commonroom can talk to `ce` without a flag day.

There is no hosted account to migrate. When a hosted service exists, local rooms stay valid without it.
