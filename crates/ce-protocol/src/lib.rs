//! Wire protocol and shared error vocabulary for Complexity Eater.
//!
//! Identifiers that already exist on the wire stay `commonroom`. The product
//! and the executable are Complexity Eater and `ce`. See `docs/migration/decisions.md`.

use std::fmt;

use base64::Engine;
use serde_json::{Map, Value};

pub const PROTOCOL_NAME: &str = "commonroom";
pub const PROTOCOL_VERSION: &str = "1";
pub const INVITATION_PREFIX: &str = "commonroom://join/";
pub const MAX_INVITATION_BYTES: usize = 8_192;
pub const MCP_PROTOCOL_VERSION: &str = "2026-07-28";
pub const SKILL_URI: &str = "skill://commonroom/SKILL.md";
pub const PRODUCT_NAME: &str = "Complexity Eater";
pub const CLI_NAME: &str = "ce";

pub const CAPABILITIES: &[&str] = &[
    "read", "observe", "propose", "answer", "comment", "checkout", "commit", "approve", "invite",
    "admin",
];

pub const DEFAULT_AGENT_CAPABILITIES: &[&str] =
    &["read", "observe", "propose", "checkout", "commit"];

const RECOVERY_FIELDS: &[&str] = &[
    "base_version",
    "current_version",
    "expected_version",
    "diff",
    "changes",
    "retry_after_seconds",
    "lease_id",
    "proposal_id",
    "region",
    "candidates",
    "limit",
    "size",
    "holder",
    "expires_at",
    "baton_id",
    "mode",
    "renewals",
    "max_renewals",
];

pub const ERROR_CODES: &[&str] = &[
    "unauthorized",
    "permission_denied",
    "invitation_expired",
    "invitation_revoked",
    "invitation_redeemed",
    "unsupported_version",
    "session_expired",
    "lease_conflict",
    "stale_version",
    "proposal_not_found",
    "proposal_conflict",
    "invalid_region",
    "version_not_found",
    "transport_unavailable",
    "limit_exceeded",
    "invalid_request",
    "baton_held",
    "baton_required",
    "baton_expired",
    "baton_renewal_limit",
    "mode_mismatch",
    "question_not_found",
];

#[derive(Clone, Debug)]
pub struct Operation {
    pub name: &'static str,
    pub http: &'static str,
    pub capability: &'static str,
}

