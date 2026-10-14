---
title: Supply chain
---

This page is the build, scan, and release policy for the `ce` binary and its container image. It does not change room permissions or MCP authorization. Those are in the [threat model](threat-model.md).

## Build inputs

The compiler is Rust **1.99.0**, pinned in `rust-toolchain.toml`. `Cargo.lock` is committed. Application builds use `cargo build --locked`. The workspace `rust-version` of 1.85 is the minimum the code claims. The toolchain file is the compiler CI and local development install.

The release profile uses optimization level 3, thin LTO, one codegen unit, symbol stripping, and unwind panics. Linux gnu targets add full RELRO, immediate binding, and a non-executable stack from `.cargo/config.toml`. `scripts/check-binary-hardening.py` reads those properties back from the file. A macOS binary is checked for PIE and for the dynamic libraries it actually links. RELRO is not required on macOS.

`ce serve` does not make outbound TLS calls. SQLite is compiled into the binary with `rusqlite`'s bundled feature. The workspace has no `build.rs`. The HTTP tests use `reqwest` without its default TLS features, so the release graph does not include OpenSSL.

`task repro` builds the release binary twice in clean target directories with `SOURCE_DATE_EPOCH` taken from the current commit and incremental compilation disabled. On 2026-10-13, that command on this macOS arm64 host produced the same SHA-256 for both files: `c6ec61fd7f2b2a984c72f54b095e0b75d1ab0aacfe67bb84712dd47779692bae`. That is one host and one target. Linux builds and builds on another machine were not compared, so this is not a general reproducibility claim. A pinned toolchain and lockfile are not, by themselves, a bit-reproducible build.

## Dependency policy

`deny.toml` is the license, advisory, ban, and source policy. `cargo deny check` fails when any of these are true:

* A RustSec advisory vulnerability applies to the locked graph.
* A dependency uses a license outside the allow list.
* A version requirement is a wildcard.
* A crate comes from a git repository or a registry other than crates.io.

Duplicate versions warn. The current warnings are transitive and are not skipped: `base64` 0.22 and 0.23, `hashbrown` 0.14 and 0.17, and `syn` 2 and 3.

The allow list is the set of licenses in the current graph:

* MIT
* Apache-2.0
* BSD-2-Clause
* BSD-3-Clause
* Unicode-3.0
* Unlicense

Workspace crates are unpublished (`publish = false`) and are not license-checked as third-party crates. Unmaintained or unsound advisories on a direct workspace dependency fail the check. Yanked crates warn.

`scripts/cargo-audit-gate.zsh` prints every RustSec finding. High, critical, and unrated vulnerabilities fail that command. Medium, low, and warning findings are printed and do not fail it. `cargo deny` is the stricter gate: any advisory vulnerability fails the build. A database download failure fails both commands.

## Scanners

| Check | Tool | Failure |
| --- | --- | --- |
| Formatting | `cargo fmt --check` | any diff |
| Lint | `cargo clippy -- -D warnings` | any warning |
| Tests | `cargo test --workspace --locked` | any failure |
| Advisories | `cargo audit`, `cargo deny` | see above |
| Secrets in git history | gitleaks 8.30.1 | any leak |
| Binary properties | `scripts/check-binary-hardening.zsh` | missing PIE, RELRO, or an unexpected library |
| Binary and image secrets | Trivy 0.75.0 | any secret |
| Image and binary vulnerabilities | Trivy 0.75.0 | critical or high |
| Image configuration | Trivy 0.75.0 | critical or high |

Trivy prints unknown, low, and medium findings and does not fail on them. It does not see crate versions inside the `ce` binary, because the binary does not embed a dependency bill. Crate advisories are gated from `Cargo.lock` in the same workflow that builds with `--locked`. An SBOM associates that graph with the release. A Trivy database or download failure fails the job.

Pinned binaries and their archive checksums live in `scripts/install-supply-chain-tools.zsh`. `task tools` installs them into `target/supply-chain-bin`.

## Exceptions

There are no ignored advisories and no Trivy ignore file.

