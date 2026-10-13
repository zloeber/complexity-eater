//! Durable handoffs and execution tasks.
//!
//! A handoff is the collaboration record. A task is the execution record that
//! MCP clients poll. Events are the recovery log. None of these statuses is a
//! substitute for the others, and none of them is the human approval decision.

use ce_protocol::{
    has_capability, invalid_request, limit_exceeded, permission_denied, unauthorized,
    CommonroomError,
};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

use super::{json_text, next_id, parse_caps, parse_time, EngineState, WorkspaceEngine};
use crate::events::{
    HANDOFF_ACCEPTED as EVENT_ACCEPTED, HANDOFF_APPROVED as EVENT_APPROVED,
    HANDOFF_CANCELLED as EVENT_CANCELLED, HANDOFF_CREATED, HANDOFF_REJECTED as EVENT_REJECTED,
    HANDOFF_RESULT_READY, TASK_CREATED, TASK_UPDATED,
};
use crate::Event;

const TASK_SCHEMA_VERSION: i64 = 1;
const DEFAULT_TTL_MS: i64 = 86_400_000;
const POLL_INTERVAL_MS: i64 = 5_000;
const MAX_ACTIVE_TASKS: i64 = 32;
const MAX_SUMMARY_CHARS: usize = 2_000;
const MAX_CONSTRAINTS_CHARS: usize = 8_000;
const MAX_RESULT_CHARS: usize = 262_144;
const MAX_ARTIFACT_REFS: usize = 16;
const MAX_REF_CHARS: usize = 512;
const MAX_KEY_CHARS: usize = 128;
const MAX_INPUT_BYTES: usize = 65_536;
const OPERATION: &str = "handoff.execute";

const OPEN: &str = "open";
const ACCEPTED: &str = "accepted";
const READY: &str = "result_ready";
const APPROVED: &str = "approved";
const REJECTED: &str = "rejected";
const CANCELLED: &str = "cancelled";

const TASK_WORKING: &str = "working";
const TASK_INPUT: &str = "input_required";
const TASK_COMPLETED: &str = "completed";
const TASK_CANCELLED: &str = "cancelled";
const TASK_FAILED: &str = "failed";

const HANDOFF_SELECT: &str = "SELECT handoff_id, workspace_id, creator_id, assignee_id, summary, constraints, artifact_refs, status, result_text, correlation_id, task_id, created_at, updated_at FROM handoffs";
const TASK_SELECT: &str = "SELECT task_id, workspace_id, owner_id, assignee_id, handoff_id, operation, status, status_message, created_at, updated_at, ttl_ms, poll_interval_ms, result_json, error_json, cancel_requested, correlation_id, schema_version, input_requests_json FROM tasks";

#[derive(Clone, Debug, Serialize)]
pub struct Handoff {
    pub handoff_id: String,
    pub workspace_id: String,
    pub creator_id: String,
    pub assignee_id: Option<String>,
    pub summary: String,
    pub constraints: String,
    pub artifact_refs: Vec<String>,
    pub status: String,
    pub result_text: Option<String>,
    pub correlation_id: String,
    pub task_id: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TaskView {
    pub task_id: String,
    pub workspace_id: String,
    pub owner_id: String,
    pub assignee_id: Option<String>,
    pub handoff_id: Option<String>,
    pub operation: String,
    pub status: String,
    pub status_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub ttl_ms: i64,
    pub poll_interval_ms: i64,
    pub result: Option<Value>,
    pub error: Option<Value>,
    pub cancel_requested: bool,
    pub correlation_id: String,
    pub schema_version: i64,
    pub input_requests: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HandoffCreated {
    pub handoff: Handoff,
    pub task: TaskView,
    pub replayed: bool,
}

pub struct NewHandoff<'a> {
    pub summary: &'a str,
    pub constraints: &'a str,
    pub artifact_refs: &'a [String],
    pub idempotency_key: Option<&'a str>,
    pub correlation_id: Option<&'a str>,
}

impl WorkspaceEngine {
    pub fn create_handoff(
        &self,
        actor_id: &str,
        draft: NewHandoff<'_>,
    ) -> Result<HandoffCreated, CommonroomError> {
        self.lock().create_handoff(actor_id, draft)
    }

    pub fn list_handoffs(&self) -> Result<Vec<Handoff>, CommonroomError> {
        self.lock().list_handoffs()
    }

    pub fn get_handoff(&self, handoff_id: &str) -> Result<Handoff, CommonroomError> {
        self.lock().get_handoff(handoff_id)
    }

    pub fn accept_handoff(
        &self,
        actor_id: &str,
        handoff_id: &str,
    ) -> Result<Handoff, CommonroomError> {
        self.lock().accept_handoff(actor_id, handoff_id)
    }

    pub fn complete_handoff(
        &self,
        actor_id: &str,
        handoff_id: &str,
        result_text: &str,
    ) -> Result<Handoff, CommonroomError> {
        self.lock()
            .complete_handoff(actor_id, handoff_id, result_text)
    }

    pub fn request_handoff_input(
        &self,
        actor_id: &str,
        handoff_id: &str,
        prompt: &str,
    ) -> Result<TaskView, CommonroomError> {
        self.lock()
            .request_handoff_input(actor_id, handoff_id, prompt)
    }

    pub fn apply_task_input(
        &self,
        actor_id: &str,
        task_id: &str,
        responses: &Value,
    ) -> Result<TaskView, CommonroomError> {
        self.lock().apply_task_input(actor_id, task_id, responses)
    }

