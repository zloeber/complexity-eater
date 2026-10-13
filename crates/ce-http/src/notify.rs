//! Live delivery for the human event stream and MCP resource notifications.
//!
//! The workspace engine remains the source of truth. This module fans events
//! out after they are recorded. A full or disconnected consumer is dropped;
//! the next connection reloads authoritative state. Notifications are hints.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::Response;
use ce_core::{
    is_newer, Delivery, Event, Session, WorkspaceEngine, ARTIFACT_CHANGED, LEASE_ACQUIRED,
    LEASE_EXPIRED, LEASE_RELEASED, PARTICIPANT_JOINED, PARTICIPANT_LEFT, PROPOSAL_APPROVED,
    PROPOSAL_COMMITTED, PROPOSAL_CONFLICTED, PROPOSAL_CREATED, PROPOSAL_REJECTED,
    PROPOSAL_SUPERSEDED, SESSION_CREATED, SESSION_EXPIRED, WORKSPACE_CREATED, WORKSPACE_STARTED,
    WORKSPACE_STOPPED,
};
use ce_protocol::{
    has_capability, invalid_request, permission_denied, CommonroomError, MCP_PROTOCOL_VERSION,
    SKILL_URI,
};
use serde_json::json;
use tokio::sync::mpsc::Sender;

pub const RESOURCE_STATUS: &str = "commonroom://workspace/status";
pub const RESOURCE_DOCUMENT: &str = "commonroom://workspace/document";
pub const RESOURCE_PROPOSALS: &str = "commonroom://workspace/proposals";
pub const RESOURCE_LEASES: &str = "commonroom://workspace/leases";
pub const RESOURCE_PARTICIPANTS: &str = "commonroom://workspace/participants";

const STREAM_CAPACITY: usize = 16;
const KEEPALIVE: Duration = Duration::from_secs(15);

pub fn workspace_resources() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (RESOURCE_STATUS, "Workspace status", "application/json"),
        (RESOURCE_DOCUMENT, "Workspace document", "text/markdown"),
        (
            RESOURCE_PROPOSALS,
            "Workspace proposals",
            "application/json",
        ),
        (RESOURCE_LEASES, "Workspace leases", "application/json"),
        (
            RESOURCE_PARTICIPANTS,
            "Workspace participants",
            "application/json",
        ),
    ]
}

pub fn subscribable_resource(uri: &str) -> bool {
    workspace_resources()
        .iter()
        .any(|(candidate, _, _)| *candidate == uri)
}

pub fn resources_for_event(event_type: &str) -> &'static [&'static str] {
    match event_type {
        PROPOSAL_CREATED | PROPOSAL_APPROVED | PROPOSAL_REJECTED | PROPOSAL_COMMITTED
        | PROPOSAL_CONFLICTED | PROPOSAL_SUPERSEDED => &[RESOURCE_PROPOSALS, RESOURCE_STATUS],
        ARTIFACT_CHANGED | WORKSPACE_CREATED => &[RESOURCE_DOCUMENT, RESOURCE_STATUS],
        LEASE_ACQUIRED | LEASE_RELEASED | LEASE_EXPIRED => &[RESOURCE_LEASES, RESOURCE_STATUS],
        PARTICIPANT_JOINED | PARTICIPANT_LEFT | SESSION_CREATED | SESSION_EXPIRED => {
            &[RESOURCE_PARTICIPANTS, RESOURCE_STATUS]
        }
        WORKSPACE_STARTED | WORKSPACE_STOPPED => &[RESOURCE_STATUS],
        _ => &[],
    }
}

enum Note {
    Updated(String),
    Resync,
}

struct NotifyInner {
    engine: Arc<WorkspaceEngine>,
    subscriptions: Mutex<HashMap<String, HashSet<String>>>,
    streams: Mutex<HashMap<String, SyncSender<Note>>>,
}

pub struct NotifyHub {
    stop: Arc<AtomicBool>,
    inner: Arc<NotifyInner>,
}

impl NotifyHub {
    pub fn start(engine: Arc<WorkspaceEngine>) -> Arc<Self> {
        let inner = Arc::new(NotifyInner {
            engine: Arc::clone(&engine),
            subscriptions: Mutex::new(HashMap::new()),
            streams: Mutex::new(HashMap::new()),
        });
        let stop = Arc::new(AtomicBool::new(false));
        let hub = Arc::new(Self {
            stop: Arc::clone(&stop),
            inner: Arc::clone(&inner),
        });
        std::thread::spawn(move || fanout(stop, inner, engine));
        hub
    }

    pub fn subscribe_resource(&self, session: &Session, uri: &str) -> Result<(), CommonroomError> {
        self.ensure_reader(session)?;
        self.known_uri(uri)?;
        self.inner
            .subscriptions
            .lock()
            .expect("subscriptions")
            .entry(session.session_id.clone())
            .or_default()
            .insert(uri.to_string());
        Ok(())
    }

