use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use base64::Engine;
use ce_core::{
    load_skill, Event, NewHandoff, Session, TaskView, WorkspaceEngine, TASK_CREATED, TASK_UPDATED,
};
use ce_protocol::{
    has_capability, invalid_request, limit_exceeded, permission_denied, unauthorized,
    CommonroomError, MCP_PROTOCOL_VERSION, SKILL_URI,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::notify::{
    event_stream, message_frame, resources_for_event, subscribable_resource, workspace_resources,
    NotifyHub, RESOURCE_DOCUMENT, RESOURCE_LEASES, RESOURCE_PARTICIPANTS, RESOURCE_PROPOSALS,
    RESOURCE_REVIEW, RESOURCE_STATUS,
};

const HEADER_MISMATCH: i64 = -32020;
const MISSING_CAPABILITY: i64 = -32021;
const UNSUPPORTED_VERSION: i64 = -32022;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const APPLICATION_ERROR: i64 = -32000;
const TASKS_EXTENSION: &str = "io.modelcontextprotocol/tasks";
const UI_EXTENSION: &str = "io.modelcontextprotocol/ui";
const UI_MIME: &str = "text/html;profile=mcp-app";
const REVIEW_APP_URI: &str = "ui://commonroom/review/1";
const REVIEW_APP_HTML: &str = include_str!("review_app.html");

struct Tool {
    name: &'static str,
    capability: &'static str,
    also: &'static [&'static str],
    description: &'static str,
    schema: Value,
}

pub fn handle_mcp(
    engine: &Arc<WorkspaceEngine>,
    hub: &Arc<NotifyHub>,
    headers: &HeaderMap,
    body: &[u8],
) -> Response {
    if let Some(origin) = headers.get("origin").and_then(|value| value.to_str().ok()) {
        if !origin_allowed(origin) {
            return rpc_error(Value::Null, -32600, "origin not allowed", 403, None);
        }
    }
    let message: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return rpc_error(Value::Null, -32700, "parse error", 400, None),
    };
    if !message.is_object() {
        return rpc_error(Value::Null, -32600, "invalid request", 400, None);
    }
    let request_id = message.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return rpc_error(request_id, -32600, "invalid request", 400, None);
    };
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    if message.get("id").is_none() || message.get("id") == Some(&Value::Null) {
        if method == "notifications/cancelled" {
            if let Some(request_id) = params.get("requestId") {
                hub.cancel_listen(&listen_key(request_id));
            }
        }
        return StatusCode::ACCEPTED.into_response();
    }
    if !params.is_object() {
        return rpc_error(request_id, INVALID_PARAMS, "invalid params", 400, None);
    }
    if let Some(mismatch) = header_mismatch(headers, method, &params) {
        return rpc_error(request_id, HEADER_MISMATCH, mismatch, 400, None);
    }
    let version = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if version != MCP_PROTOCOL_VERSION {
        return rpc_error(
            request_id,
            UNSUPPORTED_VERSION,
            "UnsupportedProtocolVersionError",
            400,
            Some(json!({"supportedVersions": [MCP_PROTOCOL_VERSION], "requested": version})),
        );
    }
    let protected = matches!(
        method,
        "tools/list"
            | "tools/call"
            | "prompts/list"
            | "prompts/get"
            | "resources/list"
            | "resources/read"
            | "resources/subscribe"
            | "resources/unsubscribe"
            | "skills/list"
            | "skills/get"
            | "tasks/get"
            | "tasks/update"
            | "tasks/cancel"
            | "subscriptions/listen"
    );
    let session = if protected {
        match session_from(engine, headers) {
            Ok(session) => Some(session),
            Err(err) => {
                return rpc_error(
                    request_id,
                    APPLICATION_ERROR,
                    &err.message,
                    err.http_status,
                    Some(err.to_value()),
                );
            }
        }
    } else {
        None
    };
    let result = match method {
        "server/discover" => Ok(discover(engine)),
        "tools/list" => Ok(tools_list(session.as_ref(), &params)),
        "tools/call" => tools_call(engine, session.as_ref(), &params),
        "prompts/list" => Ok(prompts_list()),
        "prompts/get" => prompts_get(&params),
        "resources/list" => Ok(resources_list(session.as_ref())),
        "resources/read" => resources_read(engine, session.as_ref(), &params),
        "resources/subscribe" => resources_subscribe(hub, session.as_ref(), &params),
        "resources/unsubscribe" => resources_unsubscribe(hub, session.as_ref(), &params),
        "skills/list" => Ok(skills_list()),
        "skills/get" => skills_get(&params),
        "tasks/get" => tasks_get(engine, session.as_ref(), &params),
        "tasks/update" => tasks_update(engine, session.as_ref(), &params),
        "tasks/cancel" => tasks_cancel(engine, session.as_ref(), &params),
        "subscriptions/listen" => {
            return subscriptions_listen(engine, hub, session.as_ref(), &request_id, &params);
        }
        _ => Err(RpcError {
            code: METHOD_NOT_FOUND,
            message: "method not found".into(),
            status: 404,
            data: None,
        }),
    };
    match result {
        Ok(result) => {
            Json(json!({"jsonrpc": "2.0", "id": request_id, "result": result})).into_response()
        }
        Err(err) => rpc_error(request_id, err.code, &err.message, err.status, err.data),
    }
}

struct RpcError {
    code: i64,
    message: String,
    status: u16,
    data: Option<Value>,
}

fn origin_allowed(origin: &str) -> bool {
    let host = origin.split("://").nth(1).unwrap_or(origin);
    let host = host.split('/').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host == "127.0.0.1" || host == "localhost"
}

fn header_mismatch(headers: &HeaderMap, method: &str, params: &Value) -> Option<&'static str> {
    let meta = params.get("_meta").cloned().unwrap_or_else(|| json!({}));
    let body_version = meta
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str);
    let header_version = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok());
    if header_version.is_none() || header_version != body_version {
        return Some("HeaderMismatch");
    }
    let header_method = headers
        .get("mcp-method")
        .and_then(|value| value.to_str().ok());
    if header_method != Some(method) {
        return Some("HeaderMismatch");
    }
    let field = match method {
        "tools/call" | "prompts/get" => "name",
        "resources/read" | "resources/subscribe" | "resources/unsubscribe" => "uri",
        "tasks/get" | "tasks/update" | "tasks/cancel" => "taskId",
        _ => return None,
    };
    let Some(expected) = params.get(field).and_then(Value::as_str) else {
        return Some("HeaderMismatch");
    };
    let Some(presented) = headers
        .get("mcp-name")
        .and_then(|value| value.to_str().ok())
    else {
        return Some("HeaderMismatch");
    };
    if decode_header(presented) != expected {
        return Some("HeaderMismatch");
    }
    None
}

