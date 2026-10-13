---
title: Release
---

## Local build

```sh
task release
```

That runs `cargo build --release --locked --bin ce`. The compiler is the version in `rust-toolchain.toml`. The release profile uses optimization level 3, thin LTO, one codegen unit, and stripped symbols. The artifact is `target/release/ce`.

`ce --version` prints the workspace package version. `Cargo.lock` and the toolchain pin are the build inputs. `task repro` reports whether two clean builds produce the same bytes. System libraries that Rust links dynamically, such as the platform C library, remain platform dependencies. The release binary does not need another language runtime or a second service.

CI runs formatting, clippy with warnings denied, `cargo test --locked`, `cargo audit`, `cargo deny`, a secret scan, a release-binary scan, and a container build and scan. The [supply-chain page](../security/supply-chain.md) is the policy.

## Tagged releases

A push to `main` that passes CI is inspected for semantic-release commit prefixes:

| Prefix | Version bump |
| --- | --- |
| `feat!:` or a `BREAKING CHANGE:` footer | major |
| `feat:` | minor |
| `fix:` or `perf:` | patch |

`docs:`, `test:`, `refactor:`, `chore:`, `ci:`, `build:`, and `style:` do not create a tag.

The first successful CI on `main`, before any `vX.Y.Z` tag exists, publishes the workspace version from `Cargo.toml`. Later releases still require one of the prefixes above.

A releasable commit updates the workspace version in `Cargo.toml` and `Cargo.lock`, tags `vX.Y.Z`, and opens a GitHub release. The tag then builds `ce` and attaches:

* `ce-linux-x86_64.tar.gz`
* `ce-linux-aarch64.tar.gz`
* `ce-macos-aarch64.tar.gz`
* `ce-macos-x86_64.tar.gz`
* `SHA256SUMS`
* `ce.cdx.json`
* `ce-image.cdx.json`
* `image-digest.txt`

Each archive contains a single executable named `ce`. The workflow scans those archives before it uploads them. It also pushes `ghcr.io/zloeber/complexity-eater:<tag>` after the image scan passes. GitHub artifact attestations are attached to the archives, the checksum file, and the image. [How to verify them](../security/supply-chain.md).

The container profile is on the [container page](container.md).

## Upgrade

Keep the markdown file where it is, and keep the `.commonroom.db` sidecar beside it. Invitations use `commonroom://join/`. A spent or expired invitation still fails. There is no hosted account to move.
