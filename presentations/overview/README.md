# Complexity Eater — overview deck

A self-contained slideshow for a five-minute recording. It explains the product that exists: two people, their own agents, one markdown file, and an approval before anything lands.

- One file, no dependencies. `index.html` inlines the CSS, the script, and the pictures.
- It opens from disk. No server and no network.
- Eight slides. Arrow keys, space, and `f` for fullscreen.

```sh
open presentations/overview/index.html
```

## Record the video

Use the deck as the spoken spine, then cut in one live beat. Aim for five to seven minutes. Record at 1920×1080.

1. Open the deck and press `f`.
2. On macOS, QuickTime Player → File → New Screen Recording. Record the browser window, not the whole desktop.
3. Talk one slide at a time. About thirty to forty seconds each. The last slide is the shot list for the live beat.
4. Stop. Record a second clip in a terminal, ninety seconds:
   - `task website:preview`
   - Open `http://127.0.0.1:4173/agent` and show that it is markdown, not a room.
   - In another terminal: `ce examples/local-room.md --name Morgan`
   - Show the `Human:` line and the `Invite:` line. Do not leave an invitation in the exported video. Stop `ce` before you publish the clip.
5. Join the two clips in iMovie, CapCut, or QuickTime. Deck first, terminal second. Export 1080p.

Do not record a Cloudflare API token, a GitHub token, or a `commonroom://join/` string you still intend to use.

The live site at `https://complexityeater.com/agent` still serves the homepage until the Pages project publishes commit `8cf3823`. Use the local preview for the recording until that deploy is the production deployment.

## Cloudflare

The public site is Cloudflare Pages project `complexity-eater`. Production is the `main` branch. The room is not hosted there. `ce` still binds loopback.

GitHub Actions (`.github/workflows/website.yml`) deploys after `scripts/check-website.zsh` passes, and only when both repository secrets exist:

- `CLOUDFLARE_API_TOKEN` — permission to edit Cloudflare Pages for the account
- `CLOUDFLARE_ACCOUNT_ID` — the account shown by `npx wrangler whoami`

If either secret is missing, the job logs that it skipped and exits successfully. The previous deployment stays up.

A push to `main` does not by itself prove a new deploy. Check Workers & Pages → `complexity-eater` → Deployments. The production row should name the commit you just pushed.

To publish from this machine when Actions did not:

```sh
task website:build
npx wrangler@4.151.0 pages deploy website/dist --project-name complexity-eater --branch main --commit-dirty=true
```

Wrangler must already be logged into the account that owns the zone `complexityeater.com`. The custom domain is attached in the dashboard: Workers & Pages → `complexity-eater` → Custom domains → `complexityeater.com`. Do not attach that name to a preview deployment.

`https://complexityeater.org` does not resolve. The URL to give an agent is `https://complexityeater.com/agent`.
