---
name: pattern-readme
description: Rules for when to add a project-specific pattern and how to format it.
triggers:
  - common task repetition
  - guardrail-heavy workflow
  - non-obvious failure boundary
  - repeated validation or debug checklist
edges:
  - target: ../ROUTER.md
    condition: when starting any concrete task and looking for a route
  - target: ../context/conventions.md
    condition: when deciding whether generic guidance is already captured in project conventions
  - target: INDEX.md
    condition: when adding a new pattern to the registry
last_updated: 2026-10-12
---

# Pattern Guide

A pattern is worth creating when the task is common, has non-obvious gotchas, sits on a failure boundary, or benefits from a project-specific verification checklist.

Skip creating a pattern only when the guidance is already complete in `context/conventions.md` or when the advice is purely generic and not project-specific.

## Pattern format

### Single-task pattern
```yaml
---
name: task-name
description: One-line summary of the work.
triggers:
  - phrase or scenario that should route here
edges:
  - target: ../context/architecture.md
    condition: when understanding the system boundary involved
last_updated: 2026-10-12
---

# Task Name

## Context
Describe the scenario and what part of the system is involved.

## Steps
1. Do the key tasks in order.
2. Validate each assumption before moving on.

## Gotchas
List failure modes, edge cases, or protocol constraints.

## Verify
Document the exact tests, checks, or manual validation steps.

## Debug
Call out likely failure paths and troubleshooting signals.

## Update Scaffold
- [ ] Review `.mex/ROUTER.md` and current project state.
- [ ] Update the relevant context file if the task changed system assumptions.
- [ ] Record any new lessons or new pattern in `INDEX.md`.
```

### Multi-section pattern
Use the same frontmatter, then split content into named sections that reflect the real work: Context, Steps, Gotchas, Verify, Debug, and Update Scaffold. Keep the sections short, concrete, and anchored to the repo’s actual commands and boundaries.

## Categories
- Common tasks
- Integrations
- Debug / diagnosis
- Deploy / release

Patterns should point back into the scaffold with useful edges and avoid duplicating general Rust conventions already captured in `context/conventions.md`.
