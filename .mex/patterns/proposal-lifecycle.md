---
name: proposal-lifecycle
description: Create, review, approve, and commit a proposal using the engine-backed approval flow.
triggers:
  - apply a change through a proposal
  - review or approve a proposal
  - diagnose stale version or approval conflicts
edges:
  - target: ../context/architecture.md
    condition: when checking how engine state and proposal writes interact with the markdown artifact
  - target: ../context/conventions.md
    condition: when verifying the repo’s approval and validation expectations
  - target: ../ROUTER.md
    condition: when the project state has changed after a proposal or approval flow
last_updated: 2026-10-12
---

# Proposal lifecycle

## Context
The project uses proposals as the controlled change path. A proposal is anchored to a base version and can be reviewed and committed only with the proper permissions.

## Steps
1. Inspect the current workspace status and the relevant region or section to modify.
2. Create a proposal with a target section, base version, summary, and change payload.
3. Review the approval state; the host session or a participant with `approve` authority must approve it.
4. Commit only when the proposal is approved and the base version is still current.
5. If the markdown file changed externally, detect `stale_version` and resolve the conflict before retrying.

## Gotchas
- A stale base version returns `stale_version` and marks the proposal as conflicted.
- A raw document overwrite is a different and higher-privilege action; it is not the normal proposal path.
- Proposal status depends on approved state and capability checks, not on raw command-line assumptions.

## Verify
- Use the parity test and relevant CLI checks before assuming the approval path works.
- Confirm that the engine records the current version and the resulting diff before commit.

## Debug
- If approval is denied, confirm the session capabilities and whether the participant holds `approve`.
- If commit fails, check whether the current base version changed after proposal creation or before review.

## Update Scaffold
- [ ] Review the current state in `.mex/ROUTER.md` and update any changed assumptions.
- [ ] Check whether `context/decisions.md` or `context/architecture.md` needs a note on approval behavior.
- [ ] Add a new pattern if a different approval mode or edge case emerges.
