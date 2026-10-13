use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::{middleware, Router};
use ce_core::{load_skill, Session, Status, WorkspaceEngine};
use ce_protocol::{
    discovery_document, has_capability, invalid_request, limit_exceeded, protocol_document,
    unauthorized, CommonroomError,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mcp::handle_mcp;
use crate::notify::{
    cursor_from, mcp_stream_response, observe_response, wants_event_stream, NotifyHub,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Owner,
    Session,
}

pub struct AppState {
    pub engine: Arc<WorkspaceEngine>,
    pub access: Access,
    pub host_session: Option<Session>,
    rate: Mutex<HashMap<String, Vec<Instant>>>,
    hub: Option<Arc<NotifyHub>>,
}

pub fn create_router(
    engine: Arc<WorkspaceEngine>,
    access: Access,
    host_session: Option<Session>,
) -> Router {
    let max_bytes = engine.limits().max_request_bytes;
    let hub = if access == Access::Session {
        Some(NotifyHub::start(Arc::clone(&engine)))
    } else {
        None
    };
    let state = Arc::new(AppState {
        engine,
        access,
        host_session,
        rate: Mutex::new(HashMap::new()),
        hub,
    });
    let mut router = Router::new()
        .route("/.well-known/agent-workspace", get(well_known))
        .route("/protocol", get(protocol))
        .route("/skills", get(skills))
        .route("/skills/commonroom", get(skill))
        .route("/session/join", post(session_join))
        .route("/workspace/status", get(workspace_status))
        .route("/workspace/summary", get(workspace_summary))
        .route("/workspace/regions", get(workspace_regions))
        .route("/workspace/read", get(workspace_read))
        .route("/workspace/diff", get(workspace_diff))
        .route("/workspace/changes_since", get(changes_since))
        .route("/workspace/history", get(workspace_history))
        .route("/workspace/participants", get(participants))
        .route("/workspace/leases", get(leases))
        .route("/workspace/proposals", get(proposals))
        .route("/workspace/handoffs", get(handoffs))
        .route("/workspace/handoff", get(handoff))
        .route("/workspace/handoff/events", get(handoff_events))
        .route("/workspace/task", get(task_status))
        .route("/workspace/handoff/create", post(handoff_create))
        .route("/workspace/handoff/accept", post(handoff_accept))
        .route("/workspace/handoff/complete", post(handoff_complete))
        .route("/workspace/handoff/approve", post(handoff_approve))
        .route("/workspace/task/cancel", post(task_cancel))
        .route("/workspace/diagnostics", get(diagnostics))
        .route("/workspace/room", get(room))
        .route("/workspace/checkout", post(checkout))
        .route("/workspace/renew", post(renew))
        .route("/workspace/release", post(release))
        .route("/workspace/propose", post(propose))
        .route("/workspace/proposal/review", post(review))
        .route("/workspace/commit", post(commit))
        .route("/workspace/invite", post(invite))
        .route("/workspace/invites", get(invites))
        .route("/workspace/invite/revoke", post(invite_revoke))
        .route("/workspace/leave", post(leave))
        .route("/workspace/participant/remove", post(remove_participant))
        .route("/workspace/observe", get(observe))
        .route("/", get(index));
    if access == Access::Session {
        router = router.route("/mcp", post(mcp_post).get(mcp_get).delete(mcp_unused));
    }
    let logged = state.clone();
    router
        .layer(axum::extract::DefaultBodyLimit::max(max_bytes))
        .layer(middleware::from_fn(move |req, next| {
            let state = logged.clone();
            async move { trace_request(state, req, next).await }
        }))
        .with_state(state)
}

pub struct AppError(pub CommonroomError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        json_error(self.0.http_status, &self.0.to_value())
    }
}

impl From<CommonroomError> for AppError {
    fn from(err: CommonroomError) -> Self {
        Self(err)
    }
}