pub const OPERATIONS: &[Operation] = &[
    Operation {
        name: "workspace.status",
        http: "GET /workspace/status",
        capability: "read",
    },
    Operation {
        name: "workspace.summary",
        http: "GET /workspace/summary",
        capability: "read",
    },
    Operation {
        name: "workspace.regions",
        http: "GET /workspace/regions",
        capability: "read",
    },
    Operation {
        name: "workspace.read",
        http: "GET /workspace/read",
        capability: "read",
    },
    Operation {
        name: "workspace.diff",
        http: "GET /workspace/diff",
        capability: "read",
    },
    Operation {
        name: "workspace.changes_since",
        http: "GET /workspace/changes_since",
        capability: "read",
    },
    Operation {
        name: "workspace.observe",
        http: "GET /workspace/observe",
        capability: "observe",
    },
    Operation {
        name: "workspace.checkout",
        http: "POST /workspace/checkout",
        capability: "checkout",
    },
    Operation {
        name: "workspace.release",
        http: "POST /workspace/release",
        capability: "checkout",
    },
    Operation {
        name: "workspace.renew",
        http: "POST /workspace/renew",
        capability: "checkout",
    },
    Operation {
        name: "workspace.propose",
        http: "POST /workspace/propose",
        capability: "propose",
    },
    Operation {
        name: "workspace.review",
        http: "POST /workspace/proposal/review",
        capability: "approve",
    },
    Operation {
        name: "workspace.commit",
        http: "POST /workspace/commit",
        capability: "commit",
    },
    Operation {
        name: "workspace.participants",
        http: "GET /workspace/participants",
        capability: "read",
    },
    Operation {
        name: "workspace.leases",
        http: "GET /workspace/leases",
        capability: "read",
    },
    Operation {
        name: "workspace.proposals",
        http: "GET /workspace/proposals",
        capability: "read",
    },
    Operation {
        name: "workspace.handoffs",
        http: "GET /workspace/handoffs",
        capability: "read",
    },
    Operation {
        name: "workspace.handoff",
        http: "GET /workspace/handoff",
        capability: "read",
    },
    Operation {
        name: "workspace.handoff_events",
        http: "GET /workspace/handoff/events",
        capability: "read",
    },
    Operation {
        name: "workspace.handoff_create",
        http: "POST /workspace/handoff/create",
        capability: "propose",
    },
    Operation {
        name: "workspace.handoff_accept",
        http: "POST /workspace/handoff/accept",
        capability: "commit",
    },
    Operation {
        name: "workspace.handoff_complete",
        http: "POST /workspace/handoff/complete",
        capability: "commit",
    },
    Operation {
        name: "workspace.handoff_approve",
        http: "POST /workspace/handoff/approve",
        capability: "approve",
    },
    Operation {
        name: "workspace.task",
        http: "GET /workspace/task",
        capability: "read",
    },
    Operation {
        name: "workspace.task_cancel",
        http: "POST /workspace/task/cancel",
        capability: "read",
    },
    Operation {
        name: "workspace.history",
        http: "GET /workspace/history",
        capability: "read",
    },
    Operation {
        name: "workspace.diagnostics",
        http: "GET /workspace/diagnostics",
        capability: "read",
    },
    Operation {
        name: "workspace.invite",
        http: "POST /workspace/invite",
        capability: "invite",
    },
    Operation {
        name: "workspace.leave",
        http: "POST /workspace/leave",
        capability: "read",
    },
    Operation {
        name: "workspace.mode",
        http: "POST /workspace/mode",
        capability: "admin",
    },
    Operation {
        name: "workspace.baton_request",
        http: "POST /workspace/baton/request",
        capability: "commit",
    },
    Operation {
        name: "workspace.baton_renew",
        http: "POST /workspace/baton/renew",
        capability: "commit",
    },
    Operation {
        name: "workspace.baton_release",
        http: "POST /workspace/baton/release",
        capability: "commit",
    },
    Operation {
        name: "workspace.baton_status",
        http: "GET /workspace/baton",
        capability: "read",
    },
    Operation {
        name: "workspace.baton_revoke",
        http: "POST /workspace/baton/revoke",
        capability: "admin",
    },
    Operation {
        name: "workspace.write",
        http: "POST /workspace/write",
        capability: "commit",
    },
    Operation {
        name: "workspace.revert",
        http: "POST /workspace/revert",
        capability: "admin",
    },
    Operation {
        name: "workspace.review_context",
        http: "GET /workspace/review",
        capability: "read",
    },
    Operation {
        name: "workspace.review_export",
        http: "GET /workspace/review/export",
        capability: "read",
    },
    Operation {
        name: "workspace.question_create",
        http: "POST /workspace/review/questions",
        capability: "propose",
    },
    Operation {
        name: "workspace.question_update",
        http: "POST /workspace/review/questions/update",
        capability: "propose",
    },
    Operation {
        name: "workspace.question_answer",
        http: "POST /workspace/review/answer",
        capability: "answer",
    },
    Operation {
        name: "workspace.comment",
        http: "POST /workspace/review/comment",
        capability: "comment",
    },
    Operation {
        name: "workspace.question_status",
        http: "POST /workspace/review/question/status",
        capability: "propose",
    },
    Operation {
        name: "workspace.review_status",
        http: "POST /workspace/review/status",
        capability: "approve",
    },
    Operation {
        name: "session.join",
        http: "POST /session/join",
        capability: "invitation",
    },
];

pub const WORKFLOW: &[&str] = &[
    "session.join",
    "workspace.status",
    "workspace.summary",
    "workspace.read",
    "workspace.checkout",
    "workspace.propose",
    "workspace.review",
    "workspace.commit",
    "workspace.release",
    "workspace.question_create",
    "workspace.question_answer",
    "workspace.review_context",
];

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_request_bytes: usize,
    pub max_proposal_bytes: usize,
    pub max_active_sessions: usize,
    pub max_events: usize,
    pub max_lease_minutes: i64,
    pub default_lease_minutes: i64,
    pub default_invite_minutes: i64,
    pub max_invite_minutes: i64,
    pub default_session_minutes: i64,
    pub max_session_minutes: i64,
    pub max_requests_per_minute: usize,
    pub max_list_items: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_bytes: 1_048_576,
            max_proposal_bytes: 262_144,
            max_active_sessions: 32,
            max_events: 2_000,
            max_lease_minutes: 120,
            default_lease_minutes: 15,
            default_invite_minutes: 60,
            max_invite_minutes: 60 * 24 * 7,
            default_session_minutes: 8 * 60,
            max_session_minutes: 24 * 60,
            max_requests_per_minute: 600,
            max_list_items: 200,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommonroomError {
    pub code: &'static str,
    pub http_status: u16,
    pub message: String,
    pub details: Map<String, Value>,
}