fn decode_header(value: &str) -> String {
    if let Some(encoded) = value
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
            if let Ok(text) = String::from_utf8(bytes) {
                return text;
            }
        }
    }
    value.to_string()
}

fn session_from(engine: &WorkspaceEngine, headers: &HeaderMap) -> Result<Session, CommonroomError> {
    let header = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !header.to_ascii_lowercase().starts_with("bearer ") {
        return Err(unauthorized("session required"));
    }
    let token = header
        .split_once(' ')
        .map(|(_, token)| token.trim())
        .unwrap_or("");
    if token.is_empty() {
        return Err(unauthorized("session required"));
    }
    engine.authenticate(token)
}

fn discover(engine: &WorkspaceEngine) -> Value {
    let mode = engine
        .status()
        .map(|status| status.mode)
        .unwrap_or_else(|_| "review".into());
    let instructions = if mode == "baton" {
        "Use the commonroom skill or the workspace_onboarding prompt before editing. This room is in baton mode. The holder writes with workspace_write. Other participants can still propose."
    } else {
        "Use the commonroom skill or the workspace_onboarding prompt before editing. Propose changes. Approval stays with the workspace owner. workspace_status reports the room mode."
    };
    json!({
        "resultType": "complete",
        "supportedVersions": [MCP_PROTOCOL_VERSION],
        "capabilities": {
            "tools": {},
            "resources": {"subscribe": true},
            "prompts": {},
            "extensions": {
                "io.modelcontextprotocol/skills": {},
                "io.modelcontextprotocol/tasks": {},
                "io.modelcontextprotocol/ui": {"mimeTypes": [UI_MIME]}
            }
        },
        "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "Commonroom", "version": env!("CARGO_PKG_VERSION")}},
        "instructions": instructions,
        "ttlMs": 3_600_000,
        "cacheScope": "public"
    })
}

