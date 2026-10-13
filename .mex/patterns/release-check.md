---
name: release-check
description: Prepare the project for a release by verifying the Rust build, binary output, and workspace invariants.
triggers:
  - prepare a release candidate
  - confirm build or binary packaging
  - validate release automation assumptions
edges:
  - target: ../context/setup.md
    condition: when confirming local build and validation tasks
  - target: ../context/stack.md
    condition: when checking the Rust toolchain and workspace packaging assumptions
  - target: ../ROUTER.md
    condition: when writing a final status update for the project state
last_updated: 2026-10-12
---

# Release check

## Context
The release workflow uses GitHub Actions for CI and semantic-release. The `ce` binary is built for the supported OS/architecture matrix and attached to the release payload.

## Steps
1. Run `task check` or the equivalent cargo commands to confirm formatting, lint, and tests.
2. Run `task release` to validate the production binary build path.
3. Confirm the workspace state and release metadata before pushing or finalizing a tag.
4. If the change is a release-triggering commit, verify the conventional commit prefix matches the project’s semver policy.

## Gotchas
- `docs:`, `test:`, `refactor:`, and similar non-release prefixes do not trigger a version bump.
- Release automation expects CI to pass before semantic release creates a tag.
- The binary workflow attaches archives and `SHA256SUMS` to the GitHub release after tag creation.

## Verify
- `task check`, `task test`, and `task release` cover the expected Rust verification path.
- Ensure the workspace version and release documentation stay aligned with the actual release process.

## Debug
- If the release build fails, inspect `rust-toolchain.toml`, workspace dependencies, and CLI packaging assumptions.
- If a version tag is missing, confirm whether the repo expects the first successful CI on `main` to publish the workspace version.

## Update Scaffold
- [ ] Refresh `.mex/ROUTER.md` if release flow or packaging assumptions changed.
- [ ] Update `context/setup.md` if the commands or published build targets changed.
- [ ] Add a pattern when a new release gate or binary packaging edge case emerges.