A waiver is a reviewed change with three fields: the advisory or CVE id, the reason the build is still acceptable, and an expiry date. For RustSec, that is one `ignore` entry in `deny.toml`. Do not change a lint from `deny` to `allow` for a single finding. Remove the entry when the lockfile no longer needs it. `cargo deny` rejects an expired ignore.

## Release artifacts

A version tag on `main` builds the four `ce` archives, scans each one, and only then attaches them to the GitHub release. The same workflow builds the multi-arch image, pushes it by digest under a temporary `ci-<sha>` tag, scans that digest, and creates the `vX.Y.Z` tag only after the scan succeeds. The temporary tag is removed when the job finishes. Provenance is a GitHub artifact attestation. It records the workflow and the source revision. It does not mean the bytes are free of vulnerabilities.

The release also publishes:

* `SHA256SUMS` for the archives and SBOMs
* `ce.cdx.json`, a CycloneDX 1.5 SBOM of the locked crate graph for every release target
* `ce-image.cdx.json`, a CycloneDX SBOM of the image
* `image-digest.txt`

Pull-request CI builds and scans the native image. It does not push to the registry, and it does not receive a signing token. The image that is published is the one built from the tagged commit.

## Verify a download

From a checkout of the tag:

```sh
gh attestation verify ce-linux-x86_64.tar.gz --repo zloeber/complexity-eater
sha256sum -c SHA256SUMS
```

On macOS, `shasum -a 256 -c SHA256SUMS` checks the same file. For the image:

```sh
gh attestation verify oci://ghcr.io/zloeber/complexity-eater:<tag> --repo zloeber/complexity-eater
```

The attestation names the commit and the workflow. Compare that commit with the tag you intended to install.

## Update process

Dependabot opens one weekly pull request per ecosystem: all Cargo updates together, all GitHub Actions updates together, and all image-digest updates together. Security advisories stay in their own pull requests. An action pin is the full commit SHA, with the release tag in a comment. Merge a grouped update only when CI passes. A breaking bump can fail that one pull request; split it out only for that dependency.

Base images stay on Debian 12: `rust:1.99-bookworm` for the builder, `debian:bookworm-slim` for the copied runtime libraries, and `gcr.io/distroless/static-debian12:nonroot` for the runtime root. Inspect a new digest with `docker buildx imagetools inspect <tag>` and keep the builder and the copied glibc on the same Debian release. The Dockerfile comments record the tag that belonged to each digest on 2026-10-13.

The runtime image does not contain `libssl3`. `ce` links `libc`, `libm`, and `libgcc_s` only. The distroless `cc` image included `libssl3` 3.0.20-1~deb12u2, which Trivy reported as CVE-2026-84782 (high, no fixed version, DTLS handshake retransmission). That package is not copied. There is no ignore entry for it.

Scanner versions change by editing the URLs and SHA-256 checksums in `scripts/install-supply-chain-tools.zsh`.

## Local commands

```sh
task check
task supply-chain
task binary-hardening
task image-verify
task repro
```

`task check` is format, clippy, and tests. `task supply-chain` is the advisory, license, and secret gate. `task image-verify` builds the image, checks its contents, and runs it with the hardened profile from [the container page](../operations/container.md).

## Remaining limits

* `task repro` matched on one macOS arm64 host. Linux and cross-machine builds were not compared.
* The image scan on 2026-10-13 reported one unknown-severity finding, `tzdata` DLA-4792-1 (installed 2026b-0+deb12u1, fixed in 2026c-0+deb12u1). It does not fail the gate. Bump the static image digest when distroless publishes that package. There is no ignore entry.
* Trivy does not inventory Rust crates inside the binary.
* `tokio` still uses the `full` feature set. Narrowing it would be a behavior change, so this pass left it.
* The container uses the host network so the loopback listeners are the host's loopback. That is not privileged mode, and it is not a Docker socket mount.
* The image does not include `tailcat`.
* A tag uses the workflow in the tagged commit. Semantic-release dispatches the workflow on `main`. Restrict who can push tags so an older commit cannot publish with an older workflow.
* A two-machine Tailcat session is still not part of the automated suite.