fn tools() -> Vec<Tool> {
    vec![
        tool(
            "workspace_status",
            "read",
            "Workspace version and lifecycle.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_summary",
            "read",
            "Outline and a short excerpt.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_regions",
            "read",
            "Markdown regions in the workspace. Prefer full paths. Results are paged.",
            json!({"type": "object", "properties": {"depth": {"type": "integer"}, "under": {"type": "string", "description": "Only this region and its children."}, "cursor": {"type": "integer"}, "limit": {"type": "integer"}}}),
        ),
        tool(
            "workspace_read",
            "read",
            "Read one region, or a bounded excerpt.",
            json!({"type": "object", "properties": {"section": {"type": "string", "description": "Region path. A bare title that matches more than one heading is invalid_region."}, "max_lines": {"type": "integer"}, "max_bytes": {"type": "integer"}, "offset": {"type": "integer", "description": "Byte offset from a previous next_offset."}}}),
        ),
        tool(
            "workspace_diff",
            "read",
            "Unified diff since a version.",
            json!({"type": "object", "properties": {"base_version": {"type": "integer"}}, "required": ["base_version"]}),
        ),
        tool(
            "workspace_changes_since",
            "read",
            "Versions committed after a version.",
            json!({"type": "object", "properties": {"version": {"type": "integer"}}, "required": ["version"]}),
        ),
        tool(
            "workspace_participants",
            "read",
            "Who is in the room.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_leases",
            "read",
            "Active region leases.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_proposals",
            "read",
            "Proposals and their status.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_history",
            "read",
            "Recent room events.",
            json!({"type": "object", "properties": {"limit": {"type": "integer"}}}),
        ),
        tool(
            "workspace_diagnostics",
            "read",
            "Transport plugin name and mode, without the address.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_leave",
            "read",
            "Leave the room and release your leases.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_observe",
            "observe",
            "Events since an event id.",
            json!({"type": "object", "properties": {"since_event_id": {"type": "integer"}}}),
        ),
        tool(
            "workspace_checkout",
            "checkout",
            "Lease a region.",
            json!({"type": "object", "properties": {"target": {"type": "string"}, "minutes": {"type": "integer"}}, "required": ["target"]}),
        ),
        tool(
            "workspace_release",
            "checkout",
            "Release a lease you hold.",
            json!({"type": "object", "properties": {"lease_id": {"type": "string"}}, "required": ["lease_id"]}),
        ),
        tool(
            "workspace_renew",
            "checkout",
            "Extend a lease you hold.",
            json!({"type": "object", "properties": {"lease_id": {"type": "string"}, "minutes": {"type": "integer"}}, "required": ["lease_id"]}),
        ),
        tool(
            "workspace_propose",
            "propose",
            "Propose a change against a base version.",
            json!({"type": "object", "properties": {"target": {"type": "string", "description": "Region path. Use a full path when headings repeat."}, "base_version": {"type": "integer"}, "summary": {"type": "string"}, "change": {"type": "object", "description": "Replacement for the target region.", "properties": {"content": {"type": "string", "description": "New text for the target region, including its heading."}, "section": {"type": "string", "description": "Optional region. Must name the same region as target."}, "mode": {"type": "string", "enum": ["section", "document"], "description": "document replaces the whole file. The default edits the target region. target workspace also replaces the whole file."}}, "required": ["content"]}}, "required": ["target", "base_version", "summary", "change"]}),
        ),
        tool(
            "workspace_commit",
            "commit",
            "Commit an approved proposal. proposal_id is required.",
            json!({"type": "object", "properties": {"proposal_id": {"type": "string", "description": "Approved proposal to commit."}}, "required": ["proposal_id"]}),
        ),
        tool(
            "workspace_baton_request",
            "commit",
            "Request the editing baton in baton mode. A request above the cap is clamped. The same holder renews, up to max_renewals on workspace_status.",
            json!({"type": "object", "properties": {"minutes": {"type": "integer"}, "reason": {"type": "string"}}}),
        ),
        tool(
            "workspace_baton_renew",
            "commit",
            "Extend the baton you hold, to at most now plus the room maximum. baton_renewal_limit means this grant cannot be extended again.",
            json!({"type": "object", "properties": {"baton_id": {"type": "string"}, "minutes": {"type": "integer"}}, "required": ["baton_id"]}),
        ),
        tool(
            "workspace_baton_release",
            "commit",
            "Release the baton you hold.",
            json!({"type": "object", "properties": {"baton_id": {"type": "string"}}, "required": ["baton_id"]}),
        ),
        tool(
            "workspace_baton_status",
            "read",
            "Room mode, baton holder, expiry, and the maximum grant.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_write",
            "commit",
            "Write a region directly while you hold the editing baton. Uses the same change shape as workspace_propose.",
            json!({"type": "object", "properties": {"baton_id": {"type": "string"}, "target": {"type": "string"}, "base_version": {"type": "integer"}, "summary": {"type": "string"}, "change": {"type": "object", "properties": {"content": {"type": "string"}, "section": {"type": "string"}, "mode": {"type": "string"}}, "required": ["content"]}}, "required": ["baton_id", "target", "base_version", "summary", "change"]}),
        ),
        tool(
            "workspace_review",
            "approve",
            "Approve or reject a proposal.",
            json!({"type": "object", "properties": {"proposal_id": {"type": "string"}, "approve": {"type": "boolean"}}, "required": ["proposal_id", "approve"]}),
        ),
        tool(
            "handoff_create",
            "propose",
            "Create a handoff and a durable execution task.",
            json!({"type": "object", "properties": {"summary": {"type": "string"}, "constraints": {"type": "string"}, "artifact_refs": {"type": "array", "items": {"type": "string"}}, "idempotency_key": {"type": "string"}, "correlation_id": {"type": "string"}}, "required": ["summary"]}),
        ),
        tool(
            "handoff_list",
            "read",
            "List handoffs in this room.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "handoff_get",
            "read",
            "Read one handoff, including a retained result.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}}, "required": ["handoff_id"]}),
        ),
        tool(
            "handoff_accept",
            "commit",
            "Accept an open handoff and start its work.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}}, "required": ["handoff_id"]}),
        ),
        tool(
            "handoff_complete",
            "commit",
            "Record the result of a handoff you accepted.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}, "result": {"type": "string"}}, "required": ["handoff_id", "result"]}),
        ),
        tool(
            "handoff_request_input",
            "commit",
            "Pause a handoff you accepted until the owner answers.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}, "prompt": {"type": "string"}}, "required": ["handoff_id", "prompt"]}),
        ),
        tool(
            "handoff_events",
            "read",
            "Events for one handoff. This is the recovery log, not a replay of the live stream.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}}, "required": ["handoff_id"]}),
        ),
        tool(
            "handoff_approve",
            "approve",
            "Approve a handoff result. This does not change the execution task.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}}, "required": ["handoff_id"]}),
        ),
        tool(
            "handoff_reject",
            "approve",
            "Reject a handoff result. The execution task stays completed.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}}, "required": ["handoff_id"]}),
        ),
        tool(
            "review_context",
            "read",
            "Review questions, saved answers, source passages, and proposals. Proposals are not applied to the document.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "review_export",
            "read",
            "Export the review as json or markdown. This does not change the document.",
            json!({"type": "object", "properties": {"format": {"type": "string", "enum": ["json", "markdown"]}}}),
        ),
        tool(
            "review_question_create",
            "propose",
            "Ask a question anchored to a quote in one markdown region. kind is text, single, multi, yes_no, or approve_reject.",
            json!({"type": "object", "properties": {"parent_id": {"type": "string"}, "kind": {"type": "string", "enum": ["text", "single", "multi", "yes_no", "approve_reject"]}, "prompt": {"type": "string"}, "options": {"type": "array", "items": {"type": "string"}}, "required": {"type": "boolean"}, "region": {"type": "string"}, "quote": {"type": "string"}, "prefix": {"type": "string"}, "suffix": {"type": "string"}, "idempotency_key": {"type": "string"}}, "required": ["kind", "prompt"]}),
        ),
        tool(
            "review_question_update",
            "propose",
            "Update the prompt, options, or required flag of an open question you created.",
            json!({"type": "object", "properties": {"question_id": {"type": "string"}, "prompt": {"type": "string"}, "options": {"type": "array", "items": {"type": "string"}}, "required": {"type": "boolean"}}, "required": ["question_id"]}),
        ),
        tool_also(
            "review_answer",
            "answer",
            &["propose"],
            "Record an answer. propose also grants this. The answer does not change the document.",
            json!({"type": "object", "properties": {"question_id": {"type": "string"}, "value": {}, "rationale": {"type": "string"}}, "required": ["question_id", "value"]}),
        ),
        tool_also(
            "review_comment",
            "comment",
            &["propose"],
            "Add a comment on a question or region. propose also grants this.",
            json!({"type": "object", "properties": {"question_id": {"type": "string"}, "region": {"type": "string"}, "quote": {"type": "string"}, "body": {"type": "string"}}, "required": ["body"]}),
        ),
        tool_also(
            "review_question_status",
            "propose",
            &["approve"],
            "Set a question to open, needs_clarification, resolved, or rejected. Closing it requires approve.",
            json!({"type": "object", "properties": {"question_id": {"type": "string"}, "status": {"type": "string", "enum": ["open", "needs_clarification", "resolved", "rejected"]}}, "required": ["question_id", "status"]}),
        ),
        tool(
            "review_status",
            "approve",
            "Mark the review complete or returned. Reopening it requires admin.",
            json!({"type": "object", "properties": {"status": {"type": "string", "enum": ["open", "complete", "returned"]}}, "required": ["status"]}),
        ),
        tool(
            "review_open",
            "read",
            "Open a review. The result is a short brief. The document body is a later read. An optional note is shown to the person and is not stored.",
            json!({"type": "object", "properties": {"handoff_id": {"type": "string"}, "note": {"type": "string", "description": "Shown beside the document. Not stored and not a decision."}}}),
        ),
        tool(
            "review_section",
            "read",
            "Read one bounded region for the review view. The model-visible text is a pointer.",
            json!({"type": "object", "properties": {"section": {"type": "string"}, "offset": {"type": "integer"}, "max_bytes": {"type": "integer"}, "max_lines": {"type": "integer"}}}),
        ),
    ]
}

fn tool(
    name: &'static str,
    capability: &'static str,
    description: &'static str,
    schema: Value,
) -> Tool {
    tool_also(name, capability, &[], description, schema)
}

fn tool_also(
    name: &'static str,
    capability: &'static str,
    also: &'static [&'static str],
    description: &'static str,
    schema: Value,
) -> Tool {
    Tool {
        name,
        capability,
        also,
        description,
        schema,
    }
}

fn tool_allowed(granted: &[String], item: &Tool) -> bool {
    has_capability(granted, item.capability)
        || item.also.iter().any(|cap| has_capability(granted, cap))
}

