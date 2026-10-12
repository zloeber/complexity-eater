use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use base64::Engine;
use ce_core::{load_skill, Session, WorkspaceEngine};
use ce_protocol::{
    has_capability, invalid_request, permission_denied, unauthorized, CommonroomError,
    MCP_PROTOCOL_VERSION, SKILL_URI,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const HEADER_MISMATCH: i64 = -32020;
const UNSUPPORTED_VERSION: i64 = -32022;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const APPLICATION_ERROR: i64 = -32000;

struct Tool {
    name: &'static str,
    capability: &'static str,
    description: &'static str,
    schema: Value,
}

pub fn handle_mcp(engine: &WorkspaceEngine, headers: &HeaderMap, body: &[u8]) -> Response {
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
    if message.get("id").is_none() || message.get("id") == Some(&Value::Null) {
        return StatusCode::ACCEPTED.into_response();
    }
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
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
            | "resources/read"
            | "skills/list"
            | "skills/get"
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
        "server/discover" => Ok(discover()),
        "tools/list" => Ok(tools_list(session.as_ref())),
        "tools/call" => tools_call(engine, session.as_ref(), &params),
        "prompts/list" => Ok(prompts_list()),
        "prompts/get" => prompts_get(&params),
        "resources/read" => resources_read(&params),
        "skills/list" => Ok(skills_list()),
        "skills/get" => skills_get(&params),
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
        "resources/read" => "uri",
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

fn discover() -> Value {
    json!({
        "resultType": "complete",
        "supportedVersions": [MCP_PROTOCOL_VERSION],
        "capabilities": {
            "tools": {},
            "resources": {},
            "prompts": {},
            "extensions": {"io.modelcontextprotocol/skills": {}}
        },
        "_meta": {"io.modelcontextprotocol/serverInfo": {"name": "Commonroom", "version": "0.1.0"}},
        "instructions": "Use the commonroom skill or the workspace_onboarding prompt before editing. Propose changes. Approval stays with the workspace owner.",
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
            "Markdown regions in the workspace.",
            json!({"type": "object", "properties": {}}),
        ),
        tool(
            "workspace_read",
            "read",
            "Read one region, or a bounded excerpt.",
            json!({"type": "object", "properties": {"section": {"type": "string"}, "max_lines": {"type": "integer"}, "max_bytes": {"type": "integer"}}}),
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
            json!({"type": "object", "properties": {"target": {"type": "string"}, "base_version": {"type": "integer"}, "summary": {"type": "string"}, "change": {"type": "object"}}, "required": ["target", "base_version", "summary", "change"]}),
        ),
        tool(
            "workspace_commit",
            "commit",
            "Commit an approved proposal.",
            json!({"type": "object", "properties": {"proposal_id": {"type": "string"}}, "required": ["proposal_id"]}),
        ),
        tool(
            "workspace_review",
            "approve",
            "Approve or reject a proposal.",
            json!({"type": "object", "properties": {"proposal_id": {"type": "string"}, "approve": {"type": "boolean"}}, "required": ["proposal_id", "approve"]}),
        ),
    ]
}

fn tool(
    name: &'static str,
    capability: &'static str,
    description: &'static str,
    schema: Value,
) -> Tool {
    Tool {
        name,
        capability,
        description,
        schema,
    }
}

fn tools_list(session: Option<&Session>) -> Value {
    let granted = session
        .map(|session| session.capabilities.clone())
        .unwrap_or_default();
    let tools = tools()
        .into_iter()
        .filter(|item| has_capability(&granted, item.capability))
        .map(|item| json!({"name": item.name, "description": item.description, "inputSchema": item.schema}))
        .collect::<Vec<_>>();
    json!({"resultType": "complete", "tools": tools, "ttlMs": 0, "cacheScope": "private"})
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
        .is_none_or(|item| !has_capability(&session.capabilities, item.capability))
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
        "workspace_regions" => as_json(engine.regions()?)?,
        "workspace_read" => as_json(engine.read(
            args.get("section").and_then(Value::as_str),
            args.get("max_lines").and_then(Value::as_i64),
            args.get("max_bytes").and_then(Value::as_i64),
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
            if let Some(proposal_id) = args.get("proposal_id").and_then(Value::as_str) {
                as_json(engine.commit_proposal(
                    proposal_id,
                    &session.participant_id,
                    Some(session),
                )?)?
            } else {
                as_json(
                    engine.commit(
                        &session.participant_id,
                        required_i64(args, "base_version")?,
                        required_str(args, "summary")?,
                        required_str(args, "content")?,
                        args.get("target")
                            .and_then(Value::as_str)
                            .unwrap_or("workspace"),
                        Some(session),
                    )?,
                )?
            }
        }
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
        "prompts": [{"name": "workspace_onboarding", "description": "How to collaborate in this Commonroom before editing."}],
        "ttlMs": 300_000,
        "cacheScope": "public"
    })
}

fn prompts_get(params: &Value) -> Result<Value, RpcError> {
    if params.get("name").and_then(Value::as_str) != Some("workspace_onboarding") {
        return Err(rpc(INVALID_PARAMS, "unknown prompt", 400));
    }
    Ok(json!({
        "resultType": "complete",
        "description": "How to collaborate in this Commonroom before editing.",
        "messages": [{"role": "user", "content": {"type": "text", "text": ONBOARDING}}],
        "ttlMs": 300_000,
        "cacheScope": "public"
    }))
}

fn resources_read(params: &Value) -> Result<Value, RpcError> {
    if params.get("uri").and_then(Value::as_str) != Some(SKILL_URI) {
        return Err(rpc(INVALID_PARAMS, "unknown resource", 400));
    }
    Ok(json!({
        "resultType": "complete",
        "contents": [{"uri": SKILL_URI, "mimeType": "text/markdown", "text": load_skill()}],
        "ttlMs": 0,
        "cacheScope": "private"
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

const ONBOARDING: &str = "\
Work in this Commonroom in this order:
1. Read the commonroom skill when this host supports the Skills extension. Otherwise follow the steps below.
2. workspace_status
3. workspace_summary
4. workspace_read for one region
5. workspace_checkout for that region
6. workspace_propose against the base_version you just read
7. Wait until the proposal is approved
8. workspace_commit with the proposal_id
9. workspace_release

Propose changes. Approval stays with the workspace owner.
";