fn json_error(status: u16, body: &Value) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    (status, Json(body.clone())).into_response()
}

async fn trace_request(state: Arc<AppState>, req: Request, next: Next) -> Response {
    if let Some(len) = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
    {
        if let Ok(n) = len.parse::<usize>() {
            if n > state.engine.limits().max_request_bytes {
                return json_error(
                    413,
                    &json!({"error": "limit_exceeded", "message": "request too large"}),
                );
            }
        }
    }
    let path = req.uri().path().to_string();
    let started = Instant::now();
    let response = next.run(req).await;
    tracing::info!(
        operation = %path,
        result = response.status().as_u16(),
        duration_ms = started.elapsed().as_millis() as u64
    );
    response
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value.get(7..)?;
    if value[..7].eq_ignore_ascii_case("bearer ") {
        let token = rest.trim();
        if token.is_empty() {
            None
        } else {
            Some(token.to_string())
        }
    } else {
        None
    }
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let value = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in value.split(';') {
        let mut pieces = part.trim().splitn(2, '=');
        if pieces.next()?.trim() == name {
            return Some(pieces.next()?.trim().to_string());
        }
    }
    None
}

fn principal(
    state: &AppState,
    headers: &HeaderMap,
    peer: &str,
) -> Result<Session, CommonroomError> {
    rate_limit(state, headers, peer)?;
    let token = bearer(headers).or_else(|| cookie(headers, "commonroom_session"));
    let Some(token) = token else {
        return Err(unauthorized("session required"));
    };
    state.engine.authenticate(&token)
}

fn rate_limit(state: &AppState, headers: &HeaderMap, peer: &str) -> Result<(), CommonroomError> {
    let cap = state.engine.limits().max_requests_per_minute;
    if cap == 0 {
        return Ok(());
    }
    let key = bearer(headers).unwrap_or_else(|| peer.to_string());
    let now = Instant::now();
    let mut buckets = state.rate.lock().expect("rate");
    let hits = buckets.entry(key).or_default();
    hits.retain(|stamp| now.duration_since(*stamp) < Duration::from_secs(60));
    if hits.len() >= cap {
        return Err(limit_exceeded("rate limit exceeded").detail("retry_after_seconds", 5));
    }
    hits.push(now);
    Ok(())
}

fn peer_of(headers: &HeaderMap) -> String {
    let _ = headers;
    "anon".into()
}

fn object(bytes: &Bytes) -> Result<Value, CommonroomError> {
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| invalid_request("body must be JSON"))?;
    if !value.is_object() {
        return Err(invalid_request("body must be a JSON object"));
    }
    Ok(value)
}

fn json_ok(value: impl serde::Serialize) -> Result<Response, AppError> {
    Ok(Json(
        serde_json::to_value(value).map_err(|err| ce_protocol::internal_error(err.to_string()))?,
    )
    .into_response())
}

fn redact_status(mut status: Status, session: &Session) -> Status {
    if !has_capability(&session.capabilities, "admin") {
        status.path = None;
    }
    status
}

async fn well_known(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(discovery_document(
        &state.engine.workspace_id().unwrap_or_default(),
    ))
}

async fn protocol() -> Json<Value> {
    Json(protocol_document())
}

async fn skills() -> Json<Value> {
    Json(json!({
        "skills": [{
            "name": "commonroom",
            "description": "Join a room, observe it, lease a region, and propose changes.",
            "path": "/skills/commonroom"
        }]
    }))
}

async fn skill() -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        load_skill(),
    )
        .into_response()
}

