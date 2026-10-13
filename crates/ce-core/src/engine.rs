use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use ce_protocol::{
    capabilities_from_permissions, encode_invitation, has_capability, internal_error,
    invalid_region, invalid_request, invitation_expired, invitation_redeemed, invitation_revoked,
    lease_conflict, limit_exceeded, normalize_capabilities, permission_denied, proposal_conflict,
    proposal_not_found, session_expired, stale_version, unauthorized, version_not_found,
    CommonroomError, Limits,
};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::bus::EventBus;
use crate::events::{
    compose_event, redact_event_payload, Event, ARTIFACT_CHANGED, INVITE_CREATED, INVITE_REVOKED,
    LEASE_ACQUIRED, LEASE_EXPIRED, LEASE_RELEASED, PARTICIPANT_JOINED, PARTICIPANT_LEFT,
    PROPOSAL_APPROVED, PROPOSAL_COMMITTED, PROPOSAL_CONFLICTED, PROPOSAL_CREATED,
    PROPOSAL_REJECTED, PROPOSAL_SUPERSEDED, SESSION_CREATED, SESSION_EXPIRED, WORKSPACE_CREATED,
    WORKSPACE_STARTED, WORKSPACE_STOPPED,
};
use crate::markdown::{self, Heading};
use crate::paths::resolve_user_path;
use crate::{EventFilter, Subscription, SUBSCRIBER_CAPACITY};

mod collaboration;