    pub fn approve_handoff(
        &self,
        actor_id: &str,
        handoff_id: &str,
    ) -> Result<Handoff, CommonroomError> {
        self.lock().approve_handoff(actor_id, handoff_id, true)
    }

    pub fn reject_handoff(
        &self,
        actor_id: &str,
        handoff_id: &str,
    ) -> Result<Handoff, CommonroomError> {
        self.lock().approve_handoff(actor_id, handoff_id, false)
    }

    pub fn get_task(&self, actor_id: &str, task_id: &str) -> Result<TaskView, CommonroomError> {
        self.lock().get_task(actor_id, task_id)
    }

    pub fn cancel_task(&self, actor_id: &str, task_id: &str) -> Result<TaskView, CommonroomError> {
        self.lock().cancel_task(actor_id, task_id)
    }

    pub fn handoff_events(&self, handoff_id: &str) -> Result<Vec<Event>, CommonroomError> {
        let state = self.lock();
        let _ = state.get_handoff(handoff_id)?;
        let events = state.events_since(0, None, state.limits.max_events as i64)?;
        Ok(events
            .into_iter()
            .filter(|event| {
                event.payload.get("handoff_id").and_then(Value::as_str) == Some(handoff_id)
            })
            .collect())
    }
}

impl EngineState {
    pub(super) fn expire_tasks(&self) -> Result<(), CommonroomError> {
        let now = self.now();
        let tasks = self.task_rows()?;
        let due: Vec<StoredTask> = tasks
            .into_iter()
            .filter(|task| task_is_due(task, now))
            .collect();
        if due.is_empty() {
            return Ok(());
        }
        self.transact(|state| {
            let mut events = Vec::new();
            for task in &due {
                events.extend(state.expire_one(task)?);
            }
            Ok(((), events))
        })
    }

    fn create_handoff(
        &self,
        actor_id: &str,
        draft: NewHandoff<'_>,
    ) -> Result<HandoffCreated, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        require_cap(&caps, "propose")?;
        let summary = bounded_text("summary", draft.summary, MAX_SUMMARY_CHARS, false)?;
        let constraints = bounded_text(
            "constraints",
            draft.constraints,
            MAX_CONSTRAINTS_CHARS,
            true,
        )?;
        let refs = bounded_refs(draft.artifact_refs)?;
        let idempotency_key = optional_key("idempotency_key", draft.idempotency_key)?;
        let correlation_id = match optional_key("correlation_id", draft.correlation_id)? {
            Some(value) => value,
            None => next_id("corr"),
        };
        self.expire_tasks()?;
        if let Some(key) = &idempotency_key {
            if let Some(existing) = self.handoff_by_key(&actor_id, key)? {
                let task = match self.task_by_id(&existing.task_id)? {
                    Some(task) => task,
                    None => retained_task(&existing),
                };
                return Ok(HandoffCreated {
                    handoff: existing,
                    task,
                    replayed: true,
                });
            }
        }
        let active = self.active_task_count()?;
        if active >= MAX_ACTIVE_TASKS {
            return Err(limit_exceeded("too many active tasks").detail("limit", MAX_ACTIVE_TASKS));
        }
        let handoff_id = next_id("hnd");
        let task_id = next_id("task");
        let workspace_id = self.workspace_id()?;
        let now = self.now_text();
        let refs_json = json_text(&refs)?;
        let created = self.transact(|state| {
            state.conn()?.execute(
                "INSERT INTO handoffs(handoff_id, workspace_id, creator_id, assignee_id, summary, constraints, artifact_refs, status, result_text, correlation_id, idempotency_key, task_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?11)",
                params![handoff_id, workspace_id, actor_id, summary, constraints, refs_json, OPEN, correlation_id, idempotency_key, task_id, now],
            ).map_err(super::db_err)?;
            state.conn()?.execute(
                "INSERT INTO tasks(task_id, workspace_id, owner_id, assignee_id, handoff_id, operation, status, status_message, created_at, updated_at, ttl_ms, poll_interval_ms, result_json, error_json, cancel_requested, correlation_id, idempotency_key, schema_version, input_requests_json)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?10, NULL, NULL, 0, ?11, ?12, ?13, NULL)",
                params![task_id, workspace_id, actor_id, handoff_id, OPERATION, TASK_WORKING, "Waiting for another participant.", now, DEFAULT_TTL_MS, POLL_INTERVAL_MS, correlation_id, idempotency_key, TASK_SCHEMA_VERSION],
            ).map_err(super::db_err)?;
            let mut events = Vec::new();
            events.push(state.insert_event(HANDOFF_CREATED, json!({
                "handoff_id": handoff_id,
                "task_id": task_id,
                "status": OPEN,
                "correlation_id": correlation_id,
                "participant_id": actor_id,
                "summary": summary,
            }))?);
            events.push(state.insert_event(TASK_CREATED, json!({
                "handoff_id": handoff_id,
                "task_id": task_id,
                "status": TASK_WORKING,
                "correlation_id": correlation_id,
                "participant_id": actor_id,
            }))?);
            let handoff = state.must_handoff(&handoff_id)?;
            let task = state.must_task(&task_id)?;
            Ok((
                HandoffCreated {
                    handoff,
                    task,
                    replayed: false,
                },
                events,
            ))
        })?;
        log_task(&created.task, "task created");
        Ok(created)
    }

    fn list_handoffs(&self) -> Result<Vec<Handoff>, CommonroomError> {
        self.expire_tasks()?;
        self.map_rows(
            &format!("{HANDOFF_SELECT} ORDER BY created_at ASC"),
            [],
            handoff_from_row,
        )
    }