async fn session_join(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Result<Response, AppError> {
    let payload = object(&body)?;
    let token = payload
        .get("invite")
        .or_else(|| payload.get("invite_token"))
        .and_then(Value::as_str)
        .ok_or_else(|| unauthorized("invitation required"))?;
    let kind = payload
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    if kind != "human" && kind != "agent" {
        return Err(invalid_request("kind must be 'human' or 'agent'").into());
    }
    let name = truncate(
        payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("participant"),
        80,
    );
    let transport = truncate(
        payload
            .get("transport")
            .and_then(Value::as_str)
            .unwrap_or("remote"),
        64,
    );
    json_ok(
        state
            .engine
            .join(kind, &name, None, Some(token), &transport, None)?,
    )
}

async fn workspace_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, &peer_of(&headers))?;
    state.engine.require(&session, "read")?;
    json_ok(redact_status(state.engine.status()?, &session))
}

#[derive(Deserialize, Default)]
struct SummaryQuery {
    #[serde(default = "twenty")]
    max_lines: i64,
    #[serde(default = "two_kb")]
    max_bytes: i64,
    max_depth: Option<i64>,
}

fn twenty() -> i64 {
    20
}
fn two_kb() -> i64 {
    2048
}
fn fifty() -> i64 {
    50
}

async fn workspace_summary(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<SummaryQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(
        state
            .engine
            .summary(query.max_lines, query.max_bytes, query.max_depth)?,
    )
}

async fn workspace_regions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(state.engine.regions()?)
}

#[derive(Deserialize, Default)]
struct ReadQuery {
    section: Option<String>,
    max_lines: Option<i64>,
    max_bytes: Option<i64>,
}

async fn workspace_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ReadQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(
        state
            .engine
            .read(query.section.as_deref(), query.max_lines, query.max_bytes)?,
    )
}

#[derive(Deserialize, Default)]
struct DiffQuery {
    base_version: Option<i64>,
}

async fn workspace_diff(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<DiffQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    let base = query
        .base_version
        .ok_or_else(|| invalid_request("base_version is required"))?;
    json_ok(state.engine.diff(base)?)
}

#[derive(Deserialize, Default)]
struct ChangesQuery {
    version: Option<i64>,
    #[serde(default = "twenty")]
    limit: i64,
    max_items: Option<i64>,
}

async fn changes_since(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ChangesQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    let version = query
        .version
        .ok_or_else(|| invalid_request("version is required"))?;
    json_ok(
        state
            .engine
            .changes_since(version, query.limit, query.max_items)?,
    )
}

#[derive(Deserialize, Default)]
struct HistoryQuery {
    #[serde(default = "fifty")]
    limit: i64,
    max_items: Option<i64>,
}

async fn workspace_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(state.engine.history(query.limit, query.max_items)?)
}

async fn participants(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(state.engine.participants()?)
}

async fn leases(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(state.engine.leases()?)
}

async fn proposals(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(state.engine.proposals()?)
}

#[derive(Deserialize, Default)]
struct HandoffQuery {
    handoff_id: Option<String>,
    task_id: Option<String>,
}

async fn handoffs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(state.engine.list_handoffs()?)
}

async fn handoff(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HandoffQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    let id = query
        .handoff_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_request("handoff_id is required"))?;
    json_ok(state.engine.get_handoff(id)?)
}

async fn handoff_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HandoffQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    let id = query
        .handoff_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_request("handoff_id is required"))?;
    json_ok(json!({"events": state.engine.handoff_events(id)?}))
}

async fn task_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<HandoffQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    let id = query
        .task_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_request("task_id is required"))?;
    json_ok(state.engine.get_task(&session.participant_id, id)?)
}

async fn handoff_create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "propose")?;
    let payload = object(&body)?;
    let refs = payload
        .get("artifact_refs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json_ok(
        state.engine.create_handoff(
            &session.participant_id,
            ce_core::NewHandoff {
                summary: payload.get("summary").and_then(Value::as_str).unwrap_or(""),
                constraints: payload
                    .get("constraints")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                artifact_refs: &refs,
                idempotency_key: payload.get("idempotency_key").and_then(Value::as_str),
                correlation_id: payload.get("correlation_id").and_then(Value::as_str),
            },
        )?,
    )
}