fn tools_list(session: Option<&Session>, params: &Value) -> Value {
    let granted = session
        .map(|session| session.capabilities.clone())
        .unwrap_or_default();
    let ui = client_has_ui(params);
    let tools = tools()
        .into_iter()
        .filter(|item| tool_allowed(&granted, item))
        .map(|item| {
            let mut value = json!({"name": item.name, "description": item.description, "inputSchema": item.schema});
            if let Some(meta) = tool_ui(item.name, ui) {
                value["_meta"] = json!({"ui": meta});
            }
            value
        })
        .collect::<Vec<_>>();
    json!({"resultType": "complete", "tools": tools, "ttlMs": 0, "cacheScope": "private"})
}

fn tool_ui(name: &str, ui: bool) -> Option<Value> {
    match name {
        "review_open" if ui => Some(json!({
            "resourceUri": REVIEW_APP_URI,
            "visibility": ["model", "app"]
        })),
        "review_section" => Some(json!({"visibility": ["app"]})),
        _ => None,
    }
}

fn tools_call(
    engine: &WorkspaceEngine,
    session: Option<&Session>,
    params: &Value,
) -> Result<Value, RpcError> {
    let Some(session) = session else {
        return Err(rpc(INVALID_PARAMS, "session required", 400));
    };
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let tool = tools().into_iter().find(|item| item.name == name);
    if tool
        .as_ref()
        .is_none_or(|item| !tool_allowed(&session.capabilities, item))
    {
        let capability = tool
            .as_ref()
            .map(|item| item.capability)
            .unwrap_or("requested");
        return Ok(tool_error(permission_denied(format!(
            "session is missing the {capability} capability"
        ))));
    }
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !arguments.is_object() {
        return Ok(tool_error(invalid_request(
            "tool arguments must be an object",
        )));
    }
    if name == "review_open" {
        return match open_review(engine, &arguments, client_has_ui(params)) {
            Ok((text, structured)) => Ok(complete_tool(&text, structured)),
            Err(err) => Ok(tool_error(err)),
        };
    }
    if name == "review_section" {
        return match read_review_section(engine, &arguments) {
            Ok((text, structured)) => Ok(complete_tool(&text, Some(structured))),
            Err(err) => Ok(tool_error(err)),
        };
    }
    if name == "handoff_create" && client_has_tasks(params) {
        return match create_handoff(engine, session, &arguments) {
            Ok(created) => Ok(task_body(&created.task, "task")),
            Err(err) => Ok(tool_error(err)),
        };
    }
    match dispatch(engine, session, name, &arguments) {
        Ok(payload) => Ok(json!({
            "resultType": "complete",
            "content": [{"type": "text", "text": payload.to_string()}],
            "isError": false
        })),
        Err(err) => Ok(tool_error(err)),
    }
}

fn as_json(value: impl serde::Serialize) -> Result<Value, CommonroomError> {
    serde_json::to_value(value).map_err(|err| ce_protocol::internal_error(err.to_string()))
}

fn dispatch(
    engine: &WorkspaceEngine,
    session: &Session,
    name: &str,
    args: &Value,
) -> Result<Value, CommonroomError> {
    let value = match name {
        "workspace_status" => {
            let mut status = as_json(engine.status()?)?;
            if !has_capability(&session.capabilities, "admin") {
                if let Some(map) = status.as_object_mut() {
                    map.insert("path".into(), Value::Null);
                }
            }
            status
        }
        "workspace_summary" => as_json(
            engine.summary(
                args.get("max_lines").and_then(Value::as_i64).unwrap_or(20),
                args.get("max_bytes")
                    .and_then(Value::as_i64)
                    .unwrap_or(2048),
                args.get("max_depth").and_then(Value::as_i64),
            )?,
        )?,
        "workspace_regions" => as_json(engine.regions_page(
            args.get("depth").and_then(Value::as_i64),
            args.get("under").and_then(Value::as_str),
            args.get("cursor").and_then(Value::as_i64),
            args.get("limit").and_then(Value::as_i64),
        )?)?,
        "workspace_read" => as_json(engine.read_with_offset(
            args.get("section").and_then(Value::as_str),
            args.get("max_lines").and_then(Value::as_i64),
            args.get("max_bytes").and_then(Value::as_i64),
            args.get("offset").and_then(Value::as_i64),
        )?)?,
        "workspace_diff" => as_json(engine.diff(required_i64(args, "base_version")?)?)?,
        "workspace_changes_since" => as_json(engine.changes_since(
            required_i64(args, "version")?,
            args.get("limit").and_then(Value::as_i64).unwrap_or(20),
            None,
        )?)?,
        "workspace_participants" => as_json(engine.participants()?)?,
        "workspace_leases" => as_json(engine.leases()?)?,
        "workspace_proposals" => as_json(engine.proposals()?)?,
        "workspace_history" => as_json(engine.history(
            args.get("limit").and_then(Value::as_i64).unwrap_or(50),
            None,
        )?)?,
        "workspace_diagnostics" => as_json(engine.connection_diagnostics()?)?,
        "workspace_leave" => {
            engine.leave(&session.participant_id)?;
            json!({"status": "ok"})
        }
        "workspace_observe" => json!({"events": engine.events_since(
            args.get("since_event_id").and_then(Value::as_i64).unwrap_or(0),
            None,
            args.get("limit").and_then(Value::as_i64).unwrap_or(100),
        )?}),
        "workspace_checkout" => as_json(engine.checkout(
            &session.participant_id,
            required_str(args, "target")?,
            args.get("minutes").and_then(Value::as_i64),
            Some(session),
        )?)?,
        "workspace_release" => {
            engine.release(
                required_str(args, "lease_id")?,
                Some(&session.participant_id),
                has_capability(&session.capabilities, "admin"),
            )?;
            json!({"status": "ok"})
        }
        "workspace_renew" => as_json(engine.renew(
            required_str(args, "lease_id")?,
            &session.participant_id,
            args.get("minutes").and_then(Value::as_i64),
            Some(session),
        )?)?,
        "workspace_propose" => as_json(engine.propose(
            &session.participant_id,
            required_str(args, "target")?,
            required_i64(args, "base_version")?,
            required_str(args, "summary")?,
            args.get("change").cloned().unwrap_or(Value::Null),
            Some(session),
        )?)?,
        "workspace_commit" => {
            let proposal_id = args
                .get("proposal_id")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid_request("proposal_id is required"))?;
            as_json(engine.commit_proposal(proposal_id, &session.participant_id, Some(session))?)?
        }
        "workspace_baton_request" => as_json(engine.baton_request(
            args.get("minutes").and_then(Value::as_i64),
            args.get("reason").and_then(Value::as_str),
            session,
        )?)?,
        "workspace_baton_renew" => as_json(engine.baton_renew(
            required_str(args, "baton_id")?,
            args.get("minutes").and_then(Value::as_i64),
            session,
        )?)?,
        "workspace_baton_release" => {
            engine.baton_release(required_str(args, "baton_id")?, session)?;
            json!({"status": "ok"})
        }
        "workspace_baton_status" => engine.baton_status()?,
        "workspace_write" => as_json(engine.write_with_baton(
            required_str(args, "baton_id")?,
            required_str(args, "target")?,
            required_i64(args, "base_version")?,
            required_str(args, "summary")?,
            args.get("change").cloned().unwrap_or(Value::Null),
            session,
        )?)?,
        "handoff_create" => as_json(create_handoff(engine, session, args)?)?,
        "handoff_list" => as_json(engine.list_handoffs()?)?,
        "handoff_get" => as_json(engine.get_handoff(required_str(args, "handoff_id")?)?)?,
        "handoff_accept" => as_json(
            engine.accept_handoff(&session.participant_id, required_str(args, "handoff_id")?)?,
        )?,
        "handoff_complete" => as_json(engine.complete_handoff(
            &session.participant_id,
            required_str(args, "handoff_id")?,
            required_str(args, "result")?,
        )?)?,
        "handoff_request_input" => as_json(engine.request_handoff_input(
            &session.participant_id,
            required_str(args, "handoff_id")?,
            required_str(args, "prompt")?,
        )?)?,
        "handoff_events" => {
            json!({"events": engine.handoff_events(required_str(args, "handoff_id")?)?})
        }
        "handoff_approve" => as_json(
            engine.approve_handoff(&session.participant_id, required_str(args, "handoff_id")?)?,
        )?,
        "handoff_reject" => as_json(
            engine.reject_handoff(&session.participant_id, required_str(args, "handoff_id")?)?,
        )?,
        "review_context" => as_json(engine.review_context()?)?,
        "review_export" => as_json(
            engine.export_review(args.get("format").and_then(Value::as_str).unwrap_or("json"))?,
        )?,
        "review_question_create" => as_json(engine.create_question(session, args)?)?,
        "review_question_update" => as_json(engine.update_question(session, args)?)?,
        "review_answer" => as_json(engine.answer_question(session, args)?)?,
        "review_comment" => as_json(engine.add_comment(session, args)?)?,
        "review_question_status" => as_json(engine.set_question_status(session, args)?)?,
        "review_status" => as_json(engine.set_review_status(session, args)?)?,
        "workspace_review" => as_json(
            engine.review_proposal(
                required_str(args, "proposal_id")?,
                &session.participant_id,
                args.get("approve")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                Some(session),
            )?,
        )?,
        _ => return Err(invalid_request("unknown tool")),
    };
    Ok(value)
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, CommonroomError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request(format!("{key} is required")))
}

