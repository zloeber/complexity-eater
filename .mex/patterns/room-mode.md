---
name: room-mode
description: Switch a room between review and baton, and keep the owner session on the printed loopback URL.
triggers:
  - start a room in baton mode
  - grant, renew, release, or revoke the baton
  - debug the owner cookie or a cross-site owner request
edges:
  - target: ../context/architecture.md
    condition: when checking how mode and the owner listener sit beside the engine
  - target: ../context/decisions.md
    condition: when the reason for the owner token or baton mode is in question
  - target: proposal-lifecycle.md
    condition: when a change should go through approval instead of a direct write
  - target: ../ROUTER.md
    condition: when project state should mention the room mode or the owner gate
last_updated: 2026-10-14
---

# Room mode and the owner gate

## Context
`review` is the default. A participant proposes, someone with `approve` accepts, and commit applies the leased region. `baton` adds a time-boxed holder who can write one region without that approval. The markdown file stays the artifact. The owner page is the review surface for both.

## Steps
1. Start with `ce serve notes.md` for review, or `ce serve notes.md --mode baton --baton-max-minutes 15`. Zero minutes is refused. `--baton-default-minutes` cannot exceed the cap. The same values can live in `notes.md.commonroom.toml` under `[baton]`, or in the user `config.toml`. An explicit flag wins. `--baton-max-renewals` caps extensions of one grant.
2. Open the printed Human URL, including its one-time `token`. The listener sets `commonroom_session` and redirects to `/`. The banner shows the mode and, in baton mode, the holder.
3. A participant with `commit` calls `workspace_baton_request`. A request longer than the cap is clamped. A second participant sees `baton_held` with `details.holder` and `details.expires_at`.
4. The holder calls `workspace_write` with `baton_id`, `target`, `base_version`, `summary`, and the same `change` object as a proposal. The write replaces that region. `stale_version` still applies.
5. Release with `workspace_baton_release`, or let it expire. The owner revokes with `POST /workspace/baton/revoke` or switches back to review with `POST /workspace/mode`. Switching to review revokes the holder.
6. Recent commits on the owner page include a diff and a revert. Revert appends a new version; it does not rewrite history.

## Gotchas
- `workspace_propose` stays available in baton mode. Committing it still requires approval.
- Checkout and write by anyone other than the holder return `baton_held`. Reads stay open. A write with no baton is `baton_required`. An expired baton is `baton_expired` and leaves the file unchanged. A write in review mode is `mode_mismatch`.
- Mode changes and revoke require `admin`. A default invitation cannot do either.
- `GET /` without the token does not set the cookie. A non-loopback `Host` is rejected. A mutating owner request with a foreign `Origin`, or with `Sec-Fetch-Site: cross-site`, is rejected.
- The same holder calling `workspace_baton_request` again renews. `workspace_baton_renew` extends to at most now plus the cap. When `max_renewals` is set, the next renewal after that count returns `baton_renewal_limit`. Release, then request, starts a new grant at zero renewals.

## Verify
- `cargo test -p ce-core --lib baton` and the owner-listener test in `ce-http` cover the grant, the region write, expiry, revoke, and the cookie gate.
- On the owner page, confirm the mode banner, a proposal diff, a mode switch, and a revert of a recent commit.

## Debug
- If a write returns `mode_mismatch`, the room is in review. Switch with an admin session or restart with `--mode baton`.
- If the owner page loads and no cookie is set, the token was already spent or the `Host` was not loopback. Restart the room and open the new Human URL.
- If a foreign page can mutate the room, confirm the request carried a loopback `Origin` and was not `cross-site`.

## Update Scaffold
- [ ] Review `.mex/ROUTER.md` if the default mode or the owner gate changes.
- [ ] Update `context/decisions.md` when the reason for review versus baton changes. Mark the old decision superseded; do not delete it.
- [ ] Keep [proposal-lifecycle](./proposal-lifecycle.md) aligned when commit semantics change.
