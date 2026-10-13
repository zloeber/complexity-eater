---
name: supply-chain
description: Change the pinned toolchain, dependency policy, container profile, scanners, SBOMs, or release attestations.
triggers:
  - bump the Rust toolchain or a base image digest
  - change cargo-deny, cargo audit, gitleaks, or Trivy policy
  - change the Dockerfile, Compose profile, or release attestations
edges:
  - target: release-check.md
    condition: when the change also needs a release build
  - target: ../context/stack.md
    condition: when the toolchain, image, or scanner set changes
  - target: ../context/decisions.md
    condition: when the distribution or exception policy changes
  - target: ../ROUTER.md
    condition: when writing a final status update for the project state
last_updated: 2026-10-13
---

# Supply chain

## Context
Release publication is gated by the scans in `.github/workflows/release-binaries.yml`. Policy lives in `deny.toml` and `docs/security/supply-chain.md`. The runtime image is distroless and runs as UID 65532.

## Steps
1. Read `docs/security/supply-chain.md` before changing a pin, an allow list, or an exception.
2. Keep builder and runtime base images on the same Debian release.
3. Run `task supply-chain` after a lockfile or policy change.
4. Run `task binary-hardening` after a release-profile or linker change.
5. Run `task image-verify` after a Dockerfile or Compose change.
6. Record a waiver as one dated `ignore` entry. Do not silence a whole class of findings.

## Gotchas
- `ce` binds loopback only. The Compose file uses host networking for that reason. Do not publish the owner port on `0.0.0.0`.
- Trivy does not see crate versions inside the binary. `cargo audit` and `cargo deny` are the crate advisory gates.
- An attestation identifies the workflow and commit. It is not a claim that the artifact is free of vulnerabilities.
- A tag runs the workflow stored in that commit. Semantic-release dispatches the workflow on `main`.

## Verify
- `cargo deny check` exits 0. Duplicate-version warnings may remain.
- `zsh scripts/cargo-audit-gate.zsh` exits 0 when high and critical advisories are absent.
- `task image-verify` exits 0 when Docker is available.

## Debug
- A checksum mismatch in `scripts/install-supply-chain-tools.zsh` means the upstream archive changed. Update the pinned digest only after inspecting the release.
- An image scan failure after a digest push deletes the temporary `ci-<sha>` tag and does not create the version tag.

## Update Scaffold
- [ ] Refresh `.mex/ROUTER.md` if the validation commands change.
- [ ] Update `context/stack.md` when the toolchain or image base changes.
- [ ] Add a decision when an exception or distribution choice is new.