    fn get_handoff(&self, handoff_id: &str) -> Result<Handoff, CommonroomError> {
        self.expire_tasks()?;
        self.handoff_by_id(handoff_id)?
            .ok_or_else(|| invalid_request("unknown handoff"))
    }

    fn accept_handoff(&self, actor_id: &str, handoff_id: &str) -> Result<Handoff, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        require_cap(&caps, "commit")?;
        self.expire_tasks()?;
        let updated = self.transact(|state| {
            let handoff = state.must_handoff(handoff_id)?;
            if handoff.status == ACCEPTED && handoff.assignee_id.as_deref() == Some(actor_id.as_str())
            {
                return Ok((handoff, Vec::new()));
            }
            if handoff.status != OPEN {
                return Err(invalid_request("handoff cannot be accepted"));
            }
            let now = state.now_text();
            let changed = state.conn()?.execute(
                "UPDATE handoffs SET status = ?1, assignee_id = ?2, updated_at = ?3 WHERE handoff_id = ?4 AND status = ?5",
                params![ACCEPTED, actor_id, now, handoff_id, OPEN],
            ).map_err(super::db_err)?;
            if changed != 1 {
                return Err(invalid_request("handoff cannot be accepted"));
            }
            state.conn()?.execute(
                "UPDATE tasks SET assignee_id = ?1, status_message = ?2, updated_at = ?3 WHERE task_id = ?4 AND status = ?5",
                params![actor_id, "Accepted. Waiting for a result.", now, handoff.task_id, TASK_WORKING],
            ).map_err(super::db_err)?;
            let mut events = Vec::new();
            events.push(state.insert_event(EVENT_ACCEPTED, json!({
                "handoff_id": handoff_id,
                "task_id": handoff.task_id,
                "status": ACCEPTED,
                "correlation_id": handoff.correlation_id,
                "participant_id": actor_id,
            }))?);
            events.push(state.insert_event(TASK_UPDATED, json!({
                "handoff_id": handoff_id,
                "task_id": handoff.task_id,
                "status": TASK_WORKING,
                "correlation_id": handoff.correlation_id,
                "participant_id": actor_id,
            }))?);
            let handoff = state.must_handoff(handoff_id)?;
            let task = state.must_task(&handoff.task_id)?;
            log_task(&task, "handoff accepted");
            Ok((handoff, events))
        })?;
        Ok(updated)
    }

    fn complete_handoff(
        &self,
        actor_id: &str,
        handoff_id: &str,
        result_text: &str,
    ) -> Result<Handoff, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        require_cap(&caps, "commit")?;
        let result_text = bounded_text("result", result_text, MAX_RESULT_CHARS, false)?;
        self.expire_tasks()?;
        self.transact(|state| {
            let handoff = state.must_handoff(handoff_id)?;
            if handoff.assignee_id.as_deref() != Some(actor_id.as_str()) {
                return Err(permission_denied(
                    "only the participant who accepted the handoff can complete it",
                ));
            }
            if handoff.status == READY {
                return Ok((Ok(handoff), Vec::new()));
            }
            if handoff.status != ACCEPTED {
                return Err(invalid_request("handoff cannot be completed"));
            }
            let task = state.must_task(&handoff.task_id)?;
            if task.cancel_requested || task.status == TASK_CANCELLED {
                let events = if task.status == TASK_CANCELLED {
                    Vec::new()
                } else {
                    state.finish_cancelled(&handoff, &actor_id)?
                };
                return Ok((Err(invalid_request("task was cancelled")), events));
            }
            if task.status != TASK_WORKING {
                return Err(invalid_request("task is not ready for a result"));
            }
            let now = state.now_text();
            let snapshot = {
                let mut next = handoff.clone();
                next.status = READY.into();
                next.result_text = Some(result_text.clone());
                next.updated_at = now.clone();
                next
            };
            let result_json = json_text(&tool_result(&snapshot))?;
            let changed = state.conn()?.execute(
                "UPDATE tasks SET status = ?1, status_message = ?2, result_json = ?3, updated_at = ?4, input_requests_json = NULL
                 WHERE task_id = ?5 AND status = ?6 AND cancel_requested = 0",
                params![TASK_COMPLETED, "Completed.", result_json, now, task.task_id, TASK_WORKING],
            ).map_err(super::db_err)?;
            if changed != 1 {
                return Err(invalid_request("task was cancelled"));
            }
            state.conn()?.execute(
                "UPDATE handoffs SET status = ?1, result_text = ?2, updated_at = ?3 WHERE handoff_id = ?4 AND status = ?5",
                params![READY, result_text, now, handoff_id, ACCEPTED],
            ).map_err(super::db_err)?;
            let mut events = Vec::new();
            events.push(state.insert_event(HANDOFF_RESULT_READY, json!({
                "handoff_id": handoff_id,
                "task_id": task.task_id,
                "status": READY,
                "correlation_id": handoff.correlation_id,
                "participant_id": actor_id,
            }))?);
            events.push(state.insert_event(TASK_UPDATED, json!({
                "handoff_id": handoff_id,
                "task_id": task.task_id,
                "status": TASK_COMPLETED,
                "correlation_id": handoff.correlation_id,
                "participant_id": actor_id,
            }))?);
            let handoff = state.must_handoff(handoff_id)?;
            let task = state.must_task(&task.task_id)?;
            log_task(&task, "task completed");
            Ok((Ok(handoff), events))
        })?
    }

    fn request_handoff_input(
        &self,
        actor_id: &str,
        handoff_id: &str,
        prompt: &str,
    ) -> Result<TaskView, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        require_cap(&caps, "commit")?;
        let prompt = bounded_text("prompt", prompt, MAX_SUMMARY_CHARS, false)?;
        self.expire_tasks()?;
        self.transact(|state| {
            let handoff = state.must_handoff(handoff_id)?;
            if handoff.assignee_id.as_deref() != Some(actor_id.as_str()) {
                return Err(permission_denied(
                    "only the participant who accepted the handoff can request input",
                ));
            }
            let task = state.must_task(&handoff.task_id)?;
            if task.status == TASK_INPUT {
                return Ok((task, Vec::new()));
            }
            if task.status != TASK_WORKING {
                return Err(invalid_request("task is not accepting input requests"));
            }
            let requests = json!({
                "note": {
                    "method": "elicitation/create",
                    "params": {"message": prompt}
                }
            });
            let now = state.now_text();
            let changed = state.conn()?.execute(
                "UPDATE tasks SET status = ?1, status_message = ?2, input_requests_json = ?3, updated_at = ?4
                 WHERE task_id = ?5 AND status = ?6 AND cancel_requested = 0",
                params![TASK_INPUT, "Input required.", requests.to_string(), now, task.task_id, TASK_WORKING],
            ).map_err(super::db_err)?;
            if changed != 1 {
                return Err(invalid_request("task is not accepting input requests"));
            }
            let event = state.insert_event(TASK_UPDATED, json!({
                "handoff_id": handoff_id,
                "task_id": task.task_id,
                "status": TASK_INPUT,
                "correlation_id": task.correlation_id,
                "participant_id": actor_id,
            }))?;
            let task = state.must_task(&task.task_id)?;
            log_task(&task, "task input required");
            Ok((task, vec![event]))
        })
    }

    fn apply_task_input(
        &self,
        actor_id: &str,
        task_id: &str,
        responses: &Value,
    ) -> Result<TaskView, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        let encoded = responses.to_string();
        if encoded.len() > MAX_INPUT_BYTES {
            return Err(limit_exceeded("input response is too large"));
        }
        let Some(responses) = responses.as_object() else {
            return Err(invalid_request("inputResponses must be an object"));
        };
        self.expire_tasks()?;
        self.transact(|state| {
            let task = state.task_for_actor(&actor_id, &caps, task_id)?;
            if task.owner_id != actor_id && !has_capability(&caps, "admin") {
                return Err(unknown_task());
            }
            if task.status != TASK_INPUT {
                return Err(invalid_request("task is not waiting for input"));
            }
            let mut outstanding = task
                .input_requests
                .clone()
                .unwrap_or_else(|| json!({}))
                .as_object()
                .cloned()
                .unwrap_or_default();
            for key in responses.keys() {
                outstanding.remove(key);
            }
            let now = state.now_text();
            let (status, message, stored) = if outstanding.is_empty() {
                (TASK_WORKING, "Input received.".to_string(), None)
            } else {
                (
                    TASK_INPUT,
                    "Input required.".to_string(),
                    Some(Value::Object(outstanding).to_string()),
                )
            };
            state.conn()?.execute(
                "UPDATE tasks SET status = ?1, status_message = ?2, input_requests_json = ?3, updated_at = ?4
                 WHERE task_id = ?5 AND status = ?6",
                params![status, message, stored, now, task.task_id, TASK_INPUT],
            ).map_err(super::db_err)?;
            let event = state.insert_event(TASK_UPDATED, json!({
                "handoff_id": task.handoff_id,
                "task_id": task.task_id,
                "status": status,
                "correlation_id": task.correlation_id,
                "participant_id": actor_id,
            }))?;
            let task = state.must_task(&task.task_id)?;
            log_task(&task, "task input applied");
            Ok((task, vec![event]))
        })
    }

    fn approve_handoff(
        &self,
        actor_id: &str,
        handoff_id: &str,
        approve: bool,
    ) -> Result<Handoff, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        require_cap(&caps, "approve")?;
        self.expire_tasks()?;
        let status = if approve { APPROVED } else { REJECTED };
        let event_type = if approve {
            EVENT_APPROVED
        } else {
            EVENT_REJECTED
        };
        self.transact(|state| {
            let handoff = state.must_handoff(handoff_id)?;
            if handoff.status == status {
                return Ok((handoff, Vec::new()));
            }
            if handoff.status != READY {
                return Err(invalid_request("handoff result is not ready for review"));
            }
            let now = state.now_text();
            let changed = state.conn()?.execute(
                "UPDATE handoffs SET status = ?1, updated_at = ?2 WHERE handoff_id = ?3 AND status = ?4",
                params![status, now, handoff_id, READY],
            ).map_err(super::db_err)?;
            if changed != 1 {
                return Err(invalid_request("handoff result is not ready for review"));
            }
            let event = state.insert_event(event_type, json!({
                "handoff_id": handoff_id,
                "task_id": handoff.task_id,
                "status": status,
                "correlation_id": handoff.correlation_id,
                "participant_id": actor_id,
            }))?;
            Ok((state.must_handoff(handoff_id)?, vec![event]))
        })
    }

    fn get_task(&self, actor_id: &str, task_id: &str) -> Result<TaskView, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        self.expire_tasks()?;
        self.task_for_actor(&actor_id, &caps, task_id)
    }

    fn cancel_task(&self, actor_id: &str, task_id: &str) -> Result<TaskView, CommonroomError> {
        let (actor_id, caps) = self.actor(actor_id)?;
        self.expire_tasks()?;
        self.transact(|state| {
            let task = state.task_for_actor(&actor_id, &caps, task_id)?;
            if task_terminal(&task.status) {
                return Ok((task, Vec::new()));
            }
            let handoff = match &task.handoff_id {
                Some(id) => Some(state.must_handoff(id)?),
                None => None,
            };
            let now = state.now_text();
            state.conn()?.execute(
                "UPDATE tasks SET status = ?1, status_message = ?2, cancel_requested = 1, updated_at = ?3, input_requests_json = NULL
                 WHERE task_id = ?4 AND status NOT IN ('completed', 'cancelled', 'failed')",
                params![TASK_CANCELLED, "Cancelled.", now, task.task_id],
            ).map_err(super::db_err)?;
            let mut events = Vec::new();
            if let Some(handoff) = handoff {
                if !handoff_terminal(&handoff.status) {
                    state.conn()?.execute(
                        "UPDATE handoffs SET status = ?1, updated_at = ?2 WHERE handoff_id = ?3",
                        params![CANCELLED, now, handoff.handoff_id],
                    ).map_err(super::db_err)?;
                    events.push(state.insert_event(EVENT_CANCELLED, json!({
                        "handoff_id": handoff.handoff_id,
                        "task_id": task.task_id,
                        "status": CANCELLED,
                        "correlation_id": task.correlation_id,
                        "participant_id": actor_id,
                    }))?);
                }
            }
            events.push(state.insert_event(TASK_UPDATED, json!({
                "handoff_id": task.handoff_id,
                "task_id": task.task_id,
                "status": TASK_CANCELLED,
                "correlation_id": task.correlation_id,
                "participant_id": actor_id,
            }))?);
            let task = state.must_task(&task.task_id)?;
            log_task(&task, "task cancelled");
            Ok((task, events))
        })
    }

    fn finish_cancelled(
        &self,
        handoff: &Handoff,
        actor_id: &str,
    ) -> Result<Vec<Event>, CommonroomError> {
        if self.must_task(&handoff.task_id)?.status == TASK_CANCELLED {
            return Ok(Vec::new());
        }
        let now = self.now_text();
        self.conn()?.execute(
            "UPDATE tasks SET status = ?1, status_message = ?2, cancel_requested = 1, updated_at = ?3 WHERE task_id = ?4",
            params![TASK_CANCELLED, "Cancelled.", now, handoff.task_id],
        ).map_err(super::db_err)?;
        self.conn()?.execute(
            "UPDATE handoffs SET status = ?1, updated_at = ?2 WHERE handoff_id = ?3 AND status NOT IN ('approved', 'rejected', 'cancelled')",
            params![CANCELLED, now, handoff.handoff_id],
        ).map_err(super::db_err)?;
        Ok(vec![
            self.insert_event(
                EVENT_CANCELLED,
                json!({
                    "handoff_id": handoff.handoff_id,
                    "task_id": handoff.task_id,
                    "status": CANCELLED,
                    "correlation_id": handoff.correlation_id,
                    "participant_id": actor_id,
                }),
            )?,
            self.insert_event(
                TASK_UPDATED,
                json!({
                    "handoff_id": handoff.handoff_id,
                    "task_id": handoff.task_id,
                    "status": TASK_CANCELLED,
                    "correlation_id": handoff.correlation_id,
                    "participant_id": actor_id,
                }),
            )?,
        ])
    }

    fn expire_one(&self, stale: &StoredTask) -> Result<Vec<Event>, CommonroomError> {
        let Some(task) = self.task_by_id(&stale.task_id)? else {
            return Ok(Vec::new());
        };
        let now = self.now();
        if !task_is_due(&task, now) {
            return Ok(Vec::new());
        }
        if task_terminal(&task.status) {
            self.conn()?
                .execute(
                    "DELETE FROM tasks WHERE task_id = ?1",
                    params![task.task_id],
                )
                .map_err(super::db_err)?;
            return Ok(Vec::new());
        }
        let error = json!({"code": -32000, "message": "task expired"});
        let stamp = self.now_text();
        self.conn()?.execute(
            "UPDATE tasks SET status = ?1, status_message = ?2, error_json = ?3, updated_at = ?4, input_requests_json = NULL
             WHERE task_id = ?5 AND status NOT IN ('completed', 'cancelled', 'failed')",
            params![TASK_FAILED, "Expired.", error.to_string(), stamp, task.task_id],
        ).map_err(super::db_err)?;
        let mut events = vec![self.insert_event(
            TASK_UPDATED,
            json!({
                "handoff_id": task.handoff_id,
                "task_id": task.task_id,
                "status": TASK_FAILED,
                "correlation_id": task.correlation_id,
                "participant_id": task.owner_id,
            }),
        )?];
        if let Some(handoff_id) = &task.handoff_id {
            if let Some(handoff) = self.handoff_by_id(handoff_id)? {
                if !handoff_terminal(&handoff.status) {
                    self.conn()?.execute(
                        "UPDATE handoffs SET status = ?1, updated_at = ?2 WHERE handoff_id = ?3",
                        params![CANCELLED, stamp, handoff_id],
                    ).map_err(super::db_err)?;
                    events.push(self.insert_event(
                        EVENT_CANCELLED,
                        json!({
                            "handoff_id": handoff_id,
                            "task_id": task.task_id,
                            "status": CANCELLED,
                            "correlation_id": task.correlation_id,
                            "participant_id": task.owner_id,
                        }),
                    )?);
                }
            }
        }
        if let Ok(task) = self.must_task(&stale.task_id) {
            log_task(&task, "task expired");
        }
        Ok(events)
    }

    fn transact<T>(
        &self,
        body: impl FnOnce(&Self) -> Result<(T, Vec<Event>), CommonroomError>,
    ) -> Result<T, CommonroomError> {
        self.conn()?
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(super::db_err)?;
        match body(self) {
            Ok((value, events)) => {
                self.conn()?
                    .execute_batch("COMMIT")
                    .map_err(super::db_err)?;
                for event in events {
                    self.bus.publish(event);
                }
                Ok(value)
            }
            Err(err) => {
                let _ = self.conn()?.execute_batch("ROLLBACK");
                Err(err)
            }
        }
    }

    fn actor(&self, participant_id: &str) -> Result<(String, Vec<String>), CommonroomError> {
        let row = self
            .conn()?
            .query_row(
                "SELECT participant_id, capabilities FROM participants WHERE participant_id = ?1 AND left_at IS NULL",
                params![participant_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(super::db_err)?;
        let Some((id, caps)) = row else {
            return Err(unauthorized("unknown participant"));
        };
        Ok((id, parse_caps(&caps)))
    }

    fn active_task_count(&self) -> Result<i64, CommonroomError> {
        self.conn()?
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status IN ('working', 'input_required')",
                [],
                |row| row.get(0),
            )
            .map_err(super::db_err)
    }

    fn task_rows(&self) -> Result<Vec<StoredTask>, CommonroomError> {
        self.map_rows(TASK_SELECT, [], task_from_row)
    }

    fn handoff_by_id(&self, handoff_id: &str) -> Result<Option<Handoff>, CommonroomError> {
        self.conn()?
            .query_row(
                &format!("{HANDOFF_SELECT} WHERE handoff_id = ?1"),
                params![handoff_id],
                handoff_from_row,
            )
            .optional()
            .map_err(super::db_err)
    }

    fn handoff_by_key(
        &self,
        creator_id: &str,
        key: &str,
    ) -> Result<Option<Handoff>, CommonroomError> {
        self.conn()?
            .query_row(
                &format!("{HANDOFF_SELECT} WHERE creator_id = ?1 AND idempotency_key = ?2"),
                params![creator_id, key],
                handoff_from_row,
            )
            .optional()
            .map_err(super::db_err)
    }

    fn task_by_id(&self, task_id: &str) -> Result<Option<TaskView>, CommonroomError> {
        self.conn()?
            .query_row(
                &format!("{TASK_SELECT} WHERE task_id = ?1"),
                params![task_id],
                task_from_row,
            )
            .optional()
            .map_err(super::db_err)
    }

    fn must_handoff(&self, handoff_id: &str) -> Result<Handoff, CommonroomError> {
        self.handoff_by_id(handoff_id)?
            .ok_or_else(|| invalid_request("unknown handoff"))
    }

    fn must_task(&self, task_id: &str) -> Result<TaskView, CommonroomError> {
        self.task_by_id(task_id)?.ok_or_else(unknown_task)
    }

    fn task_for_actor(
        &self,
        actor_id: &str,
        caps: &[String],
        task_id: &str,
    ) -> Result<TaskView, CommonroomError> {
        let Some(task) = self.task_by_id(task_id)? else {
            return Err(unknown_task());
        };
        let party = task.owner_id == actor_id || task.assignee_id.as_deref() == Some(actor_id);
        if party || has_capability(caps, "admin") {
            Ok(task)
        } else {
            Err(unknown_task())
        }
    }
}

