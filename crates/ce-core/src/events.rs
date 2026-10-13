//! Canonical workspace events.
//!
//! The engine writes one row per successful domain transition. The row is the
//! retained log. Live subscribers hear the same value after that write commits.
//! Schema version 1 is stamped on every envelope this process emits. The
//! sidecar does not store a second copy of the version: every retained row was
//! written by this schema.

use serde::Serialize;
use serde_json::Value;

pub const EVENT_SCHEMA_VERSION: u32 = 1;
pub const MAX_EVENT_FIELD_CHARS: usize = 240;

pub const WORKSPACE_CREATED: &str = "workspace.created";
pub const WORKSPACE_STARTED: &str = "workspace.started";
pub const WORKSPACE_STOPPED: &str = "workspace.stopped";
pub const PARTICIPANT_JOINED: &str = "participant.joined";
pub const PARTICIPANT_LEFT: &str = "participant.left";
pub const SESSION_CREATED: &str = "session.created";
pub const SESSION_EXPIRED: &str = "session.expired";
pub const ARTIFACT_CHANGED: &str = "change.committed";
pub const PROPOSAL_CREATED: &str = "proposal.created";
pub const PROPOSAL_APPROVED: &str = "proposal.approved";
pub const PROPOSAL_REJECTED: &str = "proposal.rejected";
pub const PROPOSAL_COMMITTED: &str = "proposal.committed";
pub const PROPOSAL_CONFLICTED: &str = "proposal.conflicted";
pub const PROPOSAL_SUPERSEDED: &str = "proposal.superseded";
pub const LEASE_ACQUIRED: &str = "lease.acquired";
pub const LEASE_RELEASED: &str = "lease.released";
pub const LEASE_EXPIRED: &str = "lease.expired";
pub const INVITE_CREATED: &str = "invite.created";
pub const INVITE_REVOKED: &str = "invite.revoked";
pub const HANDOFF_CREATED: &str = "handoff.created";
pub const HANDOFF_ACCEPTED: &str = "handoff.accepted";
pub const HANDOFF_RESULT_READY: &str = "handoff.result_ready";
pub const HANDOFF_APPROVED: &str = "handoff.approved";
pub const HANDOFF_REJECTED: &str = "handoff.rejected";
pub const HANDOFF_CANCELLED: &str = "handoff.cancelled";
pub const TASK_CREATED: &str = "task.created";
pub const TASK_UPDATED: &str = "task.updated";

pub const KNOWN_EVENT_TYPES: &[&str] = &[
    WORKSPACE_CREATED,
    WORKSPACE_STARTED,
    WORKSPACE_STOPPED,
    PARTICIPANT_JOINED,
    PARTICIPANT_LEFT,
    SESSION_CREATED,
    SESSION_EXPIRED,
    ARTIFACT_CHANGED,
    PROPOSAL_CREATED,
    PROPOSAL_APPROVED,
    PROPOSAL_REJECTED,
    PROPOSAL_COMMITTED,
    PROPOSAL_CONFLICTED,
    PROPOSAL_SUPERSEDED,
    LEASE_ACQUIRED,
    LEASE_RELEASED,
    LEASE_EXPIRED,
    INVITE_CREATED,
    INVITE_REVOKED,
    HANDOFF_CREATED,
    HANDOFF_ACCEPTED,
    HANDOFF_RESULT_READY,
    HANDOFF_APPROVED,
    HANDOFF_REJECTED,
    HANDOFF_CANCELLED,
    TASK_CREATED,
    TASK_UPDATED,
];

const SECRET_KEYS: &[&str] = &[
    "token",
    "secret",
    "session_id",
    "uri",
    "content",
    "change",
    "authorization",
    "password",
    "cookie",
    "invite",
    "token_hash",
    "address",
    "base_url",
    "pid",
];

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub event_id: i64,
    pub event_type: String,
    pub schema_version: u32,
    pub workspace_id: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    pub payload: Value,
}

pub fn is_known_event_type(event_type: &str) -> bool {
    KNOWN_EVENT_TYPES.contains(&event_type)
}

pub fn is_newer(cursor: i64, event_id: i64) -> bool {
    event_id > cursor
}

pub fn compose_event(
    event_id: i64,
    event_type: impl Into<String>,
    created_at: impl Into<String>,
    payload: Value,
) -> Event {
    let workspace_id = payload
        .get("workspace_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let version = payload.get("version").and_then(Value::as_i64);
    let actor_id = payload
        .get("participant_id")
        .and_then(Value::as_str)
        .or_else(|| payload.get("reviewer_id").and_then(Value::as_str))
        .map(str::to_string);
    Event {
        event_id,
        event_type: event_type.into(),
        schema_version: EVENT_SCHEMA_VERSION,
        workspace_id,
        created_at: created_at.into(),
        version,
        actor_id,
        payload,
    }
}

pub fn redact_event_payload(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !is_secret_key(key));
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                let Some(child) = map.get_mut(&key) else {
                    continue;
                };
                if key == "summary" {
                    truncate_string(child, MAX_EVENT_FIELD_CHARS);
                }
                redact_event_payload(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_event_payload(item);
            }
        }
        Value::String(text) => scrub_string(text),
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    SECRET_KEYS
        .iter()
        .any(|secret| key.eq_ignore_ascii_case(secret))
}

fn truncate_string(value: &mut Value, max_chars: usize) {
    let Some(text) = value.as_str() else {
        return;
    };
    if text.chars().count() > max_chars {
        *value = Value::String(text.chars().take(max_chars).collect());
    }
}

fn scrub_string(text: &mut String) {
    if text.contains("commonroom://join/") || text.starts_with("ses_") {
        *text = "[redacted]".to_string();
        return;
    }
    if text.chars().count() > 512 {
        *text = text.chars().take(512).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redaction_removes_secrets_and_limits_summaries() {
        let mut payload = json!({
            "summary": "s".repeat(400),
            "token": "invite-token",
            "session_id": "ses_secret",
            "nested": {"secret": "hidden", "proposal_id": "proposal_1"},
            "note": "commonroom://join/abc"
        });
        redact_event_payload(&mut payload);
        let blob = payload.to_string();
        assert!(payload.get("token").is_none());
        assert!(payload.get("session_id").is_none());
        assert!(payload["nested"].get("secret").is_none());
        assert_eq!(payload["nested"]["proposal_id"], "proposal_1");
        assert!(payload["summary"].as_str().unwrap().chars().count() <= MAX_EVENT_FIELD_CHARS);
        assert!(!blob.contains("invite-token"));
        assert!(!blob.contains("commonroom://join/"));
        assert!(!blob.contains("ses_secret"));
    }

    #[test]
    fn newer_events_move_the_cursor_and_duplicates_do_not() {
        assert!(is_newer(4, 5));
        assert!(!is_newer(5, 5));
        assert!(!is_newer(6, 5));
    }

    #[test]
    fn envelope_uses_schema_version_one_and_skips_missing_actor() {
        let event = compose_event(
            3,
            PROPOSAL_APPROVED,
            "2026-10-12T00:00:00.000000+00:00",
            json!({"workspace_id": "workspace_a", "proposal_id": "proposal_1", "version": 2}),
        );
        assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(event.workspace_id, "workspace_a");
        assert_eq!(event.version, Some(2));
        assert!(event.actor_id.is_none());
        assert!(is_known_event_type(&event.event_type));
    }
}
