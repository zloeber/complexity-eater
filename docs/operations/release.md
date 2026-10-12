---
title: Release
---

## Local build

```sh
task release
```

That runs `cargo build --release --locked --bin ce`. The release profile strips symbols, uses thin LTO, and one codegen unit. The artifact is `target/release/ce`.

`ce --version` prints the workspace package version. A source checkout plus `Cargo.lock` is the reproducible input. System libraries that Rust links dynamically, such as the platform C library, remain platform dependencies. The release binary does not need another language runtime or a second service.

CI on Ubuntu runs formatting, clippy with warnings denied, `cargo test --locked`, a release build, and `cargo audit`.

## Tagged releases

A push to `main` that passes CI is inspected for semantic-release commit prefixes:

| Prefix | Version bump |
| --- | --- |
| `feat!:` or a `BREAKING CHANGE:` footer | major |
| `feat:` | minor |
| `fix:` or `perf:` | patch |

`docs:`, `test:`, `refactor:`, `chore:`, `ci:`, `build:`, and `style:` do not create a tag.

A releasable commit updates the workspace version in `Cargo.toml` and `Cargo.lock`, tags `vX.Y.Z`, and opens a GitHub release. The tag then builds `ce` and attaches:

* `ce-linux-x86_64.tar.gz`
* `ce-linux-aarch64.tar.gz`
* `ce-macos-aarch64.tar.gz`
* `ce-macos-x86_64.tar.gz`
* `SHA256SUMS`

Each archive contains a single executable named `ce`.

## Upgrade

Keep the markdown file where it is, and keep the `.commonroom.db` sidecar beside it. Invitations use `commonroom://join/`. A spent or expired invitation still fails. There is no hosted account to move.