type StoredTask = TaskView;

fn task_is_due(task: &TaskView, now: DateTime<Utc>) -> bool {
    let stamp = if task_terminal(&task.status) {
        &task.updated_at
    } else {
        &task.created_at
    };
    let Ok(start) = parse_time(stamp) else {
        return false;
    };
    now > start + Duration::milliseconds(task.ttl_ms)
}

fn task_terminal(status: &str) -> bool {
    matches!(status, TASK_COMPLETED | TASK_CANCELLED | TASK_FAILED)
}

fn handoff_terminal(status: &str) -> bool {
    matches!(status, APPROVED | REJECTED | CANCELLED)
}

fn unknown_task() -> CommonroomError {
    invalid_request("unknown task")
}

fn require_cap(caps: &[String], capability: &str) -> Result<(), CommonroomError> {
    if has_capability(caps, capability) {
        Ok(())
    } else {
        Err(
            permission_denied(format!("session is missing the {capability} capability"))
                .detail("required", capability),
        )
    }
}

fn bounded_text(
    field: &str,
    value: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<String, CommonroomError> {
    let trimmed = value.trim();
    if trimmed.is_empty() && !allow_empty {
        return Err(invalid_request(format!("{field} is required")));
    }
    if trimmed.chars().count() > max_chars {
        return Err(limit_exceeded(format!("{field} is too large")));
    }
    Ok(trimmed.to_string())
}

fn bounded_refs(refs: &[String]) -> Result<Vec<String>, CommonroomError> {
    if refs.len() > MAX_ARTIFACT_REFS {
        return Err(limit_exceeded("too many artifact references"));
    }
    let mut cleaned = Vec::new();
    for item in refs {
        let trimmed = item.trim();
        if trimmed.is_empty() || trimmed.chars().count() > MAX_REF_CHARS {
            return Err(invalid_request("artifact reference is empty or too long"));
        }
        cleaned.push(trimmed.to_string());
    }
    Ok(cleaned)
}

fn optional_key(field: &str, value: Option<&str>) -> Result<Option<String>, CommonroomError> {
    let Some(value) = value.map(str::trim).filter(|item| !item.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > MAX_KEY_CHARS
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.'))
    {
        return Err(invalid_request(format!(
            "{field} is not a usable identifier"
        )));
    }
    Ok(Some(value.to_string()))
}

fn retained_task(handoff: &Handoff) -> TaskView {
    let completed = matches!(handoff.status.as_str(), READY | APPROVED | REJECTED);
    let status = if completed {
        TASK_COMPLETED
    } else if handoff.status == CANCELLED {
        TASK_CANCELLED
    } else {
        TASK_FAILED
    };
    TaskView {
        task_id: handoff.task_id.clone(),
        workspace_id: handoff.workspace_id.clone(),
        owner_id: handoff.creator_id.clone(),
        assignee_id: handoff.assignee_id.clone(),
        handoff_id: Some(handoff.handoff_id.clone()),
        operation: OPERATION.to_string(),
        status: status.to_string(),
        status_message: Some("Retained on the handoff.".into()),
        created_at: handoff.created_at.clone(),
        updated_at: handoff.updated_at.clone(),
        ttl_ms: DEFAULT_TTL_MS,
        poll_interval_ms: POLL_INTERVAL_MS,
        result: completed.then(|| tool_result(handoff)),
        error: None,
        cancel_requested: status == TASK_CANCELLED,
        correlation_id: handoff.correlation_id.clone(),
        schema_version: TASK_SCHEMA_VERSION,
        input_requests: None,
    }
}

fn tool_result(handoff: &Handoff) -> Value {
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": serde_json::to_string(handoff).unwrap_or_default()}],
        "isError": false
    })
}