async fn handoff_accept(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "commit")?;
    let payload = object(&body)?;
    let id = payload
        .get("handoff_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("handoff_id is required"))?;
    json_ok(state.engine.accept_handoff(&session.participant_id, id)?)
}

async fn handoff_complete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "commit")?;
    let payload = object(&body)?;
    let id = payload
        .get("handoff_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("handoff_id is required"))?;
    let result = payload
        .get("result")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("result is required"))?;
    json_ok(
        state
            .engine
            .complete_handoff(&session.participant_id, id, result)?,
    )
}

async fn handoff_approve(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "approve")?;
    let payload = object(&body)?;
    let id = payload
        .get("handoff_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("handoff_id is required"))?;
    json_ok(state.engine.approve_handoff(&session.participant_id, id)?)
}

async fn task_cancel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    let payload = object(&body)?;
    let id = payload
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("task_id is required"))?;
    json_ok(state.engine.cancel_task(&session.participant_id, id)?)
}

async fn diagnostics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    json_ok(state.engine.connection_diagnostics()?)
}

async fn room(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "read")?;
    let mut payload = json!({
        "status": redact_status(state.engine.status()?, &session),
        "participants": state.engine.participants()?,
        "leases": state.engine.leases()?,
        "proposals": state.engine.proposals()?,
        "events": state.engine.history(20, None)?,
        "summary": state.engine.summary(20, 2048, None)?,
        "diagnostics": state.engine.connection_diagnostics()?,
    });
    if has_capability(&session.capabilities, "invite") {
        payload["invites"] =
            serde_json::to_value(state.engine.list_invites()?).unwrap_or(Value::Null);
    }
    json_ok(payload)
}

async fn checkout(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    let payload = object(&body)?;
    let target = payload
        .get("target")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("target is required"))?;
    let minutes = payload.get("minutes").and_then(Value::as_i64);
    json_ok(state.engine.checkout("", target, minutes, Some(&session))?)
}

async fn renew(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    let payload = object(&body)?;
    let lease_id = payload
        .get("lease_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("lease_id is required"))?;
    let minutes = payload.get("minutes").and_then(Value::as_i64);
    json_ok(state.engine.renew(lease_id, "", minutes, Some(&session))?)
}

async fn release(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "checkout")?;
    let payload = object(&body)?;
    let lease_id = payload
        .get("lease_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("lease_id is required"))?;
    state.engine.release(
        lease_id,
        Some(&session.participant_id),
        has_capability(&session.capabilities, "admin"),
    )?;
    json_ok(json!({"status": "ok"}))
}

async fn propose(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    let payload = object(&body)?;
    for key in ["target", "base_version", "summary", "change"] {
        if payload.get(key).is_none() {
            return Err(invalid_request("missing required proposal fields").into());
        }
    }
    json_ok(state.engine.propose(
        "",
        payload["target"].as_str().unwrap_or(""),
        payload["base_version"].as_i64().unwrap_or(0),
        payload["summary"].as_str().unwrap_or(""),
        payload["change"].clone(),
        Some(&session),
    )?)
}

async fn review(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    let payload = object(&body)?;
    if payload.get("proposal_id").is_none() || payload.get("approve").is_none() {
        return Err(invalid_request("missing proposal review fields").into());
    }
    json_ok(state.engine.review_proposal(
        payload["proposal_id"].as_str().unwrap_or(""),
        "",
        payload["approve"].as_bool().unwrap_or(false),
        Some(&session),
    )?)
}

async fn commit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    let payload = object(&body)?;
    if let Some(proposal_id) = payload.get("proposal_id").and_then(Value::as_str) {
        return json_ok(
            state
                .engine
                .commit_proposal(proposal_id, "", Some(&session))?,
        );
    }
    for key in ["base_version", "summary", "content"] {
        if payload.get(key).is_none() {
            return Err(invalid_request("missing required commit fields").into());
        }
    }
    json_ok(
        state.engine.commit(
            "",
            payload["base_version"].as_i64().unwrap_or(0),
            payload["summary"].as_str().unwrap_or(""),
            payload["content"].as_str().unwrap_or(""),
            payload
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or("workspace"),
            Some(&session),
        )?,
    )
}

