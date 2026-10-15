---
name: complexity-eater-agent
description: Install the ce binary, optionally install tailcat, and save the Complexity Eater skills used to open a room or join one. Use when a person points you at https://complexityeater.com/agent or asks you to set Complexity Eater up on a machine.
---

# Complexity Eater agent entry

Canonical URL: `https://complexityeater.com/agent`

If you were told to open `https://complexityeater.org/agent`, fetch `https://complexityeater.com/agent` instead. `complexityeater.com` is the public site. This document is the procedure. Fetching it does not create a room, upload a markdown file, or mint an invitation.

Do the steps in order. Stop and tell the person when a step does not match this machine. Do not invent a download URL, a transport address, or an invitation.

The release named below is {{release.tag}}. Newer tags, when they exist, are listed at {{release.page}}. Do not guess a newer archive name. If this document and `ce --version` disagree after a later upgrade, fetch this URL again before you download.

## What you are installing

- `ce` is the room. One binary opens a room around a markdown file, joins a room, and speaks MCP.
- `tailcat` is optional. It is a separate program. Install it only when a participant is on another machine, and only after the person agrees.
- Two skills do the work after install. `commonroom` joins and edits. `complexity-eater-adopt` opens a room and invites. Save this file as well so a later session can upgrade without being pointed here again.

A machine-readable copy of the publish set:

```json
{{agent_manifest}}
```

## 1. Install ce

Published archives exist for macOS Apple silicon (`arm64`), macOS Intel (`x86_64`), Linux `x86_64`, and Linux `aarch64`. Each archive contains one executable named `ce`. A Windows binary is not published. A Homebrew formula for `ce` is not published. Do not build from source unless the person asks you to. Building needs the Rust toolchain pinned in the repository's `rust-toolchain.toml`.

Match the archive to `uname -s` and `uname -m`:

{{archive_list}}

Example for the archive you selected. Replace the file name and URL with that row. On Linux, use `sha256sum -c -` instead of `shasum`.

```sh
mkdir -p "$HOME/.local/bin"
tmpdir=$(mktemp -d)
curl -fsSL -o "$tmpdir/ARCHIVE" "ARCHIVE_URL"
curl -fsSL -o "$tmpdir/SHA256SUMS" "{{release.checksums}}"
grep 'ARCHIVE' "$tmpdir/SHA256SUMS" | (cd "$tmpdir" && shasum -a 256 -c -)
tar -xzf "$tmpdir/ARCHIVE" -C "$tmpdir"
install -m 755 "$tmpdir/ce" "$HOME/.local/bin/ce"
"$HOME/.local/bin/ce" --version
```

The checksum shows the archive matches the release file. It does not show who published it. When `gh` is already installed, also run:

```sh
gh attestation verify "$tmpdir/ARCHIVE" -R zloeber/complexity-eater
```

Use the archive you downloaded. Do not use `sudo` for `ce`. Do not copy it to `/usr/local/bin` unless the person asks. If the shell cannot find `ce` after install, tell the person to add `$HOME/.local/bin` to `PATH`. Do not edit their shell startup file unless they ask.

`ce --version` must print {{release.tag}} without the leading `v`, or a newer version the person asked you to install. If the checksum fails, delete the archive and stop.

## 2. Install tailcat only when another machine will join

Skip this step for a room that stays on one machine. Loopback does not need `tailcat`.

Ask the person before you install it. If they say no, stop this step and use loopback. If `tailcat` is missing and someone starts `ce` with `--tailcat`, the process exits with `transport_unavailable` and does not open a public listener.

Use an install method from <https://github.com/tailscale/tailcat/blob/main/INSTALL.md>. Do not invent a package name or a URL.

- macOS with Homebrew: `brew install tailcat`
- Linux with snap, only if the person agrees to snap: `sudo snap install tailcat`
- A machine with a Go toolchain: `go install github.com/tailscale/tailcat/cmd/tailcat@latest`
- Anything else: follow the install document above. Prebuilt Linux and Windows archives are on the tailcat releases page linked from that document. macOS prebuilt archives are not the Homebrew path.

Then confirm the program runs:

```sh
command -v tailcat
tailcat --help
```

A two-machine Tailcat session is not part of this repository's automated test suite. The plugin exists. Do not describe a remote session as proven by CI.

## 3. Install the skills

Save the skill files where this agent already loads skills. Tell the person the directory you used.

- Cursor, for the person's own agents: `$HOME/.cursor/skills/<name>/SKILL.md`. Do not write under `$HOME/.cursor/skills-cursor`. That directory is reserved.
- Claude Code, when that is the host the person named: `$HOME/.claude/skills/<name>/SKILL.md`.
- This repository, only when the person asked you to install the skills into the project: `.agents/skills/<name>/SKILL.md` if that tree already exists, otherwise `.cursor/skills/<name>/SKILL.md`.

If you cannot tell which host should receive them, ask. Do not copy the files into every skill directory you find.

```sh
root="${COMPLEXITY_EATER_SKILLS:-$HOME/.cursor/skills}"
mkdir -p "$root/commonroom" "$root/complexity-eater-adopt" "$root/complexity-eater-agent"
curl -fsSL -o "$root/commonroom/SKILL.md" "https://complexityeater.com/skills/commonroom/SKILL.md"
curl -fsSL -o "$root/complexity-eater-adopt/SKILL.md" "https://complexityeater.com/skills/commonroom/ADOPT.md"
curl -fsSL -o "$root/complexity-eater-agent/SKILL.md" "https://complexityeater.com/agent"
```

Check the first lines. `commonroom/SKILL.md` begins with `name: commonroom`. `complexity-eater-adopt/SKILL.md` begins with `name: complexity-eater-adopt`. Leave the bytes as published. Do not rewrite them.

Use those skills from here on. `commonroom` is how you join, lease, propose, and commit. `complexity-eater-adopt` is how you open a room and hand another agent the invitation. Once you are inside a room, prefer the copy it serves at `skill://commonroom/SKILL.md`. The public file is the same text for use before you are connected.

## 4. Open a room

You are on the machine that has the markdown file. This site did not create the room.

```sh
ce notes.md --name "Host"
```

That is the same as `ce serve notes.md --name "Host"`. The process binds `127.0.0.1` twice. The line printed as `Human:` is the owner page. The line printed as `Invite:` is the secret. Give the joining agent the invitation, not the owner URL.

For a participant on another machine, after `tailcat` is installed and the person agreed:

```sh
ce notes.md --name "Host" --tailcat
```

Another invitation for a room that is already running:

```sh
ce invite --workspace notes.md
```

A default invitation grants `read`, `observe`, `propose`, `checkout`, and `commit`. It does not grant `approve`, `invite`, or `admin`. It expires in 60 minutes and works once. A failed dial does not spend it.

`--ephemeral` deletes the temporary sidecar directory when the process exits. The markdown file stays.

## 5. Join a room that already exists

The host gives you a string that looks like `commonroom://join/<invitation>`. Treat it as a password. Do not put it in a public page, a query string, a ticket, or this website. Do not turn a tailcat hint into a URL.

Prefer MCP, and leave the process running:

```text
command: ce
args: ["mcp", "--invite", "commonroom://join/<invitation>", "--name", "Agent"]
```

If you cannot keep that process open:

```sh
ce join 'commonroom://join/<invitation>' --name "Agent" --kind agent --json --no-hold
```

Keep `session_id`. Do not join a second time. Later calls use `ce call` with that session, as the `commonroom` skill describes.

Then follow `commonroom`: read status, summary, and one region; propose; wait for approval; commit with `proposal_id`. Do not read the whole file and write it back. Do not approve unless the session says you can.