fn required_i64(args: &Value, key: &str) -> Result<i64, CommonroomError> {
    args.get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| invalid_request(format!("{key} is required")))
}

fn complete_tool(text: &str, structured: Option<Value>) -> Value {
    let mut result = json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text}],
        "isError": false
    });
    if let Some(structured) = structured {
        result["structuredContent"] = structured;
    }
    result
}

fn open_review(
    engine: &WorkspaceEngine,
    args: &Value,
    ui: bool,
) -> Result<(String, Option<Value>), CommonroomError> {
    let version = engine.status()?.version;
    let review = engine.review_context()?;
    let page = engine.regions_page(Some(6), None, None, Some(40))?;
    let sections: Vec<Value> = page
        .regions
        .iter()
        .map(|region| json!({"path": region.path, "title": region.title}))
        .collect();
    let questions: Vec<Value> = review
        .review
        .questions
        .iter()
        .map(|question| {
            json!({
                "question_id": question.question_id,
                "status": question.status,
                "region": question.anchor.region
            })
        })
        .collect();
    let proposals: Vec<Value> = review
        .proposals
        .iter()
        .map(|proposal| {
            json!({
                "proposal_id": proposal.proposal_id,
                "status": proposal.status,
                "base_version": proposal.base_version,
                "target": proposal.target
            })
        })
        .collect();
    let pending = proposals
        .iter()
        .filter(|item| {
            let status = item.get("status").and_then(Value::as_str);
            status == Some("pending") || status == Some("approved")
        })
        .count();
    let requested = args
        .get("handoff_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty());
    let handoff = match requested {
        Some(id) => match engine.get_handoff(id) {
            Ok(item) => Some(json!({"handoff_id": item.handoff_id, "status": item.status})),
            Err(_) => None,
        },
        None => None,
    };
    let titles = page
        .regions
        .iter()
        .map(|region| region.title.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut lines = vec![
        format!("version {version}"),
        format!("sections: {titles}"),
        format!("open questions: {}", review.review.counts.open),
        format!("pending proposals: {pending}"),
    ];
    if let Some(id) = requested {
        match &handoff {
            Some(item) => lines.push(format!(
                "handoff {} {}",
                item.get("handoff_id").and_then(Value::as_str).unwrap_or(id),
                item.get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            )),
            None => lines.push("handoff: not_found".into()),
        }
    }
    let structured = ui.then(|| {
        json!({
            "version": version,
            "sections": sections,
            "open_questions": review.review.counts.open,
            "questions": questions,
            "proposals": proposals,
            "pending_proposals": pending,
            "handoff": handoff
        })
    });
    Ok((lines.join("\n"), structured))
}

fn read_review_section(
    engine: &WorkspaceEngine,
    args: &Value,
) -> Result<(String, Value), CommonroomError> {
    let max_bytes = args
        .get("max_bytes")
        .and_then(Value::as_i64)
        .unwrap_or(8_192);
    let read = engine.read_with_offset(
        args.get("section").and_then(Value::as_str),
        args.get("max_lines").and_then(Value::as_i64),
        Some(max_bytes),
        args.get("offset").and_then(Value::as_i64),
    )?;
    let label = read.section.clone().unwrap_or_else(|| "document".into());
    let text = format!(
        "version {} section {label} bytes {} truncated {}",
        read.version,
        read.content.len(),
        read.truncated
    );
    let structured = json!({
        "version": read.version,
        "section": read.section,
        "content": read.content,
        "truncated": read.truncated,
        "next_offset": read.next_offset
    });
    Ok((text, structured))
}

fn tool_error(err: CommonroomError) -> Value {
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": err.to_value().to_string()}],
        "isError": true
    })
}

fn prompts_list() -> Value {
    json!({
        "resultType": "complete",
        "prompts": [{"name": "workspace_onboarding", "description": "How to collaborate in this room before editing."}],
        "ttlMs": 300_000,
        "cacheScope": "public"
    })
}

