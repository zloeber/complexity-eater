# Sidecar storage

This note asks whether the room should keep its metadata in the SQLite file beside the markdown, or add a second database for review questions.

## Decision

Keep the sidecar in SQLite.

Questions, answers, and comments are rows in that file. The markdown stays the source. A proposed edit is stored as a proposal and does not replace the passage until it is approved and committed.

| Choice | What it keeps |
| --- | --- |
| SQLite sidecar | One writer, one file next to the document |
| Separate service | Another store and another failure mode |

## Risk

A second database would split the room.

The engine is already the only writer. Review state that lives somewhere else can drift from the version the question was anchored to. If a quote disappears, the question should say so instead of sticking to a different sentence.

## Open questions

- Who is allowed to answer without also proposing a document change?
- When is a review complete enough to hand back to the agent?