pub use collaboration::{Handoff, HandoffCreated, NewHandoff, TaskView};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS versions (
    version INTEGER PRIMARY KEY,
    content TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    summary TEXT NOT NULL,
    participant_id TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS participants (
    participant_id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    capabilities TEXT NOT NULL,
    joined_at TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    left_at TEXT
);
CREATE TABLE IF NOT EXISTS sessions (
    session_id TEXT PRIMARY KEY,
    participant_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    capabilities TEXT NOT NULL,
    transport TEXT NOT NULL,
    issued_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    ended_at TEXT
);
CREATE TABLE IF NOT EXISTS leases (
    lease_id TEXT PRIMARY KEY,
    participant_id TEXT NOT NULL,
    target TEXT NOT NULL,
    acquired_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    released_at TEXT
);
CREATE TABLE IF NOT EXISTS proposals (
    proposal_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    participant_id TEXT NOT NULL,
    target TEXT NOT NULL,
    base_version INTEGER NOT NULL,
    summary TEXT NOT NULL,
    change_json TEXT NOT NULL,
    status TEXT NOT NULL,
    review_by TEXT,
    reviewed_at TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS invites (
    token TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    permissions TEXT NOT NULL,
    target TEXT,
    expires_at TEXT NOT NULL,
    max_uses INTEGER,
    used_count INTEGER NOT NULL DEFAULT 0,
    revoked_at TEXT,
    created_at TEXT NOT NULL,
    capabilities TEXT,
    invite_id TEXT
);
CREATE TABLE IF NOT EXISTS events (
    event_id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS handoffs (
    handoff_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    creator_id TEXT NOT NULL,
    assignee_id TEXT,
    summary TEXT NOT NULL,
    constraints TEXT NOT NULL,
    artifact_refs TEXT NOT NULL,
    status TEXT NOT NULL,
    result_text TEXT,
    correlation_id TEXT NOT NULL,
    idempotency_key TEXT,
    task_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
    task_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    assignee_id TEXT,
    handoff_id TEXT,
    operation TEXT NOT NULL,
    status TEXT NOT NULL,
    status_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    ttl_ms INTEGER NOT NULL,
    poll_interval_ms INTEGER NOT NULL,
    result_json TEXT,
    error_json TEXT,
    cancel_requested INTEGER NOT NULL DEFAULT 0,
    correlation_id TEXT NOT NULL,
    idempotency_key TEXT,
    schema_version INTEGER NOT NULL,
    input_requests_json TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS handoffs_idempotency
    ON handoffs(creator_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;
";

#[derive(Clone)]
pub struct Clock(Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>);

impl Clock {
    pub fn system() -> Self {
        Self(Arc::new(Utc::now))
    }

    #[cfg(test)]
    pub(crate) fn fixed(at: DateTime<Utc>) -> Self {
        Self(Arc::new(move || at))
    }

    fn now(&self) -> DateTime<Utc> {
        (self.0)()
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::system()
    }
}

fn format_time(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Micros, false)
}

fn parse_time(value: &str) -> Result<DateTime<Utc>, CommonroomError> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.with_timezone(&Utc));
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f") {
        return Ok(naive.and_utc());
    }
    Err(invalid_request(format!("invalid timestamp {value}")))
}

fn db_err(err: rusqlite::Error) -> CommonroomError {
    internal_error(err.to_string())
}

trait Db<T> {
    fn db(self) -> Result<T, CommonroomError>;
}

impl<T> Db<T> for rusqlite::Result<T> {
    fn db(self) -> Result<T, CommonroomError> {
        self.map_err(db_err)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn token_hex(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    getrandom::getrandom(&mut buf).expect("entropy");
    hex::encode(buf)
}

fn token_urlsafe(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    getrandom::getrandom(&mut buf).expect("entropy");
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
}

fn next_id(prefix: &str) -> String {
    format!("{prefix}_{}", token_hex(8))
}

fn json_text(value: &impl Serialize) -> Result<String, CommonroomError> {
    serde_json::to_string(value).map_err(|err| internal_error(err.to_string()))
}

fn parse_caps(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

#[derive(Clone, Debug, Serialize)]
pub struct Session {
    pub session_id: String,
    pub participant_id: String,
    pub workspace_id: String,
    pub kind: String,
    pub name: String,
    pub capabilities: Vec<String>,
    pub transport: String,
    pub issued_at: String,
    pub expires_at: String,
    pub last_seen: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Region {
    pub region: String,
    pub title: String,
    pub level: usize,
    pub line: usize,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct OutlineItem {
    pub title: String,
    pub level: usize,
    pub line: usize,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub workspace_id: String,
    pub lifecycle: String,
    pub version: i64,
    pub substrate: String,
    pub path: Option<String>,
    pub name: String,
    pub content_hash: String,
    pub participants: usize,
    pub active_leases: usize,
    pub pending_proposals: usize,
    pub heading_tree: Vec<Region>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReadResult {
    pub workspace_id: String,
    pub version: i64,
    pub section: Option<String>,
    pub content: String,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub workspace_id: String,
    pub version: i64,
    pub outline: Vec<OutlineItem>,
    pub excerpt: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiffResult {
    pub workspace_id: String,
    pub base_version: i64,
    pub current_version: i64,
    pub diff: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Change {
    pub version: i64,
    pub summary: String,
    pub participant_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangesSince {
    pub workspace_id: String,
    pub from_version: i64,
    pub to_version: i64,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Participant {
    pub participant_id: String,
    pub kind: String,
    pub name: String,
    pub capabilities: Vec<String>,
    pub joined_at: String,
    pub last_seen: String,
    pub transport: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Lease {
    pub lease_id: String,
    pub participant_id: String,
    pub target: String,
    pub expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acquired_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Proposal {
    pub proposal_id: String,
    pub workspace_id: String,
    pub participant_id: String,
    pub target: String,
    pub base_version: i64,
    pub summary: String,
    pub change: Value,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProposalCreated {
    pub proposal_id: String,
    pub workspace_id: String,
    pub participant_id: String,
    pub target: String,
    pub base_version: i64,
    pub summary: String,
    pub change: Value,
    pub status: String,
    pub created_at: String,
    pub base_is_current: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewResult {
    pub proposal_id: String,
    pub reviewer_id: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommitResult {
    pub version: i64,
    pub summary: String,
    pub participant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Invite {
    pub invite_id: String,
    pub token: String,
    pub uri: String,
    pub workspace_id: String,
    pub permissions: String,
    pub capabilities: Vec<String>,
    pub target: Option<String>,
    pub expires_at: String,
    pub max_uses: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ListedInvite {
    pub invite_id: Option<String>,
    pub capabilities: Vec<String>,
    pub target: Option<String>,
    pub expires_at: String,
    pub max_uses: Option<i64>,
    pub used_count: i64,
    pub revoked_at: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticParticipant {
    pub participant_id: String,
    pub name: String,
    pub kind: String,
    pub transport: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostics {
    pub listener: Value,
    pub participants: Vec<DiagnosticParticipant>,
}

struct EngineState {
    workspace_path: PathBuf,
    db_path: PathBuf,
    limits: Limits,
    clock: Clock,
    conn: Option<Connection>,
    bus: EventBus,
}

pub struct WorkspaceEngine {
    inner: Mutex<EngineState>,
}

impl WorkspaceEngine {
    pub fn open(
        workspace_path: impl AsRef<Path>,
        db_path: Option<PathBuf>,
        limits: Limits,
        clock: Option<Clock>,
    ) -> Result<Self, CommonroomError> {
        let workspace_path = resolve_user_path(workspace_path.as_ref());
        if workspace_path.exists() && !workspace_path.is_file() {
            return Err(invalid_request("workspace_path must be a file"));
        }
        if !workspace_path.exists() {
            if let Some(parent) = workspace_path.parent() {
                std::fs::create_dir_all(parent).map_err(|err| invalid_request(err.to_string()))?;
            }
            std::fs::write(&workspace_path, "").map_err(|err| invalid_request(err.to_string()))?;
        }
        let db_path = match db_path {
            Some(path) => resolve_user_path(&path),
            None => default_db_path(&workspace_path),
        };
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| invalid_request(err.to_string()))?;
        }
        let conn = Connection::open(&db_path).db()?;
        conn.busy_timeout(Duration::from_millis(3000)).db()?;
        conn.pragma_update(None, "journal_mode", "WAL").db()?;
        let state = EngineState {
            workspace_path,
            db_path,
            limits,
            clock: clock.unwrap_or_else(Clock::system),
            conn: Some(conn),
            bus: EventBus::new(),
        };
        let engine = Self {
            inner: Mutex::new(state),
        };
        {
            let guard = engine.lock();
            guard.init_schema()?;
            guard.bootstrap()?;
            guard.recover()?;
        }
        Ok(engine)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, EngineState> {
        self.inner.lock().expect("workspace lock")
    }

    pub fn close(&self) {
        self.lock().conn = None;
    }

    pub fn limits(&self) -> Limits {
        self.lock().limits
    }

    pub fn workspace_path(&self) -> PathBuf {
        self.lock().workspace_path.clone()
    }

    pub fn db_path(&self) -> PathBuf {
        self.lock().db_path.clone()
    }

    pub fn workspace_id(&self) -> Result<String, CommonroomError> {
        self.lock().workspace_id()
    }

    pub fn require(&self, session: &Session, capability: &str) -> Result<(), CommonroomError> {
        if !has_capability(&session.capabilities, capability) {
            return Err(permission_denied(format!(
                "session is missing the {capability} capability"
            ))
            .detail("required", capability));
        }
        Ok(())
    }

    pub fn mark_started(&self) -> Result<(), CommonroomError> {
        self.lock().mark_started()
    }

    pub fn mark_stopped(&self) -> Result<(), CommonroomError> {
        self.lock().mark_stopped()
    }

    pub fn set_transport(&self, description: Option<Value>) -> Result<(), CommonroomError> {
        self.lock().set_transport(description)
    }

    pub fn current_transport(&self) -> Result<Option<Value>, CommonroomError> {
        self.lock().current_transport()
    }

    pub fn status(&self) -> Result<Status, CommonroomError> {
        self.lock().status()
    }

    pub fn regions(&self) -> Result<Vec<Region>, CommonroomError> {
        self.lock().regions()
    }

    pub fn read(
        &self,
        section: Option<&str>,
        max_lines: Option<i64>,
        max_bytes: Option<i64>,
    ) -> Result<ReadResult, CommonroomError> {
        self.lock().read(section, max_lines, max_bytes)
    }

    pub fn summary(
        &self,
        max_lines: i64,
        max_bytes: i64,
        max_depth: Option<i64>,
    ) -> Result<Summary, CommonroomError> {
        self.lock().summary(max_lines, max_bytes, max_depth)
    }

    pub fn diff(&self, base_version: i64) -> Result<DiffResult, CommonroomError> {
        self.lock().diff(base_version)
    }

    pub fn history(
        &self,
        limit: i64,
        max_items: Option<i64>,
    ) -> Result<Vec<Event>, CommonroomError> {
        self.lock().history(limit, max_items)
    }

    pub fn events_since(
        &self,
        since_event_id: i64,
        since_version: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Event>, CommonroomError> {
        self.lock()
            .events_since(since_event_id, since_version, limit)
    }

    pub fn oldest_event_id(&self) -> Result<Option<i64>, CommonroomError> {
        self.lock().oldest_event_id()
    }

    pub fn subscribe(&self) -> Subscription {
        self.subscribe_with(EventFilter::all(), SUBSCRIBER_CAPACITY)
    }

    pub fn subscribe_with(&self, filter: EventFilter, capacity: usize) -> Subscription {
        let bus = self.lock().bus.clone();
        bus.subscribe(filter, capacity)
    }

    pub fn changes_since(
        &self,
        version: i64,
        limit: i64,
        max_items: Option<i64>,
    ) -> Result<ChangesSince, CommonroomError> {
        self.lock().changes_since(version, limit, max_items)
    }

    pub fn participants(&self) -> Result<Vec<Participant>, CommonroomError> {
        self.lock().participants()
    }

    pub fn ensure_host(&self, name: &str, transport: &str) -> Result<Session, CommonroomError> {
        self.lock().ensure_host(name, transport)
    }

    pub fn join(
        &self,
        kind: &str,
        name: &str,
        capabilities: Option<Vec<String>>,
        invite_token: Option<&str>,
        transport: &str,
        session_minutes: Option<i64>,
    ) -> Result<Session, CommonroomError> {
        self.lock().join(
            kind,
            name,
            capabilities,
            invite_token,
            transport,
            session_minutes,
        )
    }

    pub fn authenticate(&self, session_id: &str) -> Result<Session, CommonroomError> {
        self.lock().authenticate(session_id)
    }

    pub fn leave(&self, participant_id: &str) -> Result<(), CommonroomError> {
        self.lock().leave(participant_id)
    }

    pub fn leases(&self) -> Result<Vec<Lease>, CommonroomError> {
        self.lock().leases()
    }

    pub fn checkout(
        &self,
        participant_id: &str,
        target: &str,
        minutes: Option<i64>,
        session: Option<&Session>,
    ) -> Result<Lease, CommonroomError> {
        self.lock()
            .checkout(participant_id, target, minutes, session)
    }

    pub fn renew(
        &self,
        lease_id: &str,
        participant_id: &str,
        minutes: Option<i64>,
        session: Option<&Session>,
    ) -> Result<Lease, CommonroomError> {
        self.lock()
            .renew(lease_id, participant_id, minutes, session)
    }

    pub fn release(
        &self,
        lease_id: &str,
        participant_id: Option<&str>,
        admin: bool,
    ) -> Result<(), CommonroomError> {
        self.lock().release(lease_id, participant_id, admin)
    }

    pub fn proposals(&self) -> Result<Vec<Proposal>, CommonroomError> {
        self.lock().proposals()
    }

    pub fn propose(
        &self,
        participant_id: &str,
        target: &str,
        base_version: i64,
        summary: &str,
        change: Value,
        session: Option<&Session>,
    ) -> Result<ProposalCreated, CommonroomError> {
        self.lock().propose(
            participant_id,
            target,
            base_version,
            summary,
            change,
            session,
        )
    }

    pub fn review_proposal(
        &self,
        proposal_id: &str,
        reviewer_id: &str,
        approve: bool,
        session: Option<&Session>,
    ) -> Result<ReviewResult, CommonroomError> {
        self.lock()
            .review_proposal(proposal_id, reviewer_id, approve, session)
    }

    pub fn commit(
        &self,
        participant_id: &str,
        base_version: i64,
        summary: &str,
        content: &str,
        target: &str,
        session: Option<&Session>,
    ) -> Result<CommitResult, CommonroomError> {
        self.lock().commit(
            participant_id,
            base_version,
            summary,
            content,
            target,
            session,
        )
    }

    pub fn commit_proposal(
        &self,
        proposal_id: &str,
        participant_id: &str,
        session: Option<&Session>,
    ) -> Result<CommitResult, CommonroomError> {
        self.lock()
            .commit_proposal(proposal_id, participant_id, session)
    }

    pub fn connection_diagnostics(&self) -> Result<Diagnostics, CommonroomError> {
        self.lock().connection_diagnostics()
    }

    pub fn create_invite(&self) -> Result<Invite, CommonroomError> {
        self.create_invite_with(
            None,
            None,
            self.limits().default_invite_minutes,
            Some(1),
            None,
        )
    }

    pub fn create_invite_with(
        &self,
        permissions: Option<&str>,
        target: Option<&str>,
        expires_in_minutes: i64,
        max_uses: Option<i64>,
        capabilities: Option<Vec<String>>,
    ) -> Result<Invite, CommonroomError> {
        self.lock().create_invite(
            permissions,
            target,
            expires_in_minutes,
            max_uses,
            capabilities,
        )
    }

    pub fn list_invites(&self) -> Result<Vec<ListedInvite>, CommonroomError> {
        self.lock().list_invites()
    }

    pub fn revoke_invite(
        &self,
        token: Option<&str>,
        invite_id: Option<&str>,
    ) -> Result<(), CommonroomError> {
        self.lock().revoke_invite(token, invite_id)
    }
}

impl EngineState {
    fn conn(&self) -> Result<&Connection, CommonroomError> {
        self.conn
            .as_ref()
            .ok_or_else(|| invalid_request("workspace is closed"))
    }

    fn map_rows<T, P, F>(&self, sql: &str, params: P, mut map: F) -> Result<Vec<T>, CommonroomError>
    where
        P: rusqlite::Params,
        F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
    {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(sql).db()?;
        let rows = stmt.query_map(params, |row| map(row)).db()?;
        rows.collect::<std::result::Result<Vec<T>, _>>().db()
    }

    fn now(&self) -> DateTime<Utc> {
        self.clock.now()
    }

    fn now_text(&self) -> String {
        format_time(self.now())
    }

    fn meta(&self, key: &str) -> Result<Option<String>, CommonroomError> {
        self.conn()?
            .query_row(
                "SELECT value FROM metadata WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .db()
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<(), CommonroomError> {
        self.conn()?
            .execute(
                "INSERT INTO metadata (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .db()?;
        Ok(())
    }

    fn workspace_id(&self) -> Result<String, CommonroomError> {
        self.meta("workspace_id")?
            .ok_or_else(|| internal_error("workspace_id missing"))
    }

    fn current_version(&self) -> Result<i64, CommonroomError> {
        Ok(self
            .meta("current_version")?
            .unwrap_or_else(|| "1".into())
            .parse()
            .unwrap_or(1))
    }

    fn add_column(
        &self,
        table: &str,
        column: &str,
        definition: &str,
    ) -> Result<(), CommonroomError> {
        let exists = {
            let conn = self.conn()?;
            let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).db()?;
            let names = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .db()?
                .collect::<Result<Vec<_>, _>>()
                .db()?;
            names.iter().any(|name| name == column)
        };
        if !exists {
            self.conn()?
                .execute(
                    &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
                    [],
                )
                .db()?;
        }
        Ok(())
    }

    fn init_schema(&self) -> Result<(), CommonroomError> {
        self.conn()?.execute_batch(SCHEMA).db()?;
        self.add_column("participants", "left_at", "TEXT")?;
        self.add_column("invites", "capabilities", "TEXT")?;
        self.add_column("invites", "invite_id", "TEXT")?;
        self.add_column("invites", "token_hash", "TEXT")?;
        self.add_column("sessions", "invite_id", "TEXT")?;
        Ok(())
    }

    fn bootstrap(&self) -> Result<(), CommonroomError> {
        if self.meta("workspace_id")?.is_some() {
            return Ok(());
        }
        let workspace_id = next_id("workspace");
        let content = std::fs::read_to_string(&self.workspace_path)
            .map_err(|err| invalid_request(err.to_string()))?;
        self.set_meta("workspace_id", &workspace_id)?;
        self.set_meta("lifecycle", "ACTIVE")?;
        self.set_meta("current_version", "1")?;
        let now = self.now_text();
        self.conn()?
            .execute(
                "INSERT INTO versions(version, content, content_hash, summary, participant_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5)",
                params![1, content, sha256_hex(content.as_bytes()), "workspace created", now],
            )
            .db()?;
        self.record(WORKSPACE_CREATED, json!({"version": 1}))?;
        Ok(())
    }

    fn recover(&self) -> Result<(), CommonroomError> {
        self.expire_sessions()?;
        self.expire_leases()?;
        self.expire_tasks()?;
        Ok(())
    }

    pub(super) fn insert_event(
        &self,
        event_type: &str,
        mut payload: Value,
    ) -> Result<Event, CommonroomError> {
        redact_event_payload(&mut payload);
        let workspace_id = self.workspace_id()?;
        if let Some(map) = payload.as_object_mut() {
            map.insert("workspace_id".into(), Value::String(workspace_id));
        }
        let body = payload.to_string();
        let now = self.now_text();
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO events(event_type, payload_json, created_at) VALUES (?1, ?2, ?3)",
            params![event_type, body, now],
        )
        .db()?;
        let event_id = conn.last_insert_rowid();
        conn.execute(
            "DELETE FROM events WHERE event_id NOT IN (
                SELECT event_id FROM events ORDER BY event_id DESC LIMIT ?1
             )",
            params![self.limits.max_events as i64],
        )
        .db()?;
        Ok(compose_event(event_id, event_type, now, payload))
    }

    fn record(&self, event_type: &str, payload: Value) -> Result<(), CommonroomError> {
        let event = self.insert_event(event_type, payload)?;
        self.bus.publish(event);
        Ok(())
    }

    fn stored_content(&self) -> Result<String, CommonroomError> {
        let version = self.current_version()?;
        Ok(self
            .conn()?
            .query_row(
                "SELECT content FROM versions WHERE version = ?1",
                params![version],
                |row| row.get(0),
            )
            .optional()
            .db()?
            .unwrap_or_default())
    }

    fn sync_substrate(&self) -> Result<(), CommonroomError> {
        if !self.workspace_path.is_file() {
            return Ok(());
        }
        let disk = std::fs::read_to_string(&self.workspace_path)
            .map_err(|err| invalid_request(err.to_string()))?;
        if disk == self.stored_content()? {
            return Ok(());
        }
        self.append_version(None, "external substrate edit", &disk, false)?;
        Ok(())
    }

    fn read_current(&self) -> Result<String, CommonroomError> {
        self.sync_substrate()?;
        self.stored_content()
    }

    fn append_version(
        &self,
        participant_id: Option<&str>,
        summary: &str,
        content: &str,
        write_file: bool,
    ) -> Result<CommitResult, CommonroomError> {
        let new_version = self.current_version()? + 1;
        if write_file {
            std::fs::write(&self.workspace_path, content)
                .map_err(|err| invalid_request(err.to_string()))?;
        }
        let now = self.now_text();
        self.conn()?
            .execute(
                "INSERT INTO versions(version, content, content_hash, summary, participant_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    new_version,
                    content,
                    sha256_hex(content.as_bytes()),
                    summary,
                    participant_id,
                    now
                ],
            )
            .db()?;
        self.set_meta("current_version", &new_version.to_string())?;
        self.record(
            ARTIFACT_CHANGED,
            json!({
                "version": new_version,
                "summary": summary,
                "participant_id": participant_id,
            }),
        )?;
        Ok(CommitResult {
            version: new_version,
            summary: summary.to_string(),
            participant_id: participant_id.map(str::to_string),
            target: None,
        })
    }

    #[allow(dead_code)]
    fn headings(&self) -> Result<Vec<Heading>, CommonroomError> {
        Ok(markdown::headings(&self.read_current()?))
    }

    fn validate_target(&self, target: &str) -> Result<(), CommonroomError> {
        if target == "workspace" || target == "*" {
            return Ok(());
        }
        if markdown::span(&self.stored_content()?, target).is_none() {
            return Err(
                invalid_region(format!("section '{target}' not found")).detail("region", target)
            );
        }
        Ok(())
    }

    fn bound_limit(&self, limit: i64, max_items: Option<i64>) -> i64 {
        let chosen = match max_items {
            Some(max_items) => limit.min(max_items),
            None => limit,
        };
        chosen.clamp(1, self.limits.max_list_items)
    }

    fn mark_started(&self) -> Result<(), CommonroomError> {
        self.set_meta("lifecycle", "ACTIVE")?;
        let version = self.current_version()?;
        self.record(WORKSPACE_STARTED, json!({"version": version}))?;
        Ok(())
    }

    fn mark_stopped(&self) -> Result<(), CommonroomError> {
        let version = self.current_version()?;
        self.record(WORKSPACE_STOPPED, json!({"version": version}))?;
        Ok(())
    }

    fn set_transport(&self, description: Option<Value>) -> Result<(), CommonroomError> {
        let Some(description) = description else {
            self.set_meta("transport_json", "")?;
            return Ok(());
        };
        let mut payload = description.as_object().cloned().unwrap_or_default();
        payload.remove("pid");
        payload.insert("pid".into(), json!(std::process::id()));
        self.set_meta("transport_json", &Value::Object(payload).to_string())?;
        Ok(())
    }

    fn current_transport(&self) -> Result<Option<Value>, CommonroomError> {
        let raw = self.meta("transport_json")?.unwrap_or_default();
        if raw.is_empty() {
            return Ok(None);
        }
        let mut data: Value =
            serde_json::from_str(&raw).map_err(|err| internal_error(err.to_string()))?;
        if let Some(pid) = data.get("pid").and_then(Value::as_i64) {
            if (0..=i64::from(u32::MAX)).contains(&pid) && !pid_alive(pid as u32) {
                return Ok(None);
            }
        }
        if let Some(map) = data.as_object_mut() {
            map.remove("pid");
        }
        Ok(Some(data))
    }

    fn status(&self) -> Result<Status, CommonroomError> {
        let content = self.read_current()?;
        let proposals = self.proposals()?;
        Ok(Status {
            workspace_id: self.workspace_id()?,
            lifecycle: self.meta("lifecycle")?.unwrap_or_else(|| "ACTIVE".into()),
            version: self.current_version()?,
            substrate: "markdown".into(),
            path: Some(self.workspace_path.display().to_string()),
            name: self
                .workspace_path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| "workspace".into()),
            content_hash: sha256_hex(content.as_bytes()),
            participants: self.participants()?.len(),
            active_leases: self.leases()?.len(),
            pending_proposals: proposals
                .iter()
                .filter(|item| item.status == "pending")
                .count(),
            heading_tree: regions_from(&markdown::headings(&content)),
        })
    }

    fn regions(&self) -> Result<Vec<Region>, CommonroomError> {
        Ok(regions_from(&markdown::headings(&self.read_current()?)))
    }

    fn read(
        &self,
        section: Option<&str>,
        max_lines: Option<i64>,
        max_bytes: Option<i64>,
    ) -> Result<ReadResult, CommonroomError> {
        let content = self.read_current()?;
        let mut selected = content.clone();
        if let Some(section) = section {
            let span = markdown::span(&content, section).ok_or_else(|| {
                invalid_region(format!("section '{section}' not found")).detail("region", section)
            })?;
            selected = content
                .lines()
                .skip(span.0)
                .take(span.1 - span.0)
                .collect::<Vec<_>>()
                .join("\n");
        }
        let limited = markdown::limit_text(&selected, max_lines, max_bytes);
        Ok(ReadResult {
            workspace_id: self.workspace_id()?,
            version: self.current_version()?,
            section: section.map(str::to_string),
            truncated: limited != selected,
            content: limited,
        })
    }

    fn summary(
        &self,
        max_lines: i64,
        max_bytes: i64,
        max_depth: Option<i64>,
    ) -> Result<Summary, CommonroomError> {
        let content = self.read_current()?;
        let mut outline = markdown::headings(&content);
        if let Some(max_depth) = max_depth {
            outline.retain(|item| item.level as i64 <= max_depth);
        }
        let outline = outline
            .into_iter()
            .take(10)
            .map(|item| OutlineItem {
                title: item.title,
                level: item.level,
                line: item.line,
                path: item.path,
            })
            .collect();
        Ok(Summary {
            workspace_id: self.workspace_id()?,
            version: self.current_version()?,
            outline,
            excerpt: markdown::limit_text(&content, Some(max_lines), Some(max_bytes)),
        })
    }

    fn diff(&self, base_version: i64) -> Result<DiffResult, CommonroomError> {
        let old: String = self
            .conn()?
            .query_row(
                "SELECT content FROM versions WHERE version = ?1",
                params![base_version],
                |row| row.get(0),
            )
            .optional()
            .db()?
            .ok_or_else(|| {
                version_not_found("base version does not exist")
                    .detail("base_version", base_version)
            })?;
        let current = self.current_version()?;
        let new = self.read_current()?;
        Ok(DiffResult {
            workspace_id: self.workspace_id()?,
            base_version,
            current_version: current,
            diff: markdown::unified_diff(
                &old,
                &new,
                &format!("v{base_version}"),
                &format!("v{current}"),
            ),
        })
    }

    fn history(&self, limit: i64, max_items: Option<i64>) -> Result<Vec<Event>, CommonroomError> {
        let chosen = self.bound_limit(limit, max_items);
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT event_id, event_type, payload_json, created_at
                 FROM events ORDER BY event_id DESC LIMIT ?1",
            )
            .db()?;
        let rows = stmt
            .query_map(params![chosen], event_from_row)
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?;
        Ok(rows)
    }

    fn events_since(
        &self,
        since_event_id: i64,
        since_version: Option<i64>,
        limit: i64,
    ) -> Result<Vec<Event>, CommonroomError> {
        let chosen = self.bound_limit(limit, None);
        let events = self.map_rows(
            "SELECT event_id, event_type, payload_json, created_at
             FROM events WHERE event_id > ?1 ORDER BY event_id ASC LIMIT ?2",
            params![since_event_id, chosen],
            event_from_row,
        )?;
        if since_version.is_none() {
            return Ok(events);
        }
        let since_version = since_version.unwrap_or(0);
        Ok(events
            .into_iter()
            .filter(
                |event| match event.payload.get("version").and_then(Value::as_i64) {
                    Some(version) => version > since_version,
                    None => true,
                },
            )
            .collect())
    }

    fn oldest_event_id(&self) -> Result<Option<i64>, CommonroomError> {
        self.conn()?
            .query_row("SELECT MIN(event_id) FROM events", [], |row| row.get(0))
            .optional()
            .db()
    }

    fn changes_since(
        &self,
        version: i64,
        limit: i64,
        max_items: Option<i64>,
    ) -> Result<ChangesSince, CommonroomError> {
        let chosen = self.bound_limit(limit, max_items);
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT version, summary, participant_id, created_at
                 FROM versions WHERE version > ?1 ORDER BY version ASC LIMIT ?2",
            )
            .db()?;
        let changes = stmt
            .query_map(params![version, chosen], |row| {
                Ok(Change {
                    version: row.get(0)?,
                    summary: row.get(1)?,
                    participant_id: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?;
        Ok(ChangesSince {
            workspace_id: self.workspace_id()?,
            from_version: version,
            to_version: self.current_version()?,
            changes,
        })
    }

    fn participants(&self) -> Result<Vec<Participant>, CommonroomError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT participant_id, kind, name, capabilities, joined_at, last_seen,
                    (
                        SELECT transport FROM sessions
                        WHERE sessions.participant_id = participants.participant_id
                          AND sessions.ended_at IS NULL
                        ORDER BY issued_at DESC LIMIT 1
                    ) AS transport
                 FROM participants
                 WHERE left_at IS NULL
                 ORDER BY joined_at ASC",
            )
            .db()?;
        let rows = stmt
            .query_map([], |row| {
                let caps: String = row.get(3)?;
                Ok(Participant {
                    participant_id: row.get(0)?,
                    kind: row.get(1)?,
                    name: row.get(2)?,
                    capabilities: parse_caps(&caps),
                    joined_at: row.get(4)?,
                    last_seen: row.get(5)?,
                    transport: row.get(6)?,
                })
            })
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?;
        Ok(rows)
    }

    fn active_session_count(&self) -> Result<i64, CommonroomError> {
        self.expire_sessions()?;
        let count: i64 = self
            .conn()?
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE ended_at IS NULL",
                [],
                |row| row.get(0),
            )
            .db()?;
        Ok(count)
    }

    #[allow(clippy::too_many_arguments)]
    fn issue_session(
        &self,
        participant_id: &str,
        kind: &str,
        name: &str,
        capabilities: &[String],
        transport: &str,
        session_minutes: Option<i64>,
        invite_id: Option<&str>,
    ) -> Result<Session, CommonroomError> {
        if self.active_session_count()? as usize >= self.limits.max_active_sessions {
            return Err(limit_exceeded("too many active sessions"));
        }
        let minutes = session_minutes.unwrap_or(self.limits.default_session_minutes);
        if minutes > self.limits.max_session_minutes {
            return Err(limit_exceeded(format!(
                "session duration exceeds max ({} minutes)",
                self.limits.max_session_minutes
            )));
        }
        let now = self.now();
        let session_id = format!("ses_{}", token_urlsafe(24));
        let issued_at = format_time(now);
        let expires_at = format_time(now + chrono::Duration::minutes(minutes));
        let caps = json_text(&capabilities)?;
        let transport = truncate_chars(transport, 64);
        self.conn()?
            .execute(
                "INSERT INTO sessions(
                    session_id, participant_id, workspace_id, capabilities, transport,
                    issued_at, expires_at, last_seen, ended_at, invite_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, ?9)",
                params![
                    session_id,
                    participant_id,
                    self.workspace_id()?,
                    caps,
                    transport,
                    issued_at,
                    expires_at,
                    issued_at,
                    invite_id
                ],
            )
            .db()?;
        self.record(
            SESSION_CREATED,
            json!({
                "participant_id": participant_id,
                "transport": transport,
                "expires_at": expires_at,
            }),
        )?;
        Ok(Session {
            session_id,
            participant_id: participant_id.to_string(),
            workspace_id: self.workspace_id()?,
            kind: kind.to_string(),
            name: name.to_string(),
            capabilities: capabilities.to_vec(),
            transport,
            issued_at: issued_at.clone(),
            expires_at,
            last_seen: issued_at,
        })
    }

    fn ensure_host(&self, name: &str, transport: &str) -> Result<Session, CommonroomError> {
        let existing: Option<String> = self
            .conn()?
            .query_row(
                "SELECT participant_id FROM participants
                 WHERE kind = 'human' AND name = ?1 AND left_at IS NULL
                 ORDER BY joined_at ASC LIMIT 1",
                params![name],
                |row| row.get(0),
            )
            .optional()
            .db()?;
        let now = self.now_text();
        let participant_id = if let Some(participant_id) = existing {
            participant_id
        } else {
            let participant_id = next_id("participant");
            let caps = json_text(&vec!["*".to_string()])?;
            self.conn()?
                .execute(
                    "INSERT INTO participants(participant_id, kind, name, capabilities, joined_at, last_seen, left_at)
                     VALUES (?1, 'human', ?2, ?3, ?4, ?5, NULL)",
                    params![participant_id, name, caps, now, now],
                )
                .db()?;
            self.record(
                PARTICIPANT_JOINED,
                json!({
                    "participant_id": participant_id,
                    "kind": "human",
                    "name": name,
                    "capabilities": ["*"],
                }),
            )?;
            participant_id
        };
        let now = self.now_text();
        self.conn()?
            .execute(
                "UPDATE sessions SET ended_at = ?1 WHERE participant_id = ?2 AND ended_at IS NULL",
                params![now, participant_id],
            )
            .db()?;
        self.issue_session(
            &participant_id,
            "human",
            name,
            &["*".to_string()],
            transport,
            None,
            None,
        )
    }

    fn join(
        &self,
        kind: &str,
        name: &str,
        capabilities: Option<Vec<String>>,
        invite_token: Option<&str>,
        transport: &str,
        session_minutes: Option<i64>,
    ) -> Result<Session, CommonroomError> {
        if kind != "human" && kind != "agent" {
            return Err(invalid_request("kind must be 'human' or 'agent'"));
        }
        let mut granted = normalize_capabilities(&capabilities.unwrap_or_default())?;
        let mut invite_id = None;
        if let Some(token) = invite_token {
            let invite = self.consume_invite(token)?;
            granted = invite.capabilities;
            invite_id = invite.invite_id;
        }
        let participant_id = next_id("participant");
        let now = self.now_text();
        let caps = json_text(&granted)?;
        self.conn()?
            .execute(
                "INSERT INTO participants(participant_id, kind, name, capabilities, joined_at, last_seen, left_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
                params![participant_id, kind, name, caps, now, now],
            )
            .db()?;
        self.record(
            PARTICIPANT_JOINED,
            json!({
                "participant_id": participant_id,
                "kind": kind,
                "name": name,
                "capabilities": granted,
                "transport": transport,
            }),
        )?;
        self.issue_session(
            &participant_id,
            kind,
            name,
            &granted,
            transport,
            session_minutes,
            invite_id.as_deref(),
        )
    }

    fn authenticate(&self, session_id: &str) -> Result<Session, CommonroomError> {
        self.expire_sessions()?;
        let row = self
            .conn()?
            .query_row(
                "SELECT session_id, participant_id, workspace_id, capabilities, transport,
                    issued_at, expires_at, last_seen, ended_at
             FROM sessions WHERE session_id = ?1",
                params![session_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .db()?;
        let Some((
            session_id,
            participant_id,
            workspace_id,
            caps,
            transport,
            issued_at,
            expires_at,
            ended_at,
        )) = row
        else {
            return Err(unauthorized("session not found"));
        };
        if ended_at.is_some() {
            return Err(unauthorized("session not found"));
        }
        if parse_time(&expires_at)? <= self.now() {
            let now = self.now_text();
            self.conn()?
                .execute(
                    "UPDATE sessions SET ended_at = ?1 WHERE session_id = ?2",
                    params![now, session_id],
                )
                .db()?;
            self.record(SESSION_EXPIRED, json!({"participant_id": participant_id}))?;
            return Err(session_expired());
        }
        let now = self.now_text();
        self.conn()?
            .execute(
                "UPDATE sessions SET last_seen = ?1 WHERE session_id = ?2",
                params![now, session_id],
            )
            .db()?;
        self.conn()?
            .execute(
                "UPDATE participants SET last_seen = ?1 WHERE participant_id = ?2",
                params![now, participant_id],
            )
            .db()?;
        let person: Option<(String, String)> = self
            .conn()?
            .query_row(
                "SELECT kind, name FROM participants WHERE participant_id = ?1",
                params![participant_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .db()?;
        let (kind, name) = person.unwrap_or_else(|| ("agent".into(), "participant".into()));
        Ok(Session {
            session_id,
            participant_id,
            workspace_id,
            kind,
            name,
            capabilities: parse_caps(&caps),
            transport,
            issued_at,
            expires_at,
            last_seen: now,
        })
    }

    fn expire_sessions(&self) -> Result<(), CommonroomError> {
        let now = self.now();
        let rows: Vec<(String, String, String)> = self.map_rows(
            "SELECT session_id, participant_id, expires_at FROM sessions WHERE ended_at IS NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        for (session_id, participant_id, expires_at) in rows {
            if parse_time(&expires_at)? > now {
                continue;
            }
            let stamp = format_time(now);
            self.conn()?
                .execute(
                    "UPDATE sessions SET ended_at = ?1 WHERE session_id = ?2 AND ended_at IS NULL",
                    params![stamp, session_id],
                )
                .db()?;
            self.record(SESSION_EXPIRED, json!({"participant_id": participant_id}))?;
        }
        Ok(())
    }

    fn leave(&self, participant_id: &str) -> Result<(), CommonroomError> {
        let now = self.now_text();
        let updated = self
            .conn()?
            .execute(
                "UPDATE participants SET left_at = ?1 WHERE participant_id = ?2 AND left_at IS NULL",
                params![now, participant_id],
            )
            .db()?;
        if updated == 0 {
            return Ok(());
        }
        self.conn()?
            .execute(
                "UPDATE sessions SET ended_at = ?1 WHERE participant_id = ?2 AND ended_at IS NULL",
                params![now, participant_id],
            )
            .db()?;
        let leases: Vec<String> = self.map_rows(
            "SELECT lease_id FROM leases WHERE participant_id = ?1 AND released_at IS NULL",
            params![participant_id],
            |row| row.get(0),
        )?;
        for lease_id in leases {
            self.release(&lease_id, None, false)?;
        }
        self.record(PARTICIPANT_LEFT, json!({"participant_id": participant_id}))?;
        Ok(())
    }

    fn expire_leases(&self) -> Result<(), CommonroomError> {
        let now = self.now();
        let rows: Vec<(String, String, String, String)> = self.map_rows(
            "SELECT lease_id, participant_id, target, expires_at
             FROM leases WHERE released_at IS NULL",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        for (lease_id, participant_id, target, expires_at) in rows {
            if parse_time(&expires_at)? > now {
                continue;
            }
            let stamp = format_time(now);
            self.conn()?
                .execute(
                    "UPDATE leases SET released_at = ?1 WHERE lease_id = ?2 AND released_at IS NULL",
                    params![stamp, lease_id],
                )
                .db()?;
            self.record(
                LEASE_EXPIRED,
                json!({"lease_id": lease_id, "participant_id": participant_id, "target": target}),
            )?;
        }
        Ok(())
    }

    fn leases(&self) -> Result<Vec<Lease>, CommonroomError> {
        self.expire_leases()?;
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT lease_id, participant_id, target, acquired_at, expires_at
                 FROM leases WHERE released_at IS NULL ORDER BY acquired_at ASC",
            )
            .db()?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Lease {
                    lease_id: row.get(0)?,
                    participant_id: row.get(1)?,
                    target: row.get(2)?,
                    acquired_at: Some(row.get(3)?),
                    expires_at: row.get(4)?,
                })
            })
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?;
        Ok(rows)
    }

    fn checkout(
        &self,
        participant_id: &str,
        target: &str,
        minutes: Option<i64>,
        session: Option<&Session>,
    ) -> Result<Lease, CommonroomError> {
        let participant_id = if let Some(session) = session {
            if !has_capability(&session.capabilities, "checkout") {
                return Err(
                    permission_denied("session is missing the checkout capability")
                        .detail("required", "checkout"),
                );
            }
            session.participant_id.clone()
        } else {
            participant_id.to_string()
        };
        let duration = minutes.unwrap_or(self.limits.default_lease_minutes);
        if duration <= 0 {
            return Err(invalid_request("lease duration must be positive"));
        }
        if duration > self.limits.max_lease_minutes {
            return Err(limit_exceeded(format!(
                "lease duration exceeds max ({} minutes)",
                self.limits.max_lease_minutes
            )));
        }
        self.sync_substrate()?;
        self.validate_target(target)?;
        self.expire_leases()?;
        let now = self.now();
        let existing: Option<(String, String)> = self
            .conn()?
            .query_row(
                "SELECT lease_id, participant_id FROM leases
                 WHERE target = ?1 AND released_at IS NULL AND expires_at >= ?2",
                params![target, format_time(now)],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .db()?;
        if let Some((lease_id, holder)) = existing {
            if holder == participant_id {
                return self.renew(&lease_id, &participant_id, Some(duration), None);
            }
            return Err(lease_conflict("target is already leased")
                .detail("lease_id", lease_id)
                .detail("region", target));
        }
        let expires_at = format_time(now + chrono::Duration::minutes(duration));
        let lease_id = next_id("lease");
        let acquired = format_time(now);
        self.conn()?
            .execute(
                "INSERT INTO leases(lease_id, participant_id, target, acquired_at, expires_at, released_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                params![lease_id, participant_id, target, acquired, expires_at],
            )
            .db()?;
        self.record(
            LEASE_ACQUIRED,
            json!({
                "lease_id": lease_id,
                "participant_id": participant_id,
                "target": target,
                "expires_at": expires_at,
            }),
        )?;
        Ok(Lease {
            lease_id,
            participant_id,
            target: target.to_string(),
            expires_at,
            acquired_at: None,
        })
    }

    fn renew(
        &self,
        lease_id: &str,
        participant_id: &str,
        minutes: Option<i64>,
        session: Option<&Session>,
    ) -> Result<Lease, CommonroomError> {
        let participant_id = if let Some(session) = session {
            if !has_capability(&session.capabilities, "checkout") {
                return Err(
                    permission_denied("session is missing the checkout capability")
                        .detail("required", "checkout"),
                );
            }
            session.participant_id.clone()
        } else {
            participant_id.to_string()
        };
        let duration = minutes.unwrap_or(self.limits.default_lease_minutes);
        let row: Option<(String, String, String, String, Option<String>)> = self
            .conn()?
            .query_row(
                "SELECT participant_id, target, acquired_at, expires_at, released_at FROM leases WHERE lease_id = ?1",
                params![lease_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()
            .db()?;
        let Some((holder, target, acquired_at, expires_at, released_at)) = row else {
            return Err(lease_conflict("lease is not active").detail("lease_id", lease_id));
        };
        if released_at.is_some() {
            return Err(lease_conflict("lease is not active").detail("lease_id", lease_id));
        }
        if holder != participant_id {
            return Err(permission_denied("lease is held by another participant")
                .detail("lease_id", lease_id));
        }
        if parse_time(&expires_at)? <= self.now() {
            return Err(lease_conflict("lease is not active").detail("lease_id", lease_id));
        }
        let acquired = parse_time(&acquired_at)?;
        let max_expiry = acquired + chrono::Duration::minutes(self.limits.max_lease_minutes);
        let requested = self
            .now()
            .checked_add_signed(chrono::Duration::minutes(duration))
            .unwrap_or(max_expiry);
        let requested = if requested > max_expiry {
            max_expiry
        } else {
            requested
        };
        if requested <= self.now() {
            return Err(
                limit_exceeded("lease cannot be renewed further").detail("lease_id", lease_id)
            );
        }
        let expires_at = format_time(requested);
        self.conn()?
            .execute(
                "UPDATE leases SET expires_at = ?1 WHERE lease_id = ?2",
                params![expires_at, lease_id],
            )
            .db()?;
        self.record(
            LEASE_ACQUIRED,
            json!({
                "lease_id": lease_id,
                "participant_id": participant_id,
                "target": target,
                "expires_at": expires_at,
            }),
        )?;
        Ok(Lease {
            lease_id: lease_id.to_string(),
            participant_id,
            target,
            expires_at,
            acquired_at: None,
        })
    }

    fn release(
        &self,
        lease_id: &str,
        participant_id: Option<&str>,
        admin: bool,
    ) -> Result<(), CommonroomError> {
        let row: Option<(String, Option<String>)> = self
            .conn()?
            .query_row(
                "SELECT participant_id, released_at FROM leases WHERE lease_id = ?1",
                params![lease_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .db()?;
        let Some((holder, released_at)) = row else {
            return Err(lease_conflict("lease not found").detail("lease_id", lease_id));
        };
        if let Some(participant_id) = participant_id {
            if participant_id != holder && !admin {
                return Err(permission_denied("lease is held by another participant")
                    .detail("lease_id", lease_id));
            }
        }
        if released_at.is_some() {
            return Ok(());
        }
        let now = self.now_text();
        self.conn()?
            .execute(
                "UPDATE leases SET released_at = ?1 WHERE lease_id = ?2 AND released_at IS NULL",
                params![now, lease_id],
            )
            .db()?;
        self.record(
            LEASE_RELEASED,
            json!({"lease_id": lease_id, "participant_id": holder}),
        )?;
        Ok(())
    }

    fn proposals(&self) -> Result<Vec<Proposal>, CommonroomError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT proposal_id, workspace_id, participant_id, target, base_version, summary,
                        change_json, status, created_at
                 FROM proposals ORDER BY created_at DESC",
            )
            .db()?;
        let rows = stmt
            .query_map([], |row| {
                let change: String = row.get(6)?;
                Ok(Proposal {
                    proposal_id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    participant_id: row.get(2)?,
                    target: row.get(3)?,
                    base_version: row.get(4)?,
                    summary: row.get(5)?,
                    change: serde_json::from_str(&change).unwrap_or(Value::Null),
                    status: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?;
        Ok(rows)
    }

    fn propose(
        &self,
        participant_id: &str,
        target: &str,
        base_version: i64,
        summary: &str,
        change: Value,
        session: Option<&Session>,
    ) -> Result<ProposalCreated, CommonroomError> {
        let participant_id = if let Some(session) = session {
            if !has_capability(&session.capabilities, "propose") {
                return Err(
                    permission_denied("session is missing the propose capability")
                        .detail("required", "propose"),
                );
            }
            session.participant_id.clone()
        } else {
            participant_id.to_string()
        };
        let content = change
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                invalid_request("proposal change must contain string field 'content'")
            })?;
        let _ = content;
        let encoded = serde_json::to_vec(&change).map_err(|err| internal_error(err.to_string()))?;
        if encoded.len() > self.limits.max_proposal_bytes {
            return Err(limit_exceeded("proposal is too large"));
        }
        self.sync_substrate()?;
        self.validate_target(target)?;
        if let Some(section) = change.get("section").and_then(Value::as_str) {
            if !section.is_empty() {
                self.validate_target(section)?;
            }
        }
        let proposal_id = next_id("proposal");
        let created_at = self.now_text();
        let current = self.current_version()?;
        self.conn()?
            .execute(
                "INSERT INTO proposals(
                    proposal_id, workspace_id, participant_id, target, base_version,
                    summary, change_json, status, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8)",
                params![
                    proposal_id,
                    self.workspace_id()?,
                    participant_id,
                    target,
                    base_version,
                    summary,
                    change.to_string(),
                    created_at
                ],
            )
            .db()?;
        self.record(
            PROPOSAL_CREATED,
            json!({
                "proposal_id": proposal_id,
                "workspace_id": self.workspace_id()?,
                "participant_id": participant_id,
                "target": target,
                "base_version": base_version,
                "summary": summary,
                "status": "pending",
                "created_at": created_at,
                "base_is_current": base_version == current,
            }),
        )?;
        Ok(ProposalCreated {
            proposal_id,
            workspace_id: self.workspace_id()?,
            participant_id,
            target: target.to_string(),
            base_version,
            summary: summary.to_string(),
            change,
            status: "pending".into(),
            created_at,
            base_is_current: base_version == current,
        })
    }

    fn review_proposal(
        &self,
        proposal_id: &str,
        reviewer_id: &str,
        approve: bool,
        session: Option<&Session>,
    ) -> Result<ReviewResult, CommonroomError> {
        let reviewer_id = if let Some(session) = session {
            if !has_capability(&session.capabilities, "approve") {
                return Err(
                    permission_denied("session is missing the approve capability")
                        .detail("required", "approve"),
                );
            }
            session.participant_id.clone()
        } else {
            reviewer_id.to_string()
        };
        let status: Option<String> = self
            .conn()?
            .query_row(
                "SELECT status FROM proposals WHERE proposal_id = ?1",
                params![proposal_id],
                |row| row.get(0),
            )
            .optional()
            .db()?;
        let Some(status) = status else {
            return Err(proposal_not_found("proposal not found").detail("proposal_id", proposal_id));
        };
        if status != "pending" {
            return Err(
                proposal_conflict("proposal is not pending").detail("proposal_id", proposal_id)
            );
        }
        let status = if approve { "approved" } else { "rejected" };
        let event_type = if approve {
            PROPOSAL_APPROVED
        } else {
            PROPOSAL_REJECTED
        };
        let now = self.now_text();
        self.conn()?
            .execute(
                "UPDATE proposals SET status = ?1, review_by = ?2, reviewed_at = ?3 WHERE proposal_id = ?4",
                params![status, reviewer_id, now, proposal_id],
            )
            .db()?;
        self.record(
            event_type,
            json!({"proposal_id": proposal_id, "reviewer_id": reviewer_id, "status": status}),
        )?;
        Ok(ReviewResult {
            proposal_id: proposal_id.to_string(),
            reviewer_id,
            status: status.to_string(),
        })
    }

    fn conflict(
        &self,
        base_version: i64,
        attempted: &str,
    ) -> Result<CommonroomError, CommonroomError> {
        let current_content = self.stored_content()?;
        let current_version = self.current_version()?;
        let diff = markdown::unified_diff(
            &current_content,
            attempted,
            &format!("v{current_version}"),
            &format!("attempted@v{base_version}"),
        );
        let changes = self.changes_since(base_version, 10, None)?;
        Ok(stale_version("workspace version conflict")
            .detail("base_version", base_version)
            .detail("expected_version", base_version)
            .detail("current_version", current_version)
            .detail("diff", diff)
            .detail(
                "changes",
                serde_json::to_value(&changes.changes).unwrap_or(Value::Null),
            ))
    }

    fn commit_content(
        &self,
        participant_id: Option<&str>,
        base_version: i64,
        summary: &str,
        content: &str,
    ) -> Result<CommitResult, CommonroomError> {
        self.sync_substrate()?;
        if base_version != self.current_version()? {
            return Err(self.conflict(base_version, content)?);
        }
        let payload = self.append_version(participant_id, summary, content, true)?;
        self.supersede_pending(base_version)?;
        Ok(payload)
    }

    fn supersede_pending(&self, base_version: i64) -> Result<(), CommonroomError> {
        let ids: Vec<String> = self.map_rows(
            "SELECT proposal_id FROM proposals WHERE status = 'pending' AND base_version <= ?1",
            params![base_version],
            |row| row.get(0),
        )?;
        if ids.is_empty() {
            return Ok(());
        }
        self.conn()?
            .execute(
                "UPDATE proposals SET status = 'superseded' WHERE status = 'pending' AND base_version <= ?1",
                params![base_version],
            )
            .db()?;
        for proposal_id in ids {
            self.record(PROPOSAL_SUPERSEDED, json!({"proposal_id": proposal_id}))?;
        }
        Ok(())
    }

    fn materialize(&self, change: &Value) -> Result<String, CommonroomError> {
        let content = change
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                proposal_conflict("proposal change must contain string field 'content'")
            })?;
        if let Some(section) = change.get("section").and_then(Value::as_str) {
            if !section.is_empty() {
                return markdown::replace_section(&self.stored_content()?, section, content);
            }
        }
        Ok(content.to_string())
    }

    fn commit(
        &self,
        participant_id: &str,
        base_version: i64,
        summary: &str,
        content: &str,
        target: &str,
        session: Option<&Session>,
    ) -> Result<CommitResult, CommonroomError> {
        let actor = if let Some(session) = session {
            if !has_capability(&session.capabilities, "admin") {
                return Err(permission_denied("session is missing the admin capability")
                    .detail("required", "admin"));
            }
            session.participant_id.clone()
        } else {
            participant_id.to_string()
        };
        let mut payload = self.commit_content(Some(&actor), base_version, summary, content)?;
        payload.target = Some(target.to_string());
        Ok(payload)
    }

    fn commit_proposal(
        &self,
        proposal_id: &str,
        participant_id: &str,
        session: Option<&Session>,
    ) -> Result<CommitResult, CommonroomError> {
        let actor = if let Some(session) = session {
            if !has_capability(&session.capabilities, "commit") {
                return Err(
                    permission_denied("session is missing the commit capability")
                        .detail("required", "commit"),
                );
            }
            session.participant_id.clone()
        } else {
            participant_id.to_string()
        };
        let row: Option<(i64, String, String, String)> = self
            .conn()?
            .query_row(
                "SELECT base_version, summary, change_json, status FROM proposals WHERE proposal_id = ?1",
                params![proposal_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .db()?;
        let Some((base_version, summary, change_json, status)) = row else {
            return Err(proposal_not_found("proposal not found").detail("proposal_id", proposal_id));
        };
        if status != "approved" {
            return Err(proposal_conflict("proposal must be approved before commit")
                .detail("proposal_id", proposal_id));
        }
        let change: Value = serde_json::from_str(&change_json).unwrap_or(Value::Null);
        let content = self.materialize(&change)?;
        let payload = match self.commit_content(Some(&actor), base_version, &summary, &content) {
            Ok(payload) => payload,
            Err(err) if err.code == "stale_version" => {
                self.conn()?
                    .execute(
                        "UPDATE proposals SET status = 'conflicted' WHERE proposal_id = ?1",
                        params![proposal_id],
                    )
                    .db()?;
                self.record(PROPOSAL_CONFLICTED, json!({"proposal_id": proposal_id}))?;
                return Err(err);
            }
            Err(err) => return Err(err),
        };
        self.conn()?
            .execute(
                "UPDATE proposals SET status = 'committed' WHERE proposal_id = ?1",
                params![proposal_id],
            )
            .db()?;
        self.record(
            PROPOSAL_COMMITTED,
            json!({"proposal_id": proposal_id, "version": payload.version}),
        )?;
        Ok(payload)
    }

    fn transport_for_invite(&self) -> Result<Value, CommonroomError> {
        Ok(self
            .current_transport()?
            .unwrap_or_else(|| json!({"type": "local", "mode": "local", "encrypted": false})))
    }

    fn connection_diagnostics(&self) -> Result<Diagnostics, CommonroomError> {
        let mut hint = self
            .current_transport()?
            .unwrap_or_else(|| json!({"type": "local", "mode": "local", "encrypted": false}));
        if let Some(map) = hint.as_object_mut() {
            for secret in ["address", "secret", "token", "base_url", "pid"] {
                map.remove(secret);
            }
        }
        let participants = self
            .participants()?
            .into_iter()
            .map(|person| DiagnosticParticipant {
                participant_id: person.participant_id,
                name: person.name,
                kind: person.kind,
                transport: person.transport,
            })
            .collect();
        Ok(Diagnostics {
            listener: hint,
            participants,
        })
    }

    fn create_invite(
        &self,
        permissions: Option<&str>,
        target: Option<&str>,
        expires_in_minutes: i64,
        max_uses: Option<i64>,
        capabilities: Option<Vec<String>>,
    ) -> Result<Invite, CommonroomError> {
        let granted = if let Some(capabilities) = capabilities {
            normalize_capabilities(&capabilities)?
        } else {
            capabilities_from_permissions(permissions)?
        };
        if expires_in_minutes > self.limits.max_invite_minutes {
            return Err(limit_exceeded(format!(
                "invitation duration exceeds max ({} minutes)",
                self.limits.max_invite_minutes
            )));
        }
        let token = token_urlsafe(24);
        let digest = sha256_hex(token.as_bytes());
        let invite_id = next_id("invite");
        let expires_at = format_time(self.now() + chrono::Duration::minutes(expires_in_minutes));
        let permissions_text = permissions
            .map(str::to_string)
            .unwrap_or_else(|| granted.join(","));
        let now = self.now_text();
        self.conn()?
            .execute(
                "INSERT INTO invites(
                    token, workspace_id, permissions, target, expires_at, max_uses,
                    used_count, revoked_at, created_at, capabilities, invite_id, token_hash
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, NULL, ?7, ?8, ?9, ?10)",
                params![
                    digest,
                    self.workspace_id()?,
                    permissions_text,
                    target,
                    expires_at,
                    max_uses,
                    now,
                    json_text(&granted)?,
                    invite_id,
                    digest
                ],
            )
            .db()?;
        let hint = self.transport_for_invite()?;
        let uri = encode_invitation(&json!({
            "transport": hint,
            "workspace_id": self.workspace_id()?,
            "secret": token,
            "expires_at": expires_at,
            "capabilities": granted,
        }))?;
        self.record(
            INVITE_CREATED,
            json!({
                "invite_id": invite_id,
                "capabilities": granted,
                "expires_at": expires_at,
                "max_uses": max_uses,
            }),
        )?;
        Ok(Invite {
            invite_id,
            token,
            uri,
            workspace_id: self.workspace_id()?,
            permissions: permissions_text,
            capabilities: granted,
            target: target.map(str::to_string),
            expires_at,
            max_uses,
        })
    }

    fn list_invites(&self) -> Result<Vec<ListedInvite>, CommonroomError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT invite_id, capabilities, permissions, target, expires_at, max_uses,
                        used_count, revoked_at, created_at
                 FROM invites ORDER BY created_at DESC",
            )
            .db()?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })
            .db()?
            .collect::<Result<Vec<_>, _>>()
            .db()?;
        let mut listed = Vec::new();
        for (
            invite_id,
            caps,
            permissions,
            target,
            expires_at,
            max_uses,
            used_count,
            revoked_at,
            created_at,
        ) in rows
        {
            let capabilities = if let Some(caps) = caps {
                parse_caps(&caps)
            } else {
                capabilities_from_permissions(Some(&permissions)).unwrap_or_default()
            };
            listed.push(ListedInvite {
                invite_id,
                capabilities,
                target,
                expires_at,
                max_uses,
                used_count,
                revoked_at,
                created_at,
            });
        }
        Ok(listed)
    }

    fn revoke_invite(
        &self,
        token: Option<&str>,
        invite_id: Option<&str>,
    ) -> Result<(), CommonroomError> {
        let row = if let Some(invite_id) = invite_id {
            self.invite_row(None, Some(invite_id))?
        } else if let Some(token) = token {
            self.invite_row(Some(token), None)?
        } else {
            return Err(invalid_request("token or invite_id is required"));
        };
        let Some(row) = row else {
            return Err(unauthorized("invite not found"));
        };
        if row.revoked_at.is_none() {
            let now = self.now_text();
            self.conn()?
                .execute(
                    "UPDATE invites SET revoked_at = ?1 WHERE token = ?2",
                    params![now, row.token],
                )
                .db()?;
            self.record(INVITE_REVOKED, json!({"invite_id": row.invite_id}))?;
        }
        let invite_key = row.invite_id.clone();
        let sessions: Vec<String> = self.map_rows(
            "SELECT DISTINCT participant_id FROM sessions WHERE invite_id = ?1 AND ended_at IS NULL",
            params![invite_key],
            |row| row.get(0),
        )?;
        for participant_id in sessions {
            self.leave(&participant_id)?;
        }
        Ok(())
    }

    fn invite_row(
        &self,
        secret: Option<&str>,
        invite_id: Option<&str>,
    ) -> Result<Option<InviteRow>, CommonroomError> {
        if let Some(invite_id) = invite_id {
            return self
                .conn()?
                .query_row(
                    "SELECT * FROM invites WHERE invite_id = ?1",
                    params![invite_id],
                    invite_from_row,
                )
                .optional()
                .db();
        }
        let Some(secret) = secret else {
            return Ok(None);
        };
        let digest = sha256_hex(secret.as_bytes());
        if let Some(row) = self
            .conn()?
            .query_row(
                "SELECT * FROM invites WHERE token_hash = ?1",
                params![digest],
                invite_from_row,
            )
            .optional()
            .db()?
        {
            return Ok(Some(row));
        }
        self.conn()?
            .query_row(
                "SELECT * FROM invites WHERE token = ?1 AND token_hash IS NULL",
                params![secret],
                invite_from_row,
            )
            .optional()
            .db()
    }

    fn consume_invite(&self, token: &str) -> Result<ConsumedInvite, CommonroomError> {
        let Some(row) = self.invite_row(Some(token), None)? else {
            return Err(unauthorized("invite not found"));
        };
        if row.revoked_at.is_some() {
            return Err(invitation_revoked());
        }
        if parse_time(&row.expires_at)? < self.now() {
            return Err(invitation_expired());
        }
        if let Some(max_uses) = row.max_uses {
            if row.used_count >= max_uses {
                return Err(raise_exhausted(row.used_count));
            }
        }
        let digest = sha256_hex(token.as_bytes());
        let mut stored = row.token.clone();
        if row.token_hash.is_none() {
            self.conn()?
                .execute(
                    "UPDATE invites SET token = ?1, token_hash = ?2 WHERE token = ?3",
                    params![digest, digest, stored],
                )
                .db()?;
            stored = digest;
        }
        let updated = self
            .conn()?
            .execute(
                "UPDATE invites SET used_count = used_count + 1
                 WHERE token = ?1 AND revoked_at IS NULL
                   AND (max_uses IS NULL OR used_count < max_uses)",
                params![stored],
            )
            .db()?;
        if updated != 1 {
            let again: Option<(i64, Option<String>, String)> = self
                .conn()?
                .query_row(
                    "SELECT used_count, revoked_at, expires_at FROM invites WHERE token = ?1",
                    params![stored],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
                .db()?;
            let Some((used, revoked, expires_at)) = again else {
                return Err(unauthorized("invite not found"));
            };
            if revoked.is_some() {
                return Err(invitation_revoked());
            }
            if parse_time(&expires_at)? < self.now() {
                return Err(invitation_expired());
            }
            return Err(raise_exhausted(used));
        }
        Ok(ConsumedInvite {
            capabilities: row.capabilities,
            invite_id: row.invite_id,
        })
    }
}

struct ConsumedInvite {
    capabilities: Vec<String>,
    invite_id: Option<String>,
}

struct InviteRow {
    token: String,
    token_hash: Option<String>,
    capabilities: Vec<String>,
    #[allow(dead_code)]
    permissions: String,
    expires_at: String,
    max_uses: Option<i64>,
    used_count: i64,
    revoked_at: Option<String>,
    invite_id: Option<String>,
}

fn invite_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InviteRow> {
    let caps: Option<String> = row.get("capabilities")?;
    let permissions: String = row.get("permissions")?;
    let capabilities = if let Some(caps) = caps.as_deref() {
        parse_caps(caps)
    } else {
        capabilities_from_permissions(Some(&permissions)).unwrap_or_default()
    };
    Ok(InviteRow {
        token: row.get("token")?,
        token_hash: row.get("token_hash")?,
        capabilities,
        permissions,
        expires_at: row.get("expires_at")?,
        max_uses: row.get("max_uses")?,
        used_count: row.get("used_count")?,
        revoked_at: row.get("revoked_at")?,
        invite_id: row.get("invite_id")?,
    })
}

fn event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    let payload: String = row.get(2)?;
    let created_at: String = row.get(3)?;
    let event_type: String = row.get(1)?;
    let event_id: i64 = row.get(0)?;
    Ok(compose_event(
        event_id,
        event_type,
        created_at,
        serde_json::from_str(&payload).unwrap_or(Value::Null),
    ))
}

fn regions_from(headings: &[Heading]) -> Vec<Region> {
    headings
        .iter()
        .map(|item| Region {
            region: item.path.clone(),
            title: item.title.clone(),
            level: item.level,
            line: item.line,
            path: item.path.clone(),
        })
        .collect()
}

fn raise_exhausted(used: i64) -> CommonroomError {
    if used > 0 {
        invitation_redeemed()
    } else {
        limit_exceeded("invite usage limit reached")
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn default_db_path(workspace: &Path) -> PathBuf {
    let mut name = workspace
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "workspace".into());
    name.push(".commonroom.db");
    workspace.with_file_name(name)
}

fn pid_alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    platform_pid_alive(pid)
}

#[cfg(unix)]
fn platform_pid_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn platform_pid_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn engine(content: &str) -> (TempDir, WorkspaceEngine) {
        let dir = std::env::temp_dir().join(format!("ce-engine-{}", token_hex(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workspace.md");
        std::fs::write(&path, content).unwrap();
        let engine = WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap();
        (TempDir(dir), engine)
    }

    #[test]
    fn workspace_create_read_modify_and_restart() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let agent = engine
            .join("agent", "agent-a", None, None, "local", None)
            .unwrap();
        assert_eq!(engine.status().unwrap().version, 1);
        let commit = engine
            .commit(
                &agent.participant_id,
                1,
                "update architecture",
                "# Architecture\nupdated\n",
                "workspace",
                None,
            )
            .unwrap();
        assert_eq!(commit.version, 2);
        assert!(engine
            .read(None, None, None)
            .unwrap()
            .content
            .contains("updated"));
        let db_path = engine.db_path();
        let ws_path = engine.workspace_path();
        engine.close();
        let reloaded =
            WorkspaceEngine::open(&ws_path, Some(db_path), Limits::default(), None).unwrap();
        assert_eq!(reloaded.status().unwrap().version, 2);
        assert!(reloaded
            .read(None, None, None)
            .unwrap()
            .content
            .contains("updated"));
    }

    #[test]
    fn lease_visibility_between_participants() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let a = engine
            .join("agent", "a", None, None, "local", None)
            .unwrap();
        let b = engine
            .join("human", "b", None, None, "local", None)
            .unwrap();
        let lease = engine
            .checkout(&a.participant_id, "section:Architecture", None, None)
            .unwrap();
        assert_eq!(engine.leases().unwrap()[0].lease_id, lease.lease_id);
        let err = engine
            .checkout(&b.participant_id, "section:Architecture", None, None)
            .unwrap_err();
        assert_eq!(err.code, "lease_conflict");
    }

    #[test]
    fn stale_commit_returns_structured_conflict() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let a = engine
            .join("agent", "a", None, None, "local", None)
            .unwrap();
        let b = engine
            .join("agent", "b", None, None, "local", None)
            .unwrap();
        engine
            .commit(
                &a.participant_id,
                1,
                "first",
                "# Architecture\none\n",
                "workspace",
                None,
            )
            .unwrap();
        let err = engine
            .commit(
                &b.participant_id,
                1,
                "stale",
                "# Architecture\ntwo\n",
                "workspace",
                None,
            )
            .unwrap_err();
        assert_eq!(err.code, "stale_version");
        assert_eq!(err.details["current_version"], 2);
        assert!(err.details["diff"]
            .as_str()
            .unwrap()
            .contains("attempted@v1"));
    }

    #[test]
    fn proposal_review_and_commit() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let agent = engine
            .join("agent", "writer", None, None, "local", None)
            .unwrap();
        let human = engine
            .join("human", "reviewer", None, None, "local", None)
            .unwrap();
        let proposal = engine
            .propose(
                &agent.participant_id,
                "section:Architecture",
                1,
                "refine architecture section",
                json!({"content": "# Architecture\nrefined\n"}),
                None,
            )
            .unwrap();
        assert_eq!(proposal.status, "pending");
        let review = engine
            .review_proposal(&proposal.proposal_id, &human.participant_id, true, None)
            .unwrap();
        assert_eq!(review.status, "approved");
        let commit = engine
            .commit_proposal(&proposal.proposal_id, &human.participant_id, None)
            .unwrap();
        assert_eq!(commit.version, 2);
        assert!(engine
            .read(None, None, None)
            .unwrap()
            .content
            .contains("refined"));
    }

    #[test]
    fn invite_expired_revoked_spent_and_hashed() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let expired = engine
            .create_invite_with(None, None, -1, Some(1), None)
            .unwrap();
        let err = engine
            .join("agent", "late", None, Some(&expired.token), "local", None)
            .unwrap_err();
        assert_eq!(err.code, "invitation_expired");

        let valid = engine
            .create_invite_with(None, None, 10, Some(1), None)
            .unwrap();
        let digest = sha256_hex(valid.token.as_bytes());
        let row: (String, String) = {
            let conn = Connection::open(engine.db_path()).unwrap();
            conn.query_row(
                "SELECT token, token_hash FROM invites WHERE invite_id = ?1",
                params![valid.invite_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(row, (digest.clone(), digest));
        engine.revoke_invite(Some(&valid.token), None).unwrap();
        let err = engine
            .join("agent", "blocked", None, Some(&valid.token), "local", None)
            .unwrap_err();
        assert_eq!(err.code, "invitation_revoked");

        let invite = engine
            .create_invite_with(None, None, 10, Some(1), None)
            .unwrap();
        let session = engine
            .join("agent", "first", None, Some(&invite.token), "local", None)
            .unwrap();
        let err = engine
            .join("agent", "replay", None, Some(&invite.token), "local", None)
            .unwrap_err();
        assert_eq!(err.code, "invitation_redeemed");
        let blob = format!("{:?}", engine.history(50, None).unwrap());
        assert!(!blob.contains(&invite.token));
        engine.revoke_invite(None, Some(&invite.invite_id)).unwrap();
        let err = engine.authenticate(&session.session_id).unwrap_err();
        assert_eq!(err.code, "unauthorized");
    }

    #[test]
    fn legacy_plaintext_invite_is_replaced_on_redemption() {
        let (_dir, engine) = engine("# Architecture\n");
        let secret = "legacy-invite-secret";
        let expires = {
            let guard = engine.lock();
            format_time(guard.now() + chrono::Duration::minutes(10))
        };
        let now = engine.lock().now_text();
        let workspace_id = engine.workspace_id().unwrap();
        {
            let guard = engine.lock();
            guard
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO invites(
                        token, workspace_id, permissions, expires_at, max_uses, used_count,
                        created_at, capabilities, invite_id
                     ) VALUES (?1, ?2, ?3, ?4, 1, 0, ?5, ?6, ?7)",
                    params![
                        secret,
                        workspace_id,
                        "read,observe",
                        expires,
                        now,
                        "[\"read\", \"observe\"]",
                        "invite_legacy"
                    ],
                )
                .unwrap();
        }
        engine
            .join("agent", "legacy", None, Some(secret), "local", None)
            .unwrap();
        let digest = sha256_hex(secret.as_bytes());
        let row: (String, String) = Connection::open(engine.db_path())
            .unwrap()
            .query_row(
                "SELECT token, token_hash FROM invites WHERE invite_id = ?1",
                params!["invite_legacy"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, (digest.clone(), digest));
    }

    #[test]
    fn summary_and_changes_are_bounded_and_external_edits_win() {
        let (_dir, engine) = engine("# Architecture\nline1\nline2\nline3\n");
        let agent = engine
            .join("agent", "a", None, None, "local", None)
            .unwrap();
        engine
            .commit(
                &agent.participant_id,
                1,
                "change",
                "# Architecture\nline1\nline2 updated\nline3\n",
                "workspace",
                None,
            )
            .unwrap();
        let summary = engine.summary(2, 20, None).unwrap();
        assert!(summary.excerpt.lines().count() <= 2);
        assert!(summary.excerpt.len() <= 20);
        let changes = engine.changes_since(1, 1, None).unwrap();
        assert_eq!(changes.to_version, 2);
        assert_eq!(changes.changes.len(), 1);

        std::fs::write(engine.workspace_path(), "# Architecture\nfrom disk\n").unwrap();
        assert_eq!(engine.status().unwrap().version, 3);
        assert!(engine
            .read(None, None, None)
            .unwrap()
            .content
            .contains("from disk"));
    }

    #[test]
    fn diagnostics_strip_the_endpoint() {
        let (_dir, engine) = engine("# Architecture\n");
        engine
            .set_transport(Some(json!({
                "type": "loopback",
                "address": "secret-endpoint",
                "port": 9,
                "encrypted": true,
                "mode": "private"
            })))
            .unwrap();
        let diagnostics = engine.connection_diagnostics().unwrap();
        let text = diagnostics.listener.to_string();
        assert!(!text.contains("secret-endpoint"));
        assert_eq!(diagnostics.listener["type"], "loopback");
        assert_eq!(diagnostics.listener["encrypted"], true);
    }

    #[test]
    fn a_successful_proposal_is_published_and_a_denied_one_is_not() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let writer = engine
            .join(
                "agent",
                "writer",
                Some(vec!["propose".into(), "read".into()]),
                None,
                "local",
                None,
            )
            .unwrap();
        let reader = engine
            .join(
                "agent",
                "reader",
                Some(vec!["read".into()]),
                None,
                "local",
                None,
            )
            .unwrap();
        let sub = engine.subscribe_with(crate::EventFilter::only(&[PROPOSAL_CREATED]), 8);
        let err = engine
            .propose(
                &reader.participant_id,
                "workspace",
                1,
                "nope",
                json!({"content": "secret-body"}),
                Some(&reader),
            )
            .unwrap_err();
        assert_eq!(err.code, "permission_denied");
        assert!(sub
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());
        let proposal = engine
            .propose(
                &writer.participant_id,
                "workspace",
                1,
                "sharpen",
                json!({"content": "secret-body-should-not-leak"}),
                Some(&writer),
            )
            .unwrap();
        match sub.recv_timeout(std::time::Duration::from_secs(1)).unwrap() {
            crate::Delivery::Event(event) => {
                assert_eq!(event.event_type, PROPOSAL_CREATED);
                assert_eq!(event.schema_version, crate::EVENT_SCHEMA_VERSION);
                assert_eq!(
                    event.actor_id.as_deref(),
                    Some(writer.participant_id.as_str())
                );
                assert_eq!(event.payload["proposal_id"], proposal.proposal_id);
                let blob = serde_json::to_string(&event).unwrap();
                assert!(!blob.contains("secret-body"));
                assert!(!blob.contains("ses_"));
            }
            crate::Delivery::Lagged => panic!("unexpected lag"),
        }
    }

    #[test]
    fn a_failed_commit_does_not_publish_an_artifact_change() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let agent = engine
            .join("agent", "writer", None, None, "local", None)
            .unwrap();
        engine
            .commit(
                &agent.participant_id,
                1,
                "first",
                "# Architecture\none\n",
                "workspace",
                None,
            )
            .unwrap();
        let before = engine
            .history(50, None)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == ARTIFACT_CHANGED)
            .count();
        let err = engine
            .commit(
                &agent.participant_id,
                1,
                "stale",
                "# Architecture\ntwo\n",
                "workspace",
                None,
            )
            .unwrap_err();
        assert_eq!(err.code, "stale_version");
        let after = engine
            .history(50, None)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == ARTIFACT_CHANGED)
            .count();
        assert_eq!(before, after);
    }

    #[test]
    fn releasing_a_lease_twice_publishes_one_release() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let agent = engine
            .join("agent", "writer", None, None, "local", None)
            .unwrap();
        let lease = engine
            .checkout(&agent.participant_id, "section:Architecture", None, None)
            .unwrap();
        engine
            .release(&lease.lease_id, Some(&agent.participant_id), false)
            .unwrap();
        engine
            .release(&lease.lease_id, Some(&agent.participant_id), false)
            .unwrap();
        let releases = engine
            .history(50, None)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == LEASE_RELEASED)
            .count();
        assert_eq!(releases, 1);
    }

    #[test]
    fn conflicted_proposal_records_the_conflict_and_not_a_commit() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let agent = engine
            .join("agent", "writer", None, None, "local", None)
            .unwrap();
        let human = engine
            .join("human", "reviewer", None, None, "local", None)
            .unwrap();
        let proposal = engine
            .propose(
                &agent.participant_id,
                "workspace",
                1,
                "edit",
                json!({"content": "# Architecture\nproposed\n"}),
                None,
            )
            .unwrap();
        engine
            .review_proposal(&proposal.proposal_id, &human.participant_id, true, None)
            .unwrap();
        engine
            .commit(
                &human.participant_id,
                1,
                "other",
                "# Architecture\nlanded\n",
                "workspace",
                None,
            )
            .unwrap();
        let err = engine
            .commit_proposal(&proposal.proposal_id, &agent.participant_id, None)
            .unwrap_err();
        assert_eq!(err.code, "stale_version");
        let history = engine.history(50, None).unwrap();
        assert!(history
            .iter()
            .any(|event| event.event_type == PROPOSAL_CONFLICTED));
        assert!(!history.iter().any(|event| {
            event.event_type == PROPOSAL_COMMITTED
                && event.payload["proposal_id"] == proposal.proposal_id
        }));
        assert!(history
            .iter()
            .all(|event| crate::is_known_event_type(&event.event_type)));
        let blob = format!("{history:?}");
        assert!(!blob.contains("proposed"));
    }

    #[test]
    fn concurrent_commits_keep_a_single_version_order() {
        let (_dir, engine) = engine("# Architecture\ninitial\n");
        let engine = std::sync::Arc::new(engine);
        let mut handles = Vec::new();
        for index in 0..4 {
            let engine = std::sync::Arc::clone(&engine);
            handles.push(std::thread::spawn(move || {
                let agent = engine
                    .join(
                        "agent",
                        &format!("writer-{index}"),
                        None,
                        None,
                        "local",
                        None,
                    )
                    .unwrap();
                engine.commit(
                    &agent.participant_id,
                    1,
                    &format!("edit {index}"),
                    &format!("# Architecture\nedit {index}\n"),
                    "workspace",
                    None,
                )
            }));
        }
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        let wins = results.iter().filter(|result| result.is_ok()).count();
        assert_eq!(wins, 1);
        assert_eq!(engine.status().unwrap().version, 2);
        let changes: Vec<_> = engine
            .history(50, None)
            .unwrap()
            .into_iter()
            .filter(|event| event.event_type == ARTIFACT_CHANGED)
            .collect();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].version, Some(2));
    }

    #[test]
    fn an_expired_session_records_expiry_without_the_session_id() {
        let stamp = std::sync::Arc::new(std::sync::Mutex::new(chrono::Utc::now()));
        let clock_stamp = std::sync::Arc::clone(&stamp);
        let clock = Clock(std::sync::Arc::new(move || *clock_stamp.lock().unwrap()));
        let dir = std::env::temp_dir().join(format!("ce-engine-clock-{}", token_hex(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workspace.md");
        std::fs::write(&path, "# Architecture\n").unwrap();
        let engine = WorkspaceEngine::open(&path, None, Limits::default(), Some(clock)).unwrap();
        let session = engine
            .join("agent", "short", None, None, "local", Some(1))
            .unwrap();
        *stamp.lock().unwrap() += chrono::Duration::minutes(2);
        let err = engine.authenticate(&session.session_id).unwrap_err();
        assert_eq!(err.code, "unauthorized");
        let expired = engine
            .history(20, None)
            .unwrap()
            .into_iter()
            .find(|event| event.event_type == SESSION_EXPIRED)
            .unwrap();
        let blob = serde_json::to_string(&expired).unwrap();
        assert!(!blob.contains(&session.session_id));
        assert_eq!(
            expired.actor_id.as_deref(),
            Some(session.participant_id.as_str())
        );
        let err = engine.authenticate(&session.session_id).unwrap_err();
        assert_eq!(err.code, "unauthorized");
        let expiries = engine
            .history(20, None)
            .unwrap()
            .iter()
            .filter(|event| event.event_type == SESSION_EXPIRED)
            .count();
        assert_eq!(expiries, 1);
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn subscribers_on_one_engine_do_not_see_another_workspace() {
        let (_dir_a, engine_a) = engine("# Architecture\nA\n");
        let (_dir_b, engine_b) = engine("# Architecture\nB\n");
        let sub = engine_a.subscribe();
        let agent = engine_b
            .join("agent", "b", None, None, "local", None)
            .unwrap();
        engine_b
            .checkout(&agent.participant_id, "section:Architecture", None, None)
            .unwrap();
        assert!(sub
            .recv_timeout(std::time::Duration::from_millis(80))
            .is_err());
    }
}