fn resources_list(session: Option<&Session>) -> Value {
    let mut resources = vec![json!({
        "uri": SKILL_URI,
        "name": "commonroom",
        "mimeType": "text/markdown"
    })];
    if session.is_some_and(|session| has_capability(&session.capabilities, "read")) {
        for (uri, name, mime) in workspace_resources() {
            resources.push(json!({"uri": uri, "name": name, "mimeType": mime}));
        }
    }
    json!({
        "resultType": "complete",
        "resources": resources,
        "ttlMs": 0,
        "cacheScope": "private"
    })
}

fn resources_read(
    engine: &WorkspaceEngine,
    session: Option<&Session>,
    params: &Value,
) -> Result<Value, RpcError> {
    let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
    if uri == REVIEW_APP_URI {
        let Some(session) = session else {
            return Err(rpc(INVALID_PARAMS, "session required", 400));
        };
        if !has_capability(&session.capabilities, "read") {
            return Err(rpc(APPLICATION_ERROR, "permission_denied", 403));
        }
        return Ok(json!({
            "resultType": "complete",
            "contents": [{
                "uri": REVIEW_APP_URI,
                "mimeType": UI_MIME,
                "text": REVIEW_APP_HTML,
                "_meta": {"ui": {"prefersBorder": true}}
            }],
            "ttlMs": 0,
            "cacheScope": "private"
        }));
    }
    if uri == SKILL_URI {
        return Ok(json!({
            "resultType": "complete",
            "contents": [{"uri": SKILL_URI, "mimeType": "text/markdown", "text": load_skill()}],
            "ttlMs": 0,
            "cacheScope": "private"
        }));
    }
    let Some(session) = session else {
        return Err(rpc(INVALID_PARAMS, "session required", 400));
    };
    if !has_capability(&session.capabilities, "read") {
        return Err(rpc(APPLICATION_ERROR, "permission_denied", 403));
    }
    if uri.len() > 512 || !subscribable_resource(uri) {
        return Err(rpc(INVALID_PARAMS, "unknown resource", 400));
    }
    match read_workspace_resource(engine, session, uri) {
        Ok(text_and_mime) => {
            let (mime, text) = text_and_mime;
            Ok(json!({
                "resultType": "complete",
                "contents": [{"uri": uri, "mimeType": mime, "text": text}],
                "ttlMs": 0,
                "cacheScope": "private"
            }))
        }
        Err(err) => Err(rpc(APPLICATION_ERROR, &err.message, err.http_status)),
    }
}

fn read_workspace_resource(
    engine: &WorkspaceEngine,
    session: &Session,
    uri: &str,
) -> Result<(&'static str, String), CommonroomError> {
    let value = match uri {
        RESOURCE_STATUS => serde_json::to_value(redacted_status(engine, session)?)
            .map_err(|err| ce_protocol::internal_error(err.to_string()))?,
        RESOURCE_DOCUMENT => {
            return Ok((
                "text/markdown",
                engine.read(None, None, Some(1_048_576))?.content,
            ));
        }
        RESOURCE_PROPOSALS => serde_json::to_value(engine.proposals()?)
            .map_err(|err| ce_protocol::internal_error(err.to_string()))?,
        RESOURCE_LEASES => serde_json::to_value(engine.leases()?)
            .map_err(|err| ce_protocol::internal_error(err.to_string()))?,
        RESOURCE_PARTICIPANTS => serde_json::to_value(engine.participants()?)
            .map_err(|err| ce_protocol::internal_error(err.to_string()))?,
        RESOURCE_REVIEW => serde_json::to_value(engine.review_context()?)
            .map_err(|err| ce_protocol::internal_error(err.to_string()))?,
        _ => return Err(invalid_request("unknown resource")),
    };
    Ok(("application/json", value.to_string()))
}

fn redacted_status(
    engine: &WorkspaceEngine,
    session: &Session,
) -> Result<ce_core::Status, CommonroomError> {
    let mut status = engine.status()?;
    if !has_capability(&session.capabilities, "admin") {
        status.path = None;
    }
    Ok(status)
}

fn resources_subscribe(
    hub: &NotifyHub,
    session: Option<&Session>,
    params: &Value,
) -> Result<Value, RpcError> {
    let Some(session) = session else {
        return Err(rpc(INVALID_PARAMS, "session required", 400));
    };
    let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
    match hub.subscribe_resource(session, uri) {
        Ok(()) => Ok(json!({"resultType": "complete"})),
        Err(err) => Err(rpc(APPLICATION_ERROR, &err.message, err.http_status)),
    }
}

fn resources_unsubscribe(
    hub: &NotifyHub,
    session: Option<&Session>,
    params: &Value,
) -> Result<Value, RpcError> {
    let Some(session) = session else {
        return Err(rpc(INVALID_PARAMS, "session required", 400));
    };
    let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
    match hub.unsubscribe_resource(session, uri) {
        Ok(()) => Ok(json!({"resultType": "complete"})),
        Err(err) => Err(rpc(APPLICATION_ERROR, &err.message, err.http_status)),
    }
}

fn prompts_get(params: &Value) -> Result<Value, RpcError> {
    if params.get("name").and_then(Value::as_str) != Some("workspace_onboarding") {
        return Err(rpc(INVALID_PARAMS, "unknown prompt", 400));
    }
    Ok(json!({
        "resultType": "complete",
        "description": "How to collaborate in this room before editing.",
        "messages": [{"role": "user", "content": {"type": "text", "text": ONBOARDING}}],
        "ttlMs": 300_000,
        "cacheScope": "public"
    }))
}

fn skills_list() -> Value {
    json!({"resultType": "complete", "skills": [skill_entry()], "ttlMs": 0, "cacheScope": "private"})
}

fn skills_get(params: &Value) -> Result<Value, RpcError> {
    if params.get("uri").and_then(Value::as_str) != Some(SKILL_URI) {
        return Err(rpc(INVALID_PARAMS, "unknown skill", 400));
    }
    Ok(
        json!({"resultType": "complete", "skill": skill_entry(), "ttlMs": 0, "cacheScope": "private"}),
    )
}

fn skill_entry() -> Value {
    let text = load_skill();
    let raw = text.as_bytes();
    let mut hasher = Sha256::new();
    hasher.update(raw);
    json!({
        "uri": SKILL_URI,
        "frontmatter": frontmatter(&text),
        "resources": [{
            "uri": SKILL_URI,
            "digest": format!("sha256:{}", hex::encode(hasher.finalize())),
            "size": raw.len()
        }]
    })
}

