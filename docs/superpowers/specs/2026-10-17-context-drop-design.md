# Context drop

A drop is a Complexity Eater room whose pickup window is separate from the later session. One agent leaves a markdown file for another. The sender's harness starts the room in the background and holds a local watch until the file is read or the window ends with no read. After a read, the same process stays up for ordinary room work until a participant closes it or it sits idle.

This is not a hosted workspace, not an offline package, and not a second protocol. `ce serve` does not change.

## Scenario index

`skills/commonroom/AGENT.md` opens with this index, before the install steps. The public page `https://complexityeater.com/agent` is the filled form of that file. A person who already has `ce` follows the index and does not repeat installation.

| Request | Skill |
| --- | --- |
| Install or update Complexity Eater | `complexity-eater-agent` |
| Open a live room and stay with it | `complexity-eater-adopt` |
| Leave markdown for another agent to pick up | `complexity-eater-drop` |
| Join an invitation, propose, or commit | `commonroom` |
| Comment, answer, or decide on a proposal | `complexity-eater-review` |
| Ask an agent already in the room to do work, or resume that work | `complexity-eater-handoff` |
| Find out why a connection failed | `complexity-eater-connect` |

`complexity-eater-drop` ships in the same change as `ce drop`. It owns starting the process, holding the watch, the pickup window, close, and idle. After a pickup, `commonroom` covers reads and proposals. `complexity-eater-handoff` covers asking the other agent to do work. "Send this to another agent" can match both the drop skill and the handoff skill. The drop skill applies when no room exists yet. The handoff skill applies when a room is already running. Neither skill approves a proposal.

The room still serves only `skill://commonroom/SKILL.md`. The other skills are installed locally from `/agent`.

## Command

```text
ce drop <file> --name <name> [--for <minutes>] [--pickups <n>] [--idle <minutes>] [--ephemeral] [--tailcat] [--json]
```

Defaults: `--for 60`, `--pickups 1`, `--idle 15`. `--for` is at least 1 and at most 10080 (7 days, the existing invitation cap). `--pickups` is at least 1. `--idle 0` disables idle shutdown. `--ephemeral` deletes the temporary sidecar on exit and leaves the markdown file. `--tailcat` is the existing optional plugin and still requires the person to have agreed; a missing plugin is `transport_unavailable` and does not open a public listener.

`ce drop` is the parent watch. It spawns the current executable as the room child. The child binds the same two loopback sockets as `ce serve`: an owner port and a session ingress. Stderr of the parent prints the existing `Human:` and `Invite:` banner. Stdout is two JSON lines and nothing else:

```json
{"event":"started","pid":12345,"invite":"commonroom://join/…","human":"http://127.0.0.1:…/?token=…"}
{"event":"picked_up","participant":"Ada","at":"…","pickups":1,"pickups_remaining":0}
```

The receipt omits the invitation and the document body. `pickups_remaining` is how many new sessions may still read.

Other terminal events, instead of `picked_up`:

| Event | When | Parent exit | Child |
| --- | --- | --- | --- |
| `picked_up` | First session receives document bytes | 0 | Keeps running |
| `expired` | Window ends with zero pickups | 2 | Exits |
| `closed` | Closed before any pickup | 3 | Exits |

If the parent dies before a terminal event, the child exits and does not leave a room behind. After `picked_up`, the parent exiting does not stop the child. The watch pipe is loopback-only in spirit: it is an inherited pipe, not a published socket, not a tunnel, and not an invitation.

`ce drop close --workspace <file>` asks the running drop to shut down. Inside the room, `POST /workspace/drop/close` and MCP tool `drop_close` do the same. The host session may close. A participant session may close only after its pickup row exists. Any other session gets `permission_denied`. Close is not `admin`.

## Pickup window

The window closes when `now` reaches the deadline or the pickup count is reached, whichever comes first. Closing the window stops new pickups. It does not stop a session that already has a pickup row.

A pickup is the first time one participant session is about to receive document bytes from `workspace_read` (including a bounded excerpt) or `review_section`. The engine records the row before those bytes are returned. Status, summary, and region lists do not pick up. The owner page does not pick up. A session whose capabilities include `*` does not pick up. A second read by the same session does not pick up again.

Joining authenticates and does not increment `invites.used_count` when a `drop_state` row exists. A failed dial spends nothing, as it does today. New joins after the deadline return `invitation_expired`. New joins after the count is spent return `drop_closed` with `details.reason` of `exhausted`. A session with no pickup row that tries to read after the window closed gets `drop_closed` and no body. `details.reason` is `expired`, `exhausted`, or `closed`.

A session that already picked up keeps using its `session_id` after the window closes. Reconnecting does not need a new invitation. A new join still fails.

Idle timing starts at the first pickup, not at process start. It resets when a room event is appended or a participant document read succeeds. When the idle duration passes, the child exits. The markdown file stays. `--ephemeral` still removes only the temporary sidecar directory.

After the watch exits on `picked_up`, proposals, review, and handoffs behave as they do in any other room. A default invitation still grants `read`, `observe`, `propose`, `checkout`, and `commit`, and still does not grant `approve`, `invite`, or `admin`.

## Storage

The sidecar gains:

```sql
CREATE TABLE IF NOT EXISTS drop_state (
    workspace_id TEXT PRIMARY KEY,
    deadline TEXT NOT NULL,
    max_pickups INTEGER NOT NULL,
    idle_seconds INTEGER NOT NULL,
    idle_started_at TEXT,
    last_activity_at TEXT,
    closed_at TEXT,
    close_reason TEXT
);
CREATE TABLE IF NOT EXISTS drop_pickups (
    session_id TEXT PRIMARY KEY,
    participant_name TEXT NOT NULL,
    picked_at TEXT NOT NULL
);
```

`idle_seconds` of 0 means idle shutdown is off. A normal `ce serve` room has no `drop_state` row and keeps today's invitation rules. Pickup and close append ordinary rows to `events`.

`drop_closed` is a new code in `ERROR_CODES`, HTTP 403, alongside the existing constructors. `details.reason` is one of `expired`, `exhausted`, `closed`.

## Tests

Engine and HTTP tests, not a manual script:

- Summary does not insert a pickup. The first `workspace_read` does. The owner path does not. The same session reading again does not.
- A second participant is refused at `--pickups 1` with `drop_closed` / `exhausted` and the body is absent.
- Join does not increment `used_count` for a drop. A non-drop invitation still spends a use on join.
- An unread deadline stops the child and the parent exits 2.
- A deadline after a pickup leaves the child running and refuses a new join with `invitation_expired`.
- Idle does not start before the first pickup.
- A joined session with no pickup row cannot close. The host can. A session with a pickup row can.
- `ce-core`, `ce-http`, and `ce-protocol` still do not name `tailcat`, `libp2p`, `iroh`, or `webrtc`.

## Out of scope

Hosted drop provisioning, offline packages, a watch carried by Tailcat or Cloudflare, commit buttons on the review view, and any change to default invitation capabilities.