impl CommonroomError {
    pub fn new(code: &'static str, http_status: u16, message: impl Into<String>) -> Self {
        Self {
            code,
            http_status,
            message: message.into(),
            details: Map::new(),
        }
    }

    pub fn detail(mut self, key: impl Into<String>, value: impl serde::Serialize) -> Self {
        let value = serde_json::to_value(value).unwrap_or(Value::Null);
        self.details.insert(key.into(), value);
        self
    }

    pub fn to_value(&self) -> Value {
        let mut body = Map::new();
        body.insert("error".into(), Value::String(self.code.to_string()));
        body.insert("message".into(), Value::String(self.message.clone()));
        for (key, value) in &self.details {
            if RECOVERY_FIELDS.contains(&key.as_str()) {
                body.insert(key.clone(), value.clone());
            }
        }
        if !self.details.is_empty() {
            body.insert("details".into(), Value::Object(self.details.clone()));
        }
        Value::Object(body)
    }
}

impl fmt::Display for CommonroomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CommonroomError {}

pub fn unauthorized(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("unauthorized", 401, message)
}

pub fn permission_denied(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("permission_denied", 403, message)
}

pub fn invitation_expired() -> CommonroomError {
    CommonroomError::new("invitation_expired", 403, "invite expired")
}

pub fn invitation_revoked() -> CommonroomError {
    CommonroomError::new("invitation_revoked", 403, "invite revoked")
}

pub fn invitation_redeemed() -> CommonroomError {
    CommonroomError::new("invitation_redeemed", 403, "invite already redeemed")
}

pub fn unsupported_version(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("unsupported_version", 400, message)
}

pub fn session_expired() -> CommonroomError {
    CommonroomError::new("session_expired", 401, "session expired")
}

pub fn lease_conflict(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("lease_conflict", 409, message)
}

pub fn stale_version(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("stale_version", 409, message)
}

pub fn proposal_not_found(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("proposal_not_found", 404, message)
}

pub fn proposal_conflict(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("proposal_conflict", 409, message)
}

pub fn invalid_region(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("invalid_region", 400, message)
}

pub fn version_not_found(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("version_not_found", 404, message)
}

pub fn transport_unavailable(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("transport_unavailable", 503, message)
}

pub fn limit_exceeded(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("limit_exceeded", 429, message)
}

pub fn invalid_request(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("invalid_request", 400, message)
}

pub fn baton_held(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("baton_held", 409, message)
}

pub fn baton_required(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("baton_required", 409, message)
}

pub fn baton_expired(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("baton_expired", 409, message)
}

pub fn baton_renewal_limit(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("baton_renewal_limit", 409, message)
}

pub fn mode_mismatch(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("mode_mismatch", 409, message)
}

pub fn question_not_found(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("question_not_found", 404, message)
}

pub fn internal_error(message: impl Into<String>) -> CommonroomError {
    CommonroomError::new("error", 500, message)
}

pub fn normalize_capabilities(values: &[String]) -> Result<Vec<String>, CommonroomError> {
    let mut cleaned = Vec::new();
    for value in values {
        let item = value.trim();
        if item.is_empty() {
            continue;
        }
        if !CAPABILITIES.contains(&item) && item != "*" {
            return Err(invalid_request(format!("unknown capability '{item}'")));
        }
        if !cleaned.iter().any(|existing: &String| existing == item) {
            cleaned.push(item.to_string());
        }
    }
    if cleaned.iter().any(|item| item == "*") {
        return Ok(vec!["*".to_string()]);
    }
    Ok(cleaned)
}

