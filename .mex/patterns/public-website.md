---
name: public-website
description: Build, check, and deploy the static public site without coupling it to the room process.
triggers:
  - change the public website
  - update complexityeater.com deployment
  - edit the room-launch contract
edges:
  - target: ../ROUTER.md
    condition: when starting a session or refreshing project state
  - target: ../context/architecture.md
    condition: when the site starts to describe room behavior
  - target: INDEX.md
    condition: when this pattern needs to be updated or cross-referenced
  - target: context-drop.md
    condition: when a page starts to describe a context drop or the scenario index
last_updated: 2026-10-17
---

# Public website

## Context
`website/` is a static front door for `https://complexityeater.com`. GitHub Pages still publishes `docs/`. The site prepares local `ce` commands from `website/contracts/room-launch.v1.json` and copies the commonroom skills, including the context drop skill, plus the `workspace_onboarding` prompt from `skills/commonroom/`. `skills/commonroom/AGENT.md` is the template for the agent entry. It opens with a scenario index. The build fills its release URLs and publishes that markdown at `/agent`. Hosted mode in that contract stays `not_implemented`. The site does not start a drop.

## Steps
1. Edit pages in `website/pages/` and shared assets in `website/assets/`.
2. Keep install links in `website/releases/` equal to a real GitHub release, including `image-digest.txt`.
3. Run `task website:check`.
4. Preview with `task website:preview` and read the changed pages in a browser.
5. Deploy only through `.github/workflows/website.yml`. Production is the `main` branch of the Cloudflare Pages project `complexity-eater`.

## Gotchas
- The website does not upload a markdown file or create a room. Do not add a control that reports success for a room operation.
- Do not put invitation secrets, session ids, or API tokens in pages, query strings, or the repository.
- `complexityeater.com` is already a Cloudflare zone. It does not resolve until the Pages custom domain is attached. Preview hostnames stay on `pages.dev`.
- A `feat:` commit on `main` publishes a new `ce` release. A website-only change does not need that.
- Skill files with publish mode `copy`, including `SKILL.md`, `ADOPT.md`, and `DROP.md`, are copies of `skills/commonroom/`. Do not rewrite them on the page. The adoption skill and the drop skill stay off the MCP skill list. The room still serves only `skill://commonroom/SKILL.md`.
- `/agent` is the filled form of `AGENT.md`, not a second copy of the template. It opens with the scenario index. The canonical URL is `https://complexityeater.com/agent`. `complexityeater.org` is not this site. The document must not contain an invitation or claim that fetching it opened a room or started a drop.

## Verify
- `task website:check` fetches the release archives named by the manifest.
- `cargo test --locked -p ce-cli --test room_launch_contract` keeps the contract on the CLI surface.

## Debug
- If a release URL fails, the manifest is ahead of GitHub or the asset name changed.
- If the deploy job skips, `CLOUDFLARE_API_TOKEN` or `CLOUDFLARE_ACCOUNT_ID` is unset. The check job should still pass.

## Update Scaffold
- [ ] Refresh `.mex/ROUTER.md` if the site becomes part of the room process or stops being static.
- [ ] Update `context/architecture.md` if the launch contract gains a real hosted provider.
