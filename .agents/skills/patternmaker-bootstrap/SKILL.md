---
name: patternmaker-bootstrap
description: One time bootstrap for the patternmaker scaffold to scour for existing patterns and context in the project
---

# patternmaker-bootstrap

You are expanding the existing .mex/ pattern library for this project by discovery.

Do not install or use the mex binary. Work only with the Markdown scaffold already present under .mex/.

## Goal
Find high-value, project-specific patterns that are currently missing, then add only the ones that meet the quality bar defined in patterns/README.md. Prefer evidence from real history and real code over speculation.

### Phase 1 — Inventory what already exists

1. Read .mex/ROUTER.md (especially Current Project State and the GROW contract).
2. Read .mex/patterns/INDEX.md and patterns/README.md so you know the exact format and the “when to create / when to skip” rules.
3. Skim the existing pattern files so you do not duplicate them.

### Phase 2 — Mine sources (in this order)

Search these sources for repeated tasks, non-obvious gotchas, failure modes, and verify steps:

- CHANGELOG.md / HISTORY.md / release notes / GitHub releases (if present)
- docs/, README, any ADR or decision logs
- Recent meaningful commits (git log --oneline -n 40 is enough; look for recurring themes)
- Test names and test structure (they often encode the real “how we verify X” knowledge)
- Comments and TODOs that describe hard-won lessons
- Existing .mex/context/*.md files for gaps they already hint at

Look specifically for:
- Tasks developers perform repeatedly
- Integration points that have bitten people before
- Failure boundaries between components
- Migration / upgrade / deprecation notes that contain procedural knowledge
- “Remember to also…” type instructions that currently live only in prose

### Phase 3 — Decide what is worth a pattern

Apply a strict filter. Create a new pattern only if at least two of these are true:

- The task is common or high-risk in this project
- There are non-obvious steps or ordering constraints
- There are project-specific gotchas not already covered in conventions.md
- A short verify checklist would catch real mistakes
- The knowledge currently lives only in release notes, commit messages, or tribal knowledge

Skip anything that is:
- Already covered adequately by an existing pattern or by conventions.md
- Purely generic advice
- One-off historical noise that will not recur

### Phase 4 — Author the patterns

For each approved pattern:

- Use the exact single-task or multi-section format from patterns/README.md
- Derive concrete file paths, command names, and gotchas from the sources you mined
- Keep it tight and actionable
- End with the standard Update Scaffold checklist
- Give it a descriptive kebab-case filename

After writing the files, update patterns/INDEX.md with clean rows for every new pattern.

### Phase 5 — Light scaffold maintenance

- If any new pattern reveals that a context/ file is now incomplete or inaccurate, make a surgical update (preserve frontmatter, bump last_updated).
- If Current Project State in ROUTER.md should reflect newly captured knowledge, update it briefly and bump last_updated.
- Do not rewrite large sections. Prefer small, precise edits.

### Finish

Report:
- Sources you mined
- Patterns you considered but rejected (and why)
- New patterns created (filename + one-line purpose)
- Any context/ or ROUTER.md updates made
- Confirmation that the scaffold remains pure Markdown and still requires no mex binary

When in doubt, create fewer, higher-quality patterns rather than many thin ones.