pub fn capabilities_from_permissions(
    permissions: Option<&str>,
) -> Result<Vec<String>, CommonroomError> {
    let Some(permissions) = permissions else {
        return Ok(DEFAULT_AGENT_CAPABILITIES
            .iter()
            .map(|item| (*item).to_string())
            .collect());
    };
    match permissions {
        "read_write" => Ok(DEFAULT_AGENT_CAPABILITIES
            .iter()
            .map(|item| (*item).to_string())
            .collect()),
        "read" => Ok(vec!["read".into(), "observe".into()]),
        "admin" | "*" => Ok(vec!["*".into()]),
        other => normalize_capabilities(
            &other
                .split(',')
                .map(|item| item.to_string())
                .collect::<Vec<_>>(),
        ),
    }
}

pub fn has_capability(granted: &[String], required: &str) -> bool {
    granted.iter().any(|item| item == "*" || item == "admin")
        || granted.iter().any(|item| item == required)
}

pub fn discovery_document(workspace_id: &str) -> Value {
    serde_json::json!({
        "name": "Commonroom",
        "protocol": PROTOCOL_NAME,
        "protocol_version": PROTOCOL_VERSION,
        "workspace_id": workspace_id,
        "discovery": {
            "protocol": "/protocol",
            "skills": "/skills",
            "skill": "/skills/commonroom",
            "join": "/session/join",
        },
        "operations": OPERATIONS.iter().map(|item| item.name).collect::<Vec<_>>(),
        "workflow": WORKFLOW,
        "errors": ERROR_CODES,
    })
}

pub fn protocol_document() -> Value {
    let operations: Vec<Value> = OPERATIONS
        .iter()
        .map(|item| {
            serde_json::json!({
                "name": item.name,
                "http": item.http,
                "capability": item.capability,
            })
        })
        .collect();
    serde_json::json!({
        "protocol": PROTOCOL_NAME,
        "version": PROTOCOL_VERSION,
        "operations": operations,
        "errors": ERROR_CODES,
        "workflow": WORKFLOW,
        "auth": {
            "header": "Authorization: Bearer <session_id>",
            "join": "POST /session/join",
            "note": "A transport endpoint does not authorize workspace operations. Join with an invitation to receive a session.",
        },
        "bounds": ["max_bytes", "max_lines", "max_items", "max_depth"],
        "transports": {
            "entry_point_group": "commonroom.transports",
            "methods": ["listen", "connect", "accept", "send", "receive", "close"],
            "note": "Invitations carry opaque transport hints. The workspace engine does not import transport plugins.",
        },
        "artifacts": {
            "note": "Artifact bytes move through the active transport. There is no second network stack.",
            "operations": ["workspace.read", "workspace.commit"],
        },
    })
}

pub fn encode_invitation(document: &Value) -> Result<String, CommonroomError> {
    let hints = invitation_hint_list(document)?;
    let first = hints
        .first()
        .cloned()
        .ok_or_else(|| invalid_request("invitation missing transport"))?;
    let mut payload = Map::new();
    payload.insert(
        "capabilities".into(),
        document.get("capabilities").cloned().unwrap_or(Value::Null),
    );
    payload.insert(
        "expires_at".into(),
        document.get("expires_at").cloned().unwrap_or(Value::Null),
    );
    payload.insert("protocol".into(), Value::String(PROTOCOL_NAME.into()));
    payload.insert(
        "secret".into(),
        document.get("secret").cloned().unwrap_or(Value::Null),
    );
    payload.insert("transport".into(), first);
    payload.insert("transports".into(), Value::Array(hints));
    payload.insert("version".into(), Value::String(PROTOCOL_VERSION.into()));
    payload.insert(
        "workspace_id".into(),
        document.get("workspace_id").cloned().unwrap_or(Value::Null),
    );
    let raw = serde_json::to_vec(&Value::Object(payload))
        .map_err(|err| invalid_request(err.to_string()))?;
    if raw.len() > MAX_INVITATION_BYTES {
        return Err(invalid_request("invitation too large"));
    }
    let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw);
    Ok(format!("{INVITATION_PREFIX}{token}"))
}

fn invitation_hint_list(document: &Value) -> Result<Vec<Value>, CommonroomError> {
    if let Some(list) = document.get("transports").and_then(Value::as_array) {
        if !list.is_empty() {
            return Ok(list.clone());
        }
    }
    if let Some(hint) = document.get("transport") {
        if hint.is_object() {
            return Ok(vec![hint.clone()]);
        }
    }
    Err(invalid_request("invitation missing transport"))
}