    pub fn unsubscribe_resource(
        &self,
        session: &Session,
        uri: &str,
    ) -> Result<(), CommonroomError> {
        self.ensure_reader(session)?;
        self.known_uri(uri)?;
        if let Some(uris) = self
            .inner
            .subscriptions
            .lock()
            .expect("subscriptions")
            .get_mut(&session.session_id)
        {
            uris.remove(uri);
        }
        Ok(())
    }

    fn ensure_reader(&self, session: &Session) -> Result<(), CommonroomError> {
        if has_capability(&session.capabilities, "read") {
            Ok(())
        } else {
            Err(permission_denied("session is missing the read capability")
                .detail("required", "read"))
        }
    }

    fn known_uri(&self, uri: &str) -> Result<(), CommonroomError> {
        if uri.len() > 512 {
            return Err(invalid_request("resource uri is too long"));
        }
        if uri == SKILL_URI {
            return Err(invalid_request("resource does not change"));
        }
        if subscribable_resource(uri) {
            Ok(())
        } else {
            Err(invalid_request("unknown resource"))
        }
    }
}

impl Drop for NotifyHub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn fanout(stop: Arc<AtomicBool>, inner: Arc<NotifyInner>, engine: Arc<WorkspaceEngine>) {
    let subscription = engine.subscribe();
    while !stop.load(Ordering::Relaxed) {
        match subscription.recv_timeout(Duration::from_secs(1)) {
            Ok(Delivery::Event(event)) => dispatch(&inner, &event),
            Ok(Delivery::Lagged) => resync(&inner),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn dispatch(inner: &NotifyInner, event: &Event) {
    let uris = resources_for_event(&event.event_type);
    if uris.is_empty() {
        return;
    }
    let subscriptions = inner.subscriptions.lock().expect("subscriptions").clone();
    for (session_id, wanted) in subscriptions {
        let matched: Vec<&str> = uris
            .iter()
            .copied()
            .filter(|uri| wanted.contains(*uri))
            .collect();
        if matched.is_empty() {
            continue;
        }
        if !readable(inner, &session_id) {
            drop_session(inner, &session_id);
            continue;
        }
        for uri in matched {
            enqueue(inner, &session_id, Note::Updated(uri.to_string()));
        }
    }
}

fn resync(inner: &NotifyInner) {
    let session_ids: Vec<String> = inner
        .streams
        .lock()
        .expect("streams")
        .keys()
        .cloned()
        .collect();
    for session_id in session_ids {
        enqueue(inner, &session_id, Note::Resync);
    }
}

fn readable(inner: &NotifyInner, session_id: &str) -> bool {
    match inner.engine.authenticate(session_id) {
        Ok(session) => has_capability(&session.capabilities, "read"),
        Err(_) => false,
    }
}

fn drop_session(inner: &NotifyInner, session_id: &str) {
    inner
        .subscriptions
        .lock()
        .expect("subscriptions")
        .remove(session_id);
    inner.streams.lock().expect("streams").remove(session_id);
}

fn enqueue(inner: &NotifyInner, session_id: &str, note: Note) {
    let mut streams = inner.streams.lock().expect("streams");
    let Some(tx) = streams.get(session_id) else {
        return;
    };
    if tx.try_send(note).is_err() {
        streams.remove(session_id);
    }
}

fn attach(inner: &NotifyInner, session_id: &str) -> Receiver<Note> {
    let (tx, rx) = mpsc::sync_channel(STREAM_CAPACITY);
    inner
        .streams
        .lock()
        .expect("streams")
        .insert(session_id.to_string(), tx);
    rx
}

fn detach(inner: &NotifyInner, session_id: &str) {
    inner.streams.lock().expect("streams").remove(session_id);
}

pub fn wants_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"))
}

pub fn observe_response(
    engine: Arc<WorkspaceEngine>,
    session_id: String,
    cursor: i64,
    since_version: Option<i64>,
) -> Response {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(STREAM_CAPACITY);
    std::thread::spawn(move || pump_observe(engine, session_id, cursor, since_version, tx));
    let stream = async_stream::stream! {
        while let Some(frame) = rx.recv().await {
            yield Ok::<Bytes, std::io::Error>(Bytes::from(frame));
        }
    };
    event_stream(stream)
}

pub fn mcp_stream_response(
    hub: Arc<NotifyHub>,
    engine: Arc<WorkspaceEngine>,
    session_id: String,
) -> Response {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(STREAM_CAPACITY);
    let inner = Arc::clone(&hub.inner);
    std::thread::spawn(move || pump_mcp(inner, engine, session_id, tx));
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

fn event_stream(
    stream: impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
) -> Response {
    let body = axum::body::Body::from_stream(stream);
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "text/event-stream".parse().expect("content type"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        "no-cache".parse().expect("cache control"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", "no".parse().expect("buffering"));
    response
}

fn pump_observe(
    engine: Arc<WorkspaceEngine>,
    session_id: String,
    mut cursor: i64,
    since_version: Option<i64>,
    tx: Sender<String>,
) {
    let subscription = engine.subscribe();
    if !observe_ok(&engine, &session_id) {
        return;
    }
    if cursor > 0 {
        if let Ok(Some(oldest)) = engine.oldest_event_id() {
            if oldest > cursor + 1 && tx.blocking_send(resync_frame("gap")).is_err() {
                return;
            }
        }
    }
    if !send_replay(&engine, &mut cursor, since_version, &tx) {
        return;
    }
    if tx.blocking_send("retry: 5000\n\n".into()).is_err() {
        return;
    }
    loop {
        match subscription.recv_timeout(KEEPALIVE) {
            Ok(Delivery::Event(event)) => {
                if !observe_ok(&engine, &session_id) {
                    return;
                }
                if !forward_live(&event, &mut cursor, since_version, &tx) {
                    return;
                }
            }
            Ok(Delivery::Lagged) => {
                if tx.blocking_send(resync_frame("lagged")).is_err()
                    || !send_replay(&engine, &mut cursor, since_version, &tx)
                {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !observe_ok(&engine, &session_id) {
                    return;
                }
                if !send_replay(&engine, &mut cursor, since_version, &tx)
                    || tx.blocking_send(": keepalive\n\n".into()).is_err()
                {
                    return;
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn forward_live(
    event: &Event,
    cursor: &mut i64,
    since_version: Option<i64>,
    tx: &Sender<String>,
) -> bool {
    if !is_newer(*cursor, event.event_id) {
        return true;
    }
    *cursor = event.event_id;
    if !passes_version(event, since_version) {
        return true;
    }
    tx.blocking_send(sse_frame(event)).is_ok()
}

fn send_replay(
    engine: &WorkspaceEngine,
    cursor: &mut i64,
    since_version: Option<i64>,
    tx: &Sender<String>,
) -> bool {
    loop {
        let events = match engine.events_since(*cursor, None, 100) {
            Ok(events) => events,
            Err(_) => return false,
        };
        if events.is_empty() {
            return true;
        }
        let batch = events.len();
        for event in events {
            *cursor = event.event_id;
            if passes_version(&event, since_version) && tx.blocking_send(sse_frame(&event)).is_err()
            {
                return false;
            }
        }
        if batch < 100 {
            return true;
        }
    }
}

fn passes_version(event: &Event, since_version: Option<i64>) -> bool {
    let Some(since_version) = since_version else {
        return true;
    };
    match event.version {
        Some(version) => version > since_version,
        None => true,
    }
}

fn observe_ok(engine: &WorkspaceEngine, session_id: &str) -> bool {
    match engine.authenticate(session_id) {
        Ok(session) => has_capability(&session.capabilities, "observe"),
        Err(_) => false,
    }
}

fn pump_mcp(
    inner: Arc<NotifyInner>,
    engine: Arc<WorkspaceEngine>,
    session_id: String,
    tx: Sender<String>,
) {
    if !readable(&inner, &session_id) {
        return;
    }
    let notes = attach(&inner, &session_id);
    if tx.blocking_send("retry: 5000\n\n".into()).is_err() {
        detach(&inner, &session_id);
        return;
    }
    if !emit_current(&inner, &session_id, &tx) {
        detach(&inner, &session_id);
        return;
    }
    loop {
        match notes.recv_timeout(KEEPALIVE) {
            Ok(Note::Updated(uri)) => {
                if !readable(&inner, &session_id) || tx.blocking_send(mcp_frame(&uri)).is_err() {
                    break;
                }
            }
            Ok(Note::Resync) => {
                if !emit_current(&inner, &session_id, &tx) {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !readable(&inner, &session_id)
                    || tx.blocking_send(": keepalive\n\n".into()).is_err()
                {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    detach(&inner, &session_id);
    let _ = engine;
}

fn emit_current(inner: &NotifyInner, session_id: &str, tx: &Sender<String>) -> bool {
    if !readable(inner, session_id) {
        return false;
    }
    let uris = inner
        .subscriptions
        .lock()
        .expect("subscriptions")
        .get(session_id)
        .map(|uris| {
            let mut listed: Vec<_> = uris.iter().cloned().collect();
            listed.sort();
            listed
        })
        .unwrap_or_default();
    for uri in uris {
        if tx.blocking_send(mcp_frame(&uri)).is_err() {
            return false;
        }
    }
    true
}

fn sse_frame(event: &Event) -> String {
    let data = serde_json::to_string(event).unwrap_or_else(|_| "{}".into());
    format!("id: {}\ndata: {data}\n\n", event.event_id)
}

fn resync_frame(reason: &str) -> String {
    format!("event: resync\ndata: {{\"reason\":\"{reason}\"}}\n\n")
}

fn mcp_frame(uri: &str) -> String {
    let payload = json!({
        "jsonrpc": "2.0",
        "method": "notifications/resources/updated",
        "params": {"uri": uri}
    });
    format!("event: message\ndata: {payload}\n\n")
}

pub fn cursor_from(headers: &HeaderMap, query_cursor: i64) -> i64 {
    headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(query_cursor)
        .max(0)
}