async fn invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "invite")?;
    let payload = if body.is_empty() {
        json!({})
    } else {
        object(&body)?
    };
    let capabilities = payload
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        });
    let permissions = payload.get("permissions").and_then(Value::as_str);
    let target = payload.get("target").and_then(Value::as_str);
    let expires = payload
        .get("expires_in_minutes")
        .and_then(Value::as_i64)
        .unwrap_or(state.engine.limits().default_invite_minutes);
    let max_uses = match payload.get("max_uses") {
        None => Some(1),
        Some(Value::Null) => None,
        Some(value) => value.as_i64(),
    };
    json_ok(state.engine.create_invite_with(
        permissions,
        target,
        expires,
        max_uses,
        capabilities,
    )?)
}

async fn invites(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "invite")?;
    json_ok(state.engine.list_invites()?)
}

async fn invite_revoke(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "invite")?;
    let payload = object(&body)?;
    state.engine.revoke_invite(
        payload.get("token").and_then(Value::as_str),
        payload.get("invite_id").and_then(Value::as_str),
    )?;
    json_ok(json!({"status": "ok"}))
}

async fn leave(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.leave(&session.participant_id)?;
    json_ok(json!({"status": "ok"}))
}

async fn remove_participant(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "admin")?;
    let payload = object(&body)?;
    let participant_id = payload
        .get("participant_id")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_request("participant_id is required"))?;
    state.engine.leave(participant_id)?;
    json_ok(json!({"status": "ok"}))
}

#[derive(Deserialize, Default)]
struct ObserveQuery {
    #[serde(default)]
    since_event_id: i64,
    since_version: Option<i64>,
}

async fn observe(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ObserveQuery>,
) -> Result<Response, AppError> {
    let session = principal(&state, &headers, "anon")?;
    state.engine.require(&session, "observe")?;
    let cursor = cursor_from(&headers, query.since_event_id);
    Ok(observe_response(
        Arc::clone(&state.engine),
        session.session_id,
        cursor,
        query.since_version,
    ))
}

async fn index(State(state): State<Arc<AppState>>, _headers: HeaderMap) -> Response {
    if state.access == Access::Owner {
        if let Some(session) = &state.host_session {
            let mut response = Html(include_str!("owner.html")).into_response();
            let cookie = format!(
                "commonroom_session={}; HttpOnly; SameSite=Strict; Path=/",
                session.session_id
            );
            if let Ok(value) = cookie.parse() {
                response.headers_mut().insert(header::SET_COOKIE, value);
            }
            return response;
        }
    }
    Html(include_str!("ingress.html")).into_response()
}

async fn mcp_post(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    if state.access != Access::Session {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(hub) = state.hub.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    handle_mcp(&state.engine, hub, &headers, &body)
}

async fn mcp_get(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if state.access != Access::Session {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !wants_event_stream(&headers) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(hub) = state.hub.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let session = match principal(&state, &headers, "anon") {
        Ok(session) => session,
        Err(err) => return json_error(err.http_status, &err.to_value()),
    };
    if let Err(err) = state.engine.require(&session, "read") {
        return json_error(err.http_status, &err.to_value());
    }
    let version = headers
        .get("mcp-protocol-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if version != ce_protocol::MCP_PROTOCOL_VERSION {
        return json_error(
            400,
            &json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {
                    "code": -32022,
                    "message": "UnsupportedProtocolVersionError"
                }
            }),
        );
    }
    mcp_stream_response(hub, Arc::clone(&state.engine), session.session_id)
}

async fn mcp_unused() -> Response {
    StatusCode::METHOD_NOT_ALLOWED.into_response()
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