pub fn decode_invitation(value: &str) -> Result<Value, CommonroomError> {
    let text = value.trim();
    let document: Value = if text.starts_with('{') {
        serde_json::from_str(text)
            .map_err(|_| invalid_request("invitation is not a commonroom capability"))?
    } else {
        let encoded = text.strip_prefix(INVITATION_PREFIX).unwrap_or(text);
        let padding = "=".repeat((4 - (encoded.len() % 4)) % 4);
        let raw = base64::engine::general_purpose::URL_SAFE
            .decode(format!("{encoded}{padding}"))
            .map_err(|_| invalid_request("invitation is not a commonroom capability"))?;
        if raw.len() > MAX_INVITATION_BYTES {
            return Err(invalid_request("invitation too large"));
        }
        serde_json::from_slice(&raw)
            .map_err(|_| invalid_request("invitation is not a commonroom capability"))?
    };
    let protocol = document
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or("");
    if protocol != PROTOCOL_NAME {
        return Err(invalid_request("invitation protocol is not commonroom"));
    }
    let version = match document.get("version") {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(other) => other.to_string().trim_matches('"').to_string(),
        None => String::new(),
    };
    if version != PROTOCOL_VERSION {
        return Err(invalid_request("unsupported invitation version"));
    }
    for key in [
        "transport",
        "workspace_id",
        "secret",
        "expires_at",
        "capabilities",
    ] {
        if document.get(key).is_none() {
            return Err(invalid_request(format!("invitation missing {key}")));
        }
    }
    Ok(document)
}

pub fn paste_message(uri: &str) -> String {
    format!(
        "Add this local MCP server, then use its Complexity Eater tools:\n\ncommand: {CLI_NAME}\nargs: [\"mcp\", \"--invite\", \"{uri}\"]\n\nThe invitation works once. Leave this server running and keep using it.\nRead the commonroom skill, or the workspace_onboarding prompt, before editing.\nPropose changes. Approval stays with the workspace owner.\n"
    )
}

pub fn is_remote_invitation(value: &str) -> bool {
    let text = value.trim();
    text.starts_with("commonroom://") || text.starts_with('{')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_round_trip_sorts_keys_and_keeps_the_prefix() {
        let hint = serde_json::json!({
            "type": "loopback",
            "address": "127.0.0.1",
            "port": 9,
            "encrypted": false,
            "mode": "local"
        });
        let uri = encode_invitation(&serde_json::json!({
            "transport": hint,
            "workspace_id": "workspace_abc",
            "secret": "secret-value",
            "expires_at": "2099-01-01T00:00:00+00:00",
            "capabilities": ["read", "observe"]
        }))
        .unwrap();
        assert!(uri.starts_with(INVITATION_PREFIX));
        let decoded = decode_invitation(&uri).unwrap();
        assert_eq!(decoded["protocol"], "commonroom");
        assert_eq!(decoded["version"], "1");
        assert_eq!(decoded["secret"], "secret-value");
        assert_eq!(decoded["transport"]["port"], 9);
        assert_eq!(decoded["transports"][0]["type"], "loopback");
    }

    #[test]
    fn unsupported_version_and_bad_token_use_stable_messages() {
        let err = decode_invitation("commonroom://join/not-valid!!!").unwrap_err();
        assert_eq!(err.message, "invitation is not a commonroom capability");
        let err = decode_invitation(r#"{"protocol":"commonroom","version":"9","transport":{},"workspace_id":"w","secret":"s","expires_at":"t","capabilities":[]}"#).unwrap_err();
        assert_eq!(err.message, "unsupported invitation version");
    }

    #[test]
    fn star_and_admin_cover_every_capability() {
        assert!(has_capability(&["*".into()], "approve"));
        assert!(has_capability(&["admin".into()], "invite"));
        assert!(!has_capability(
            &["read".into(), "commit".into()],
            "approve"
        ));
    }

    #[test]
    fn legacy_permission_names_map_to_capabilities() {
        assert_eq!(
            capabilities_from_permissions(Some("read_write")).unwrap(),
            DEFAULT_AGENT_CAPABILITIES
                .iter()
                .map(|item| (*item).to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            capabilities_from_permissions(Some("read")).unwrap(),
            vec!["read".to_string(), "observe".to_string()]
        );
    }
}
