---
name: patternmaker
description: initialize a continuous patternmaker loop to a project scaffold with real-time context and pattern discovery
---

# patternmaker

Install a self-updating development pattern router into this existing project.
Use only ordinary Markdown. Do not install or depend on the mex binary or any external CLI.

## Goal
Agents start every session from a tiny always-loaded surface, route through a single high-signal ROUTER, load only the context and patterns needed for the current task, then grow the scaffold from real work so future sessions inherit the knowledge.

Create this structure (merge carefully if anything similar already exists):

.mex/
├── AGENTS.md                 # tiny always-loaded anchor (~150–250 tokens)
├── ROUTER.md                 # session bootstrap + current state + routing + GROW
├── context/
│   ├── architecture.md
│   ├── stack.md
│   ├── conventions.md
│   ├── decisions.md
│   └── setup.md
└── patterns/
    ├── INDEX.md
    ├── README.md
    └── <starter patterns>

Also ensure the root always-loaded file used by this repo’s AI tools (AGENTS.md, CLAUDE.md, .cursorrules, etc.) contains exactly this instruction near the top:

“Start every session by reading .mex/ROUTER.md before any other work.”

### Pass 1 — Explore

Read the real codebase: README, dependency files, entry points, folder structure, and 2–4 representative files from each major layer. Prefer primary sources over documentation.

### Pass 2 — Populate context/

Write five concise, concrete files. Every file must have this frontmatter:

---
name: <short-name>
description: <one-line purpose>
edges:
  - target: <relative path>
    condition: <when to follow>
last_updated: YYYY-MM-DD
---

Content rules:
- architecture.md — major components, how they connect, primary request/data flows, external boundaries
- stack.md — languages, frameworks, key libraries, build/runtime tools (real ones only)
- conventions.md — naming, layout, error handling, testing, logging patterns that actually appear in the code
- decisions.md — architectural decisions with date + rationale. Never delete; mark superseded and add the new entry above
- setup.md — install, run, test, lint, common env vars (metadata only)

If a domain is deep enough that it would bloat architecture.md, create context/<domain>.md with the same frontmatter and add a routing edge. Mark unknowns “[TO DETERMINE]”.

### Pass 3 — Write the two anchors

.mex/AGENTS.md (keep it short):

---
name: agents
description: Always-loaded project anchor. Identity, non-negotiables, commands, pointer to ROUTER.
last_updated: YYYY-MM-DD
---

# <Project Name>

## What This Is
One or two precise sentences.

## Non-Negotiables
- Hard rules that must never be violated

## Commands
- Real setup / test / lint / run commands from the repo

## Scaffold Growth
After meaningful work, run the GROW step in ROUTER.md. Create or update patterns when the task type is new or non-obvious. The scaffold grows from real work.

## Navigation
Read ROUTER.md at the start of every session before doing anything else.

.mex/ROUTER.md:

---
name: router
description: Session bootstrap and navigation hub. Read at the start of every session. Contains project state, routing, and the GROW contract.
edges:
  - target: context/architecture.md
    condition: system design, integrations, component relationships
  - target: context/stack.md
    condition: technologies, libraries, tech decisions
  - target: context/conventions.md
    condition: writing or reviewing code, project patterns
  - target: context/decisions.md
    condition: architectural choices or “why is it built this way”
  - target: context/setup.md
    condition: first-time setup or running the project
  - target: patterns/INDEX.md
    condition: starting any concrete task — check for a matching pattern first
last_updated: YYYY-MM-DD
---

# Session Bootstrap

Read .mex/AGENTS.md if you have not already. Then read this file fully before any other work.

## Current Project State
Keep this section high-signal and short. Prefer 5–12 bullets max.

**Working**
- …

**Gaps / In progress**
- …

## Behavioural Contract
1. Start here every session.
2. Before a concrete task, open patterns/INDEX.md. If a matching pattern exists, follow it.
3. Load only the context/ and patterns/ files indicated by edges and the task. Do not load the whole scaffold.
4. After meaningful work, execute GROW before considering the task complete.

## GROW
- Ground — what actually changed?
- Record — surgically update Current Project State and any affected context/ files. Preserve frontmatter. Never delete decisions; mark them superseded.
- Orient — if this was a new or non-obvious task type, create or refine a pattern and update INDEX.md.
- Write — bump last_updated on every file you touched. Keep edges useful and preferably bidirectional.

### Pass 4 — Pattern system + starters

Write patterns/README.md that defines:
- A pattern is worth creating when the task is common, has non-obvious gotchas, sits on a failure boundary, or benefits from a project-specific verify checklist.
- Skip only if the guidance is already complete in conventions.md or is pure generic advice.
- Exact formats (single-task and multi-section) with YAML frontmatter (name, description, triggers, edges, last_updated) and sections: Context, Steps, Gotchas, Verify, Debug, Update Scaffold.
- Categories: common tasks, integrations, debug/diagnosis, deploy/release (only if non-trivial).

Write patterns/INDEX.md as a clean lookup table:

| Pattern | Use when |
|---------|----------|

Generate 3–6 high-value starter patterns based on real files and real risks in this codebase. Prefer the tasks a developer does most often and the places most likely to be gotten wrong. Each pattern ends with an Update Scaffold checklist that points back to ROUTER.md and the relevant context files.

### Pass 5 — Wire and finish

Re-read every file. Ensure:
- Every context file has ≥2 edges
- Every pattern has ≥1 edge
- Edges are useful and bidirectional where it makes sense
- Root always-loaded file points to .mex/ROUTER.md
- No invented facts; unknowns marked “[TO DETERMINE]”
- Current Project State stays high-signal

Report:
- Files created or updated
- Starter patterns generated
- Any remaining [TO DETERMINE] slots
- Confirmation that the entire system is pure Markdown and requires no mex binary

Ongoing behaviour (already encoded in the scaffold):
- Every session starts at .mex/ROUTER.md
- Agents follow patterns when they exist
- Agents run GROW after real work
- The router and patterns stay current without any external tool