fn frontmatter(text: &str) -> BTreeMap<String, String> {
    let mut data = BTreeMap::from([
        ("name".into(), "commonroom".into()),
        (
            "description".into(),
            "Join a Commonroom as a participant.".into(),
        ),
    ]);
    let Some(rest) = text.strip_prefix("---\n") else {
        return data;
    };
    let Some(end) = rest.find("\n---") else {
        return data;
    };
    for line in rest[..end].lines() {
        if let Some((key, value)) = line.split_once(':') {
            if !key.trim().is_empty() {
                data.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    data
}

fn rpc(code: i64, message: &str, status: u16) -> RpcError {
    RpcError {
        code,
        message: message.into(),
        status,
        data: None,
    }
}

fn rpc_error(id: Value, code: i64, message: &str, status: u16, data: Option<Value>) -> Response {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    (
        status,
        Json(json!({"jsonrpc": "2.0", "id": id, "error": error})),
    )
        .into_response()
}

fn listen_key(id: &Value) -> String {
    match id {
        Value::String(value) => format!("s:{value}"),
        Value::Number(value) => format!("n:{value}"),
        other => format!("j:{other}"),
    }
}

fn client_has_ui(params: &Value) -> bool {
    params
        .get("_meta")
        .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
        .and_then(|caps| caps.get("extensions"))
        .and_then(|extensions| extensions.get(UI_EXTENSION))
        .and_then(|ui| ui.get("mimeTypes"))
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|item| item.as_str() == Some(UI_MIME)))
}

fn client_has_tasks(params: &Value) -> bool {
    params
        .get("_meta")
        .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
        .and_then(|caps| caps.get("extensions"))
        .and_then(|extensions| extensions.get(TASKS_EXTENSION))
        .is_some_and(Value::is_object)
}

fn missing_tasks(id: Value) -> Response {
    rpc_error(
        id,
        MISSING_CAPABILITY,
        "Missing required client capability",
        400,
        Some(json!({
            "requiredCapabilities": {
                "extensions": { TASKS_EXTENSION: {} }
            }
        })),
    )
}

fn require_tasks<'a>(
    session: Option<&'a Session>,
    params: &Value,
) -> Result<&'a Session, RpcError> {
    let Some(session) = session else {
        return Err(rpc(INVALID_PARAMS, "session required", 400));
    };
    if client_has_tasks(params) {
        Ok(session)
    } else {
        Err(RpcError {
            code: MISSING_CAPABILITY,
            message: "Missing required client capability".into(),
            status: 400,
            data: Some(json!({
                "requiredCapabilities": {"extensions": {TASKS_EXTENSION: {}}}
            })),
        })
    }
}

fn create_handoff(
    engine: &WorkspaceEngine,
    session: &Session,
    args: &Value,
) -> Result<ce_core::HandoffCreated, CommonroomError> {
    let refs = match args.get("artifact_refs") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(_) => return Err(invalid_request("artifact_refs must be an array of strings")),
    };
    engine.create_handoff(
        &session.participant_id,
        NewHandoff {
            summary: args.get("summary").and_then(Value::as_str).unwrap_or(""),
            constraints: args
                .get("constraints")
                .and_then(Value::as_str)
                .unwrap_or(""),
            artifact_refs: &refs,
            idempotency_key: args.get("idempotency_key").and_then(Value::as_str),
            correlation_id: args.get("correlation_id").and_then(Value::as_str),
        },
    )
}

fn task_body(task: &TaskView, result_type: &str) -> Value {
    let mut body = json!({
        "resultType": result_type,
        "taskId": task.task_id,
        "status": task.status,
        "createdAt": task.created_at,
        "lastUpdatedAt": task.updated_at,
        "ttlMs": task.ttl_ms,
        "pollIntervalMs": task.poll_interval_ms,
    });
    if let Some(message) = &task.status_message {
        body["statusMessage"] = json!(message);
    }
    if let Some(handoff_id) = &task.handoff_id {
        body["handoffId"] = json!(handoff_id);
    }
    match task.status.as_str() {
        "completed" => {
            if let Some(result) = &task.result {
                body["result"] = result.clone();
            }
        }
        "failed" => {
            if let Some(error) = &task.error {
                body["error"] = error.clone();
            }
        }
        "input_required" => {
            body["inputRequests"] = task.input_requests.clone().unwrap_or_else(|| json!({}));
        }
        _ => {}
    }
    body
}

fn tasks_get(
    engine: &WorkspaceEngine,
    session: Option<&Session>,
    params: &Value,
) -> Result<Value, RpcError> {
    let session = require_tasks(session, params)?;
    let task_id = params.get("taskId").and_then(Value::as_str).unwrap_or("");
    match engine.get_task(&session.participant_id, task_id) {
        Ok(task) => Ok(task_body(&task, "complete")),
        Err(err) => Err(error_to_rpc(err)),
    }
}

fn tasks_update(
    engine: &WorkspaceEngine,
    session: Option<&Session>,
    params: &Value,
) -> Result<Value, RpcError> {
    let session = require_tasks(session, params)?;
    let task_id = params.get("taskId").and_then(Value::as_str).unwrap_or("");
    let responses = params.get("inputResponses").cloned().unwrap_or(Value::Null);
    match engine.apply_task_input(&session.participant_id, task_id, &responses) {
        Ok(_) => Ok(json!({"resultType": "complete"})),
        Err(err) => Err(error_to_rpc(err)),
    }
}

fn tasks_cancel(
    engine: &WorkspaceEngine,
    session: Option<&Session>,
    params: &Value,
) -> Result<Value, RpcError> {
    let session = require_tasks(session, params)?;
    let task_id = params.get("taskId").and_then(Value::as_str).unwrap_or("");
    match engine.cancel_task(&session.participant_id, task_id) {
        Ok(_) => Ok(json!({"resultType": "complete"})),
        Err(err) => Err(error_to_rpc(err)),
    }
}

fn error_to_rpc(err: CommonroomError) -> RpcError {
    if err.message == "unknown task" || err.code == "invalid_request" {
        RpcError {
            code: INVALID_PARAMS,
            message: err.message,
            status: 400,
            data: None,
        }
    } else {
        RpcError {
            code: APPLICATION_ERROR,
            message: err.message.clone(),
            status: err.http_status,
            data: Some(err.to_value()),
        }
    }
}