fn log_task(task: &TaskView, message: &str) {
    tracing::info!(
        task_id = %task.task_id,
        handoff_id = task.handoff_id.as_deref().unwrap_or(""),
        status = %task.status,
        correlation_id = %task.correlation_id,
        operation = %task.operation,
        "{message}"
    );
}

fn handoff_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Handoff> {
    let refs: String = row.get(6)?;
    Ok(Handoff {
        handoff_id: row.get(0)?,
        workspace_id: row.get(1)?,
        creator_id: row.get(2)?,
        assignee_id: row.get(3)?,
        summary: row.get(4)?,
        constraints: row.get(5)?,
        artifact_refs: serde_json::from_str(&refs).unwrap_or_default(),
        status: row.get(7)?,
        result_text: row.get(8)?,
        correlation_id: row.get(9)?,
        task_id: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskView> {
    let result: Option<String> = row.get(12)?;
    let error: Option<String> = row.get(13)?;
    let input: Option<String> = row.get(17)?;
    let cancel: i64 = row.get(14)?;
    Ok(TaskView {
        task_id: row.get(0)?,
        workspace_id: row.get(1)?,
        owner_id: row.get(2)?,
        assignee_id: row.get(3)?,
        handoff_id: row.get(4)?,
        operation: row.get(5)?,
        status: row.get(6)?,
        status_message: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        ttl_ms: row.get(10)?,
        poll_interval_ms: row.get(11)?,
        result: result.and_then(|text| serde_json::from_str(&text).ok()),
        error: error.and_then(|text| serde_json::from_str(&text).ok()),
        cancel_requested: cancel != 0,
        correlation_id: row.get(15)?,
        schema_version: row.get(16)?,
        input_requests: input.and_then(|text| serde_json::from_str(&text).ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{HANDOFF_CREATED, TASK_CREATED, TASK_UPDATED};
    use ce_protocol::Limits;
    use chrono::TimeZone;

    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn engine_at(content: &str, at: Option<DateTime<Utc>>) -> (TempDir, WorkspaceEngine) {
        let dir = std::env::temp_dir().join(format!("ce-handoff-{}", next_id("tmp")));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workspace.md");
        std::fs::write(&path, content).unwrap();
        let clock = at.map(super::super::Clock::fixed);
        let engine = WorkspaceEngine::open(&path, None, Limits::default(), clock).unwrap();
        (TempDir(dir), engine)
    }

    fn agent(engine: &WorkspaceEngine, name: &str, caps: &[&str]) -> crate::Session {
        engine
            .join(
                "agent",
                name,
                Some(caps.iter().map(|item| (*item).to_string()).collect()),
                None,
                "local",
                None,
            )
            .unwrap()
    }

    fn draft<'a>(summary: &'a str, key: Option<&'a str>) -> NewHandoff<'a> {
        NewHandoff {
            summary,
            constraints: "stay inside the room",
            artifact_refs: &[],
            idempotency_key: key,
            correlation_id: Some("corr-room"),
        }
    }

    #[test]
    fn handoff_survives_restart_and_replays_one_result() {
        let (_dir, engine) = engine_at("# Plan\n", None);
        let maker = agent(&engine, "maker", &["read", "propose", "commit"]);
        let worker = agent(&engine, "worker", &["read", "propose", "commit"]);
        let stranger = agent(&engine, "stranger", &["read"]);
        let created = engine
            .create_handoff(&maker.participant_id, draft("write the note", Some("once")))
            .unwrap();
        let again = engine
            .create_handoff(&maker.participant_id, draft("write the note", Some("once")))
            .unwrap();
        assert!(again.replayed);
        assert_eq!(again.task.task_id, created.task.task_id);
        assert_eq!(engine.list_handoffs().unwrap().len(), 1);
        let err = engine
            .get_task(&stranger.participant_id, &created.task.task_id)
            .unwrap_err();
        assert_eq!(err.message, "unknown task");
        engine
            .accept_handoff(&worker.participant_id, &created.handoff.handoff_id)
            .unwrap();
        let db = engine.db_path();
        let path = engine.workspace_path();
        let task_id = created.task.task_id.clone();
        let handoff_id = created.handoff.handoff_id.clone();
        let worker_id = worker.participant_id.clone();
        engine.close();
        let engine = WorkspaceEngine::open(&path, Some(db), Limits::default(), None).unwrap();
        let task = engine.get_task(&maker.participant_id, &task_id).unwrap();
        assert_eq!(task.status, "working");
        engine
            .complete_handoff(&worker_id, &handoff_id, "shipped the note")
            .unwrap();
        let second = engine.complete_handoff(&worker_id, &handoff_id, "shipped the note");
        assert!(second.is_ok());
        let updates = engine
            .handoff_events(&handoff_id)
            .unwrap()
            .into_iter()
            .filter(|event| {
                event.event_type == TASK_UPDATED && event.payload["status"] == "completed"
            })
            .count();
        assert_eq!(updates, 1);
        let task = engine.get_task(&maker.participant_id, &task_id).unwrap();
        assert_eq!(task.status, "completed");
        let text = task.result.unwrap().to_string();
        assert!(text.contains("shipped the note"));
        assert!(engine
            .handoff_events(&handoff_id)
            .unwrap()
            .iter()
            .any(|event| event.event_type == HANDOFF_CREATED));
        assert!(engine
            .history(20, None)
            .unwrap()
            .iter()
            .any(|event| event.event_type == TASK_CREATED));
        let blob = format!("{:?}", engine.handoff_events(&handoff_id).unwrap());
        assert!(!blob.contains("shipped the note"));
    }

    #[test]
    fn cancel_wins_over_a_later_result_and_input_round_trips() {
        let (_dir, engine) = engine_at("# Plan\n", None);
        let maker = agent(&engine, "maker", &["propose", "commit", "approve"]);
        let worker = agent(&engine, "worker", &["commit"]);
        let created = engine
            .create_handoff(&maker.participant_id, draft("need a name", None))
            .unwrap();
        engine
            .accept_handoff(&worker.participant_id, &created.handoff.handoff_id)
            .unwrap();
        let waiting = engine
            .request_handoff_input(
                &worker.participant_id,
                &created.handoff.handoff_id,
                "What name?",
            )
            .unwrap();
        assert_eq!(waiting.status, "input_required");
        let resumed = engine
            .apply_task_input(
                &maker.participant_id,
                &created.task.task_id,
                &json!({"note": {"text": "Ada"}}),
            )
            .unwrap();
        assert_eq!(resumed.status, "working");
        assert!(resumed.input_requests.is_none());
        engine
            .cancel_task(&maker.participant_id, &created.task.task_id)
            .unwrap();
        let err = engine
            .complete_handoff(
                &worker.participant_id,
                &created.handoff.handoff_id,
                "too late",
            )
            .unwrap_err();
        assert_eq!(err.code, "invalid_request");
        let task = engine
            .get_task(&maker.participant_id, &created.task.task_id)
            .unwrap();
        assert_eq!(task.status, "cancelled");
        assert!(task.result.is_none());
    }

    #[test]
    fn expired_work_fails_and_old_results_leave_the_task_table() {
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let (_dir, engine) = engine_at("# Plan\n", Some(start));
        let maker = agent(&engine, "maker", &["propose", "commit", "approve"]);
        let worker = agent(&engine, "worker", &["commit"]);
        let created = engine
            .create_handoff(&maker.participant_id, draft("short job", None))
            .unwrap();
        engine
            .accept_handoff(&worker.participant_id, &created.handoff.handoff_id)
            .unwrap();
        engine
            .complete_handoff(&worker.participant_id, &created.handoff.handoff_id, "done")
            .unwrap();
        engine
            .approve_handoff(&maker.participant_id, &created.handoff.handoff_id)
            .unwrap();
        let path = engine.workspace_path();
        let db = engine.db_path();
        let task_id = created.task.task_id.clone();
        let handoff_id = created.handoff.handoff_id.clone();
        let maker_id = maker.participant_id.clone();
        engine.close();
        let later = start + Duration::milliseconds(DEFAULT_TTL_MS + 1);
        let engine = WorkspaceEngine::open(
            &path,
            Some(db),
            Limits::default(),
            Some(super::super::Clock::fixed(later)),
        )
        .unwrap();
        let err = engine.get_task(&maker_id, &task_id).unwrap_err();
        assert_eq!(err.message, "unknown task");
        let handoff = engine.get_handoff(&handoff_id).unwrap();
        assert_eq!(handoff.status, "approved");
        assert_eq!(handoff.result_text.as_deref(), Some("done"));
    }

    #[test]
    fn an_active_task_stays_until_its_ttl_and_then_fails() {
        let start = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let (_dir, engine) = engine_at("# Plan\n", Some(start));
        let maker = agent(&engine, "maker", &["propose"]);
        let created = engine
            .create_handoff(&maker.participant_id, draft("still working", None))
            .unwrap();
        let path = engine.workspace_path();
        let db = engine.db_path();
        let task_id = created.task.task_id.clone();
        let maker_id = maker.participant_id.clone();
        engine.close();
        let soon = start + Duration::hours(1);
        let engine = WorkspaceEngine::open(
            &path,
            Some(db.clone()),
            Limits::default(),
            Some(super::super::Clock::fixed(soon)),
        )
        .unwrap();
        assert_eq!(
            engine.get_task(&maker_id, &task_id).unwrap().status,
            "working"
        );
        engine.close();
        let later = start + Duration::milliseconds(DEFAULT_TTL_MS + 1);
        let engine = WorkspaceEngine::open(
            &path,
            Some(db),
            Limits::default(),
            Some(super::super::Clock::fixed(later)),
        )
        .unwrap();
        let task = engine.get_task(&maker_id, &task_id).unwrap();
        assert_eq!(task.status, "failed");
        assert_eq!(task.error.unwrap()["message"], "task expired");
    }
}
