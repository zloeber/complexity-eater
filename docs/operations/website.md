---
title: Public website
---

The public site is the static tree in `website/`. It is the front door for `https://complexityeater.com`. It is not the documentation site, and it does not run a room.

GitHub Pages continues to publish `docs/` at `https://zloeber.github.io/complexity-eater/`. The two sites deploy separately.

## Local preview

```sh
task website:build
task website:preview
```

`task website:preview` serves `website/dist` on `http://127.0.0.1:4173/`. `task website:check` rebuilds the site and checks structure, internal links, the room-launch contract, and the published release assets.

The build copies `docs/assets/ce-banner.png` into the site. It does not call Rust. `website/dist` is generated and is not committed.

## What gets published

`scripts/build-website.zsh` writes one HTML file per page, the stylesheet and scripts, the favicon, the banner, `robots.txt`, `sitemap.xml`, `_headers`, and `contracts/room-launch.v1.json`.

Canonical URLs use `https://complexityeater.com`. The install page is filled from `website/releases/v0.2.1.json`. When a newer GitHub release should become the one the site describes, replace that manifest with the new tag's real asset URLs and digest, then run `task website:check`. The check fetches `image-digest.txt` and the archive URLs. It fails if they do not match.

Do not put API tokens, invitation strings, or session ids in the site. `_headers` sends `Referrer-Policy: no-referrer` and a Content-Security-Policy that allows only this origin's scripts and images.

## Cloudflare

The zone `complexityeater.com` is already in Cloudflare. As of this writing the name has no public DNS record, so it does not resolve until the Pages project is attached. There is no registrar transfer in this setup.

One-time project creation, from a machine that is already logged in to the same Cloudflare account:

```sh
task website:build
npx wrangler@4.151.0 pages project create complexity-eater --production-branch main
npx wrangler@4.151.0 pages deploy website/dist --project-name complexity-eater --branch main --commit-dirty=true
```

Wrangler is pinned to `4.151.0` in `.github/workflows/website.yml`. The command above uses the same version. Production is the `main` branch only.

Then, in the Cloudflare dashboard for that account:

1. Open Workers & Pages, then the `complexity-eater` project.
2. Add the custom domain `complexityeater.com`. Because the zone is already in the account, Cloudflare can create the DNS record and the certificate.
3. Add a redirect rule on the zone: if the hostname is `www.complexityeater.com`, respond with a 301 to `https://complexityeater.com` plus the original path and query. The apex is the canonical host.
4. Do not attach `complexityeater.com` to a preview deployment.

HTTPS is terminated by Cloudflare. The Pages project is static files. It does not need a Worker, a database, or an always-on server. Stay on the Pages plan that already fits a small static site, and confirm Cloudflare's current build and bandwidth limits before expecting a spike.

## GitHub Actions

`.github/workflows/website.yml` deploys after `scripts/check-website.zsh` passes.

| Event | Cloudflare branch | Environment |
| --- | --- | --- |
| Push to `main` | `main` | `website-production` |
| Pull request | the head branch, with `/` replaced by `-` | `website-preview` |

Preview URLs are `*.complexity-eater.pages.dev`. They are not the custom domain.

The workflow reads two GitHub Actions secrets and does not store them in the repository:

* `CLOUDFLARE_API_TOKEN`, with permission to edit Cloudflare Pages for the account.
* `CLOUDFLARE_ACCOUNT_ID`.

If either secret is missing, the deploy job logs that it skipped and exits successfully. The site is not live until both secrets exist, a production deploy has succeeded, and the custom domain has been attached. Pull requests from forks do not receive the secrets.

`ci.yml` runs the same website check on every push and pull request. That job does not deploy.

## Rollback

Prefer a git revert of the website commit on `main`. The next production deploy publishes that tree again, and the repository matches what visitors see.

To roll back without a new commit, open the `complexity-eater` project in Cloudflare, choose the previous production deployment, and use Rollback. That restores the static files. It does not change the custom domain and it does not change git. Follow it with a revert so the next push does not redeploy the bad tree.

If a deploy attaches the apex to the wrong project, remove `complexityeater.com` from that project and add it again on `complexity-eater`. DNS for this zone is the Cloudflare record created with the custom domain, not a second host.

Deleting a Pages deployment removes that copy of the static files. It does not delete a Complexity Eater room, because this site does not create rooms.