fn subscriptions_listen(
    engine: &Arc<WorkspaceEngine>,
    hub: &Arc<NotifyHub>,
    session: Option<&Session>,
    request_id: &Value,
    params: &Value,
) -> Response {
    let Some(session) = session else {
        return rpc_error(
            request_id.clone(),
            INVALID_PARAMS,
            "session required",
            400,
            None,
        );
    };
    let notifications = params
        .get("notifications")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if !notifications.is_object() {
        return rpc_error(
            request_id.clone(),
            INVALID_PARAMS,
            "invalid params",
            400,
            None,
        );
    }
    let task_ids = match string_list(notifications.get("taskIds")) {
        Ok(values) => values,
        Err(err) => return rpc_error(request_id.clone(), INVALID_PARAMS, &err.message, 400, None),
    };
    if !task_ids.is_empty() && !client_has_tasks(params) {
        return missing_tasks(request_id.clone());
    }
    if task_ids.len() > 32 {
        return rpc_error(
            request_id.clone(),
            APPLICATION_ERROR,
            "too many task subscriptions",
            429,
            Some(limit_exceeded("too many task subscriptions").to_value()),
        );
    }
    let requested_resources = match string_list(notifications.get("resourceSubscriptions")) {
        Ok(values) => values,
        Err(err) => return rpc_error(request_id.clone(), INVALID_PARAMS, &err.message, 400, None),
    };
    let can_read = has_capability(&session.capabilities, "read");
    let resources: Vec<String> = if can_read {
        requested_resources
            .into_iter()
            .filter(|uri| subscribable_resource(uri))
            .collect()
    } else {
        Vec::new()
    };
    let mut accepted_tasks = Vec::new();
    for task_id in &task_ids {
        if engine.get_task(&session.participant_id, task_id).is_ok() {
            accepted_tasks.push(task_id.clone());
        }
    }
    let mut accepted = serde_json::Map::new();
    if !resources.is_empty() {
        accepted.insert("resourceSubscriptions".into(), json!(resources));
    }
    if !accepted_tasks.is_empty() {
        accepted.insert("taskIds".into(), json!(accepted_tasks));
    }
    tracing::info!(
        participant_id = %session.participant_id,
        tasks = accepted_tasks.len(),
        resources = resources.len(),
        "subscription listening"
    );
    let stop = hub.begin_listen(&listen_key(request_id));
    let engine = Arc::clone(engine);
    let hub = Arc::clone(hub);
    let session = session.clone();
    let subscription_id = request_id.clone();
    let key = listen_key(request_id);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(16);
    std::thread::spawn(move || {
        pump_listen(ListenPump {
            engine,
            hub,
            session,
            subscription_id,
            accepted: Value::Object(accepted),
            resources,
            task_ids,
            stop,
            key,
            tx,
        });
    });
    let stream = async_stream::stream! {
        while let Some(frame) = rx.recv().await {
            yield Ok::<Bytes, std::io::Error>(Bytes::from(frame));
        }
    };
    let mut response = event_stream(stream);
    response.headers_mut().insert(
        "mcp-protocol-version",
        MCP_PROTOCOL_VERSION.parse().expect("protocol version"),
    );
    response
}

fn string_list(value: Option<&Value>) -> Result<Vec<String>, CommonroomError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let Some(items) = value.as_array() else {
        return Err(invalid_request("notification filters must be arrays"));
    };
    let mut values = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(invalid_request("notification filters must be strings"));
        };
        values.push(text.to_string());
    }
    Ok(values)
}

struct ListenPump {
    engine: Arc<WorkspaceEngine>,
    hub: Arc<NotifyHub>,
    session: Session,
    subscription_id: Value,
    accepted: Value,
    resources: Vec<String>,
    task_ids: Vec<String>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    key: String,
    tx: tokio::sync::mpsc::Sender<String>,
}

fn pump_listen(pump: ListenPump) {
    let ListenPump {
        engine,
        hub,
        session,
        subscription_id,
        accepted,
        resources,
        task_ids,
        stop,
        key,
        tx,
    } = pump;
    let ack = json!({
        "jsonrpc": "2.0",
        "method": "notifications/subscriptions/acknowledged",
        "params": {
            "_meta": {"io.modelcontextprotocol/subscriptionId": subscription_id},
            "notifications": accepted
        }
    });
    if tx.blocking_send(message_frame(&ack)).is_err() {
        hub.end_listen(&key, &stop);
        return;
    }
    tracing::info!(
        participant_id = %session.participant_id,
        tasks = task_ids.len(),
        resources = resources.len(),
        "subscription acknowledged"
    );
    let subscription = engine.subscribe();
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if engine.authenticate(&session.session_id).is_err() {
            break;
        }
        match subscription.recv_timeout(Duration::from_secs(15)) {
            Ok(ce_core::Delivery::Event(event)) => {
                if !forward_listen(
                    &engine,
                    &session,
                    &subscription_id,
                    &resources,
                    &task_ids,
                    &event,
                    &tx,
                ) {
                    break;
                }
            }
            Ok(ce_core::Delivery::Lagged) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if tx.blocking_send(": keepalive\n\n".into()).is_err() {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    tracing::info!(participant_id = %session.participant_id, "subscription closed");
    hub.end_listen(&key, &stop);
}

fn forward_listen(
    engine: &WorkspaceEngine,
    session: &Session,
    subscription_id: &Value,
    resources: &[String],
    task_ids: &[String],
    event: &Event,
    tx: &tokio::sync::mpsc::Sender<String>,
) -> bool {
    if event.event_type == TASK_CREATED || event.event_type == TASK_UPDATED {
        let Some(task_id) = event.payload.get("task_id").and_then(Value::as_str) else {
            return true;
        };
        if !task_ids.iter().any(|id| id == task_id) {
            return true;
        }
        let Ok(task) = engine.get_task(&session.participant_id, task_id) else {
            return true;
        };
        let mut params = task_body(&task, "complete");
        if let Some(map) = params.as_object_mut() {
            map.remove("resultType");
            let meta = map.entry("_meta").or_insert_with(|| json!({}));
            meta["io.modelcontextprotocol/subscriptionId"] = subscription_id.clone();
        }
        let note = json!({
            "jsonrpc": "2.0",
            "method": "notifications/tasks",
            "params": params
        });
        return tx.blocking_send(message_frame(&note)).is_ok();
    }
    for uri in resources_for_event(&event.event_type) {
        if !resources.iter().any(|wanted| wanted == uri) {
            continue;
        }
        let note = json!({
            "jsonrpc": "2.0",
            "method": "notifications/resources/updated",
            "params": {
                "uri": uri,
                "_meta": {"io.modelcontextprotocol/subscriptionId": subscription_id}
            }
        });
        if tx.blocking_send(message_frame(&note)).is_err() {
            return false;
        }
    }
    true
}

const ONBOARDING: &str = include_str!("../../../skills/commonroom/onboarding.txt");
