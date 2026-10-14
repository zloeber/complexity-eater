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
  - target: room-mode.md
    condition: when the room is in baton mode and a direct write is the intended path
last_updated: 2026-10-14
---

# Proposal lifecycle

## Context
The project uses proposals as the controlled change path. A proposal is anchored to a base version and can be reviewed and committed only with the proper permissions.

## Steps
1. Inspect the current workspace status and the relevant region. `workspace_status` reports `heading_count`, not the heading tree. Page regions with `depth`, `under`, and `cursor`.
2. Create a proposal with a target section, base version, summary, and `change.content`. The target is the section. Set `change.section` only when it names that same path. Set `change.mode` to `document`, or target `workspace` or `*`, only when the whole file should be replaced.
3. Review the approval state. The owner page shows a unified diff and flags a whole-document replacement or a large deletion. The host session or a participant with `approve` must approve it.
4. Commit with `proposal_id`. The engine applies the region replacement to the current file when the proposal's region does not overlap what changed since the base. Overlap returns `stale_version` and marks the proposal conflicted.
5. If a bare heading title matches more than one heading, the error is `invalid_region` and `details.candidates` lists the full paths.

## Gotchas
- A commit of one region does not supersede a pending proposal on a different region. Two proposals on the same region still conflict.
- A raw document overwrite is the admin HTTP path without `proposal_id`. MCP `workspace_commit` requires `proposal_id`.
- Proposal status depends on approved state and capability checks, not on raw command-line assumptions.
- In baton mode a holder can write the region directly. See [room-mode](./room-mode.md). Proposals stay available and still need approval.

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
