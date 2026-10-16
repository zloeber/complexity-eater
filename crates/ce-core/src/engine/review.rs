//! Anchored review questions, answers, and comments.
//!
//! The room is the review session. The markdown file stays the source artifact.
//! Questions, responses, and comments live in the sidecar and never replace
//! that file. A document change is still a proposal until someone with
//! `approve` accepts it and someone with `commit` lands it.

use std::collections::HashMap;

use ce_protocol::{
    has_capability, invalid_region, invalid_request, limit_exceeded, permission_denied,
    question_not_found, CommonroomError,
};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

use super::{json_text, next_id, Db, EngineState, OutlineItem, Proposal, Session, WorkspaceEngine};
use crate::events::{
    COMMENT_CREATED, QUESTION_ANSWERED, QUESTION_CREATED, QUESTION_STATUS, QUESTION_UPDATED,
    REVIEW_STATUS,
};
use crate::markdown::{self, LocateError};

const DOCUMENT_CAP: usize = 262_144;
const MAX_PROMPT: usize = 4_000;
const MAX_QUOTE: usize = 500;
const MAX_AFFIX: usize = 80;
const MAX_OPTION: usize = 200;
const MAX_OPTIONS: usize = 8;
const MAX_RATIONALE: usize = 4_000;
const MAX_COMMENT: usize = 4_000;
const MAX_TEXT: usize = 8_000;
const MAX_KEY: usize = 128;
const KINDS: &[&str] = &["text", "single", "multi", "yes_no", "approve_reject"];
const OPEN: &str = "open";
const ANSWERED: &str = "answered";
const CLARIFY: &str = "needs_clarification";
const RESOLVED: &str = "resolved";
const REJECTED: &str = "rejected";

const CREATE_FIELDS: &[&str] = &[
    "parent_id",
    "kind",
    "prompt",
    "options",
    "required",
    "region",
    "quote",
    "prefix",
    "suffix",
    "idempotency_key",
];
const UPDATE_FIELDS: &[&str] = &["question_id", "prompt", "options", "required"];
const ANSWER_FIELDS: &[&str] = &["question_id", "value", "rationale"];
const COMMENT_FIELDS: &[&str] = &["question_id", "region", "quote", "body"];
const STATUS_FIELDS: &[&str] = &["question_id", "status"];
const REVIEW_FIELDS: &[&str] = &["status"];

#[derive(Clone, Debug, Serialize)]
pub struct ReviewCounts {
    pub questions: i64,
    pub open: i64,
    pub answered: i64,
    pub needs_clarification: i64,
    pub resolved: i64,
    pub rejected: i64,
    pub unanchored: i64,
    pub comments: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceContext {
    pub version: i64,
    pub base_version: i64,
    pub region: String,
    pub quote: String,
    pub resolution: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_region: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passage: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResponseView {
    pub response_id: String,
    pub question_id: String,
    pub participant_id: String,
    pub participant_kind: String,
    pub value: Value,
    pub rationale: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CommentView {
    pub comment_id: String,
    pub question_id: Option<String>,
    pub region: String,
    pub quote: String,
    pub body: String,
    pub participant_id: String,
    pub participant_kind: String,
    pub created_at: String,
    pub resolution: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct QuestionView {
    pub question_id: String,
    pub parent_id: Option<String>,
    pub document_id: String,
    pub kind: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub required: bool,
    pub status: String,
    pub anchor: SourceContext,
    pub created_by: String,
    pub created_by_kind: String,
    pub created_at: String,
    pub updated_at: String,
    pub responses: Vec<ResponseView>,
    pub comments: Vec<CommentView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnswerRecord {
    pub question_id: String,
    pub kind: String,
    pub prompt: String,
    pub status: String,
    pub required: bool,
    pub value: Value,
    pub rationale: String,
    pub participant_id: String,
    pub participant_kind: String,
    pub created_at: String,
    pub source: SourceContext,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewBoard {
    pub status: String,
    pub version: i64,
    pub document: String,
    pub document_truncated: bool,
    pub outline: Vec<OutlineItem>,
    pub questions: Vec<QuestionView>,
    pub comments: Vec<CommentView>,
    pub counts: ReviewCounts,
    pub answers: Vec<AnswerRecord>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewContext {
    pub review: ReviewBoard,
    pub proposals: Vec<Proposal>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewExport {
    pub format: String,
    pub body: String,
}

struct ReviewLookups<'a> {
    content: &'a str,
    version: i64,
    workspace_id: &'a str,
    kinds: &'a HashMap<String, String>,
    responses: &'a [ResponseRow],
    comments: &'a [CommentRow],
}

struct QuestionRow {
    question_id: String,
    parent_id: Option<String>,
    kind: String,
    prompt: String,
    options_json: String,
    required: i64,
    status: String,
    region: String,
    quote: String,
    prefix: String,
    suffix: String,
    base_version: i64,
    created_by: String,
    created_at: String,
    updated_at: String,
}

struct ResponseRow {
    response_id: String,
    question_id: String,
    participant_id: String,
    value_json: String,
    rationale: String,
    created_at: String,
}

struct CommentRow {
    comment_id: String,
    question_id: Option<String>,
    region: String,
    quote: String,
    body: String,
    participant_id: String,
    created_at: String,
}

struct Placement {
    region: String,
    quote: String,
    prefix: String,
    suffix: String,
}

impl WorkspaceEngine {
    pub fn review_context(&self) -> Result<ReviewContext, CommonroomError> {
        self.lock().review_context()
    }

    pub fn review_board(&self) -> Result<ReviewBoard, CommonroomError> {
        self.lock().review_board()
    }

    pub fn export_review(&self, format: &str) -> Result<ReviewExport, CommonroomError> {
        self.lock().export_review(format)
    }

    pub fn create_question(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        self.lock().create_question(session, body)
    }

    pub fn update_question(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        self.lock().update_question(session, body)
    }

    pub fn answer_question(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        self.lock().answer_question(session, body)
    }

    pub fn add_comment(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<CommentView, CommonroomError> {
        self.lock().add_comment(session, body)
    }

    pub fn set_question_status(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        self.lock().set_question_status(session, body)
    }

    pub fn set_review_status(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<ReviewContext, CommonroomError> {
        self.lock().set_review_status(session, body)
    }
}

impl EngineState {
    fn review_context(&self) -> Result<ReviewContext, CommonroomError> {
        Ok(ReviewContext {
            review: self.review_board()?,
            proposals: self.proposals()?,
        })
    }

    fn review_board(&self) -> Result<ReviewBoard, CommonroomError> {
        let content = self.read_current()?;
        let version = self.current_version()?;
        let workspace_id = self.workspace_id()?;
        let (document, document_truncated) = cap_document(&content);
        let outline = markdown::headings(&content)
            .into_iter()
            .take(self.limits.max_list_items as usize)
            .map(|item| OutlineItem {
                title: item.title,
                level: item.level,
                line: item.line,
                path: item.path,
            })
            .collect();
        let kinds = self.participant_kinds()?;
        let questions = self.question_rows()?;
        let responses = self.response_rows()?;
        let comments = self.comment_rows()?;
        let lookups = ReviewLookups {
            content: &content,
            version,
            workspace_id: &workspace_id,
            kinds: &kinds,
            responses: &responses,
            comments: &comments,
        };
        let mut views = Vec::new();
        for row in &questions {
            views.push(self.question_view(row, &lookups)?);
        }
        let loose = comments
            .iter()
            .filter(|row| row.question_id.is_none())
            .map(|row| comment_view(row, &content, &kinds))
            .collect::<Vec<_>>();
        let answers = views
            .iter()
            .filter_map(|question| {
                question.responses.last().map(|response| AnswerRecord {
                    question_id: question.question_id.clone(),
                    kind: question.kind.clone(),
                    prompt: question.prompt.clone(),
                    status: question.status.clone(),
                    required: question.required,
                    value: response.value.clone(),
                    rationale: response.rationale.clone(),
                    participant_id: response.participant_id.clone(),
                    participant_kind: response.participant_kind.clone(),
                    created_at: response.created_at.clone(),
                    source: question.anchor.clone(),
                })
            })
            .collect::<Vec<_>>();
        let counts = counts_of(&views, loose.len() as i64);
        Ok(ReviewBoard {
            status: self.review_status_value()?,
            version,
            document,
            document_truncated,
            outline,
            questions: views,
            comments: loose,
            counts,
            answers,
        })
    }

    fn export_review(&self, format: &str) -> Result<ReviewExport, CommonroomError> {
        let context = self.review_context()?;
        let body = match format {
            "json" => serde_json::to_string_pretty(&context)
                .map_err(|err| ce_protocol::internal_error(err.to_string()))?,
            "markdown" => render_markdown(&context),
            other => {
                return Err(invalid_request(format!(
                    "export format '{other}' is not json or markdown"
                )))
            }
        };
        Ok(ReviewExport {
            format: format.to_string(),
            body,
        })
    }

    fn create_question(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        self.require_cap(session, "propose")?;
        only_fields(body, CREATE_FIELDS)?;
        self.require_review_open()?;
        if let Some(key) = optional_str(body, "idempotency_key")? {
            if let Some(existing) = self.question_by_key(&session.participant_id, key)? {
                return self.one_question(&existing.question_id);
            }
        }
        let parent_id = optional_str(body, "parent_id")?.map(str::to_string);
        let parent = if let Some(parent_id) = &parent_id {
            Some(self.question_row(parent_id)?)
        } else {
            None
        };
        let kind = required_str(body, "kind")?;
        if !KINDS.contains(&kind) {
            return Err(invalid_request(
                "kind must be text, single, multi, yes_no, or approve_reject",
            ));
        }
        let prompt = bounded(required_str(body, "prompt")?, "prompt", MAX_PROMPT)?;
        if prompt.trim().is_empty() {
            return Err(invalid_request("prompt is required"));
        }
        let options = parse_options(body, kind)?;
        let required = body
            .get("required")
            .map(expect_bool)
            .transpose()?
            .unwrap_or(true);
        let region = optional_str(body, "region")?.unwrap_or("");
        let quote = optional_str(body, "quote")?.unwrap_or("");
        let prefix = optional_str(body, "prefix")?.unwrap_or("");
        let suffix = optional_str(body, "suffix")?.unwrap_or("");
        let (region, quote, prefix, suffix) = if region.is_empty() && quote.is_empty() {
            let Some(parent) = &parent else {
                return Err(invalid_request("region is required"));
            };
            (
                parent.region.clone(),
                parent.quote.clone(),
                parent.prefix.clone(),
                parent.suffix.clone(),
            )
        } else {
            (
                region.to_string(),
                quote.to_string(),
                prefix.to_string(),
                suffix.to_string(),
            )
        };
        let placement = self.place_anchor(&region, &quote, &prefix, &suffix)?;
        let count: i64 = self
            .conn()?
            .query_row("SELECT COUNT(*) FROM questions", [], |row| row.get(0))
            .db()?;
        if count >= self.limits.max_list_items {
            return Err(
                limit_exceeded("too many questions").detail("limit", self.limits.max_list_items)
            );
        }
        let question_id = next_id("question");
        let now = self.now_text();
        let workspace_id = self.workspace_id()?;
        let version = self.current_version()?;
        let region_path = placement.region.clone();
        let idempotency_key = optional_str(body, "idempotency_key")?;
        self.conn()?
            .execute(
                "INSERT INTO questions(
                    question_id, workspace_id, parent_id, kind, prompt, options_json, required,
                    status, region, quote, prefix, suffix, base_version, created_by, created_at,
                    updated_at, idempotency_key
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
                params![
                    question_id,
                    workspace_id,
                    parent_id,
                    kind,
                    prompt,
                    json_text(&options)?,
                    i64::from(required),
                    OPEN,
                    placement.region,
                    placement.quote,
                    placement.prefix,
                    placement.suffix,
                    version,
                    session.participant_id,
                    now,
                    now,
                    idempotency_key,
                ],
            )
            .db()?;
        self.record(
            QUESTION_CREATED,
            json!({
                "question_id": question_id,
                "participant_id": session.participant_id,
                "kind": kind,
                "region": region_path,
                "status": OPEN,
                "version": version,
            }),
        )?;
        self.one_question(&question_id)
    }

    fn update_question(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        self.require_cap(session, "propose")?;
        only_fields(body, UPDATE_FIELDS)?;
        self.require_review_open()?;
        let question_id = required_str(body, "question_id")?;
        let row = self.question_row(question_id)?;
        if row.created_by != session.participant_id
            && !has_capability(&session.capabilities, "admin")
        {
            return Err(permission_denied(
                "only the author or an admin can update this question",
            ));
        }
        if row.status != OPEN && row.status != CLARIFY {
            return Err(invalid_request("only an open question can be updated"));
        }
        let prompt = if body.get("prompt").is_some() {
            bounded(required_str(body, "prompt")?, "prompt", MAX_PROMPT)?
        } else {
            row.prompt.clone()
        };
        if prompt.trim().is_empty() {
            return Err(invalid_request("prompt is required"));
        }
        let options = if body.get("options").is_some() {
            parse_options(body, &row.kind)?
        } else {
            parse_stored_options(&row.options_json)?
        };
        let required = body
            .get("required")
            .map(expect_bool)
            .transpose()?
            .unwrap_or(row.required != 0);
        let now = self.now_text();
        self.conn()?
            .execute(
                "UPDATE questions SET prompt = ?1, options_json = ?2, required = ?3, updated_at = ?4
                 WHERE question_id = ?5",
                params![prompt, json_text(&options)?, i64::from(required), now, question_id],
            )
            .db()?;
        self.record(
            QUESTION_UPDATED,
            json!({
                "question_id": question_id,
                "participant_id": session.participant_id,
                "status": row.status,
            }),
        )?;
        self.one_question(question_id)
    }

    fn answer_question(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        self.require_answer(session)?;
        only_fields(body, ANSWER_FIELDS)?;
        self.require_review_open()?;
        let question_id = required_str(body, "question_id")?;
        let row = self.question_row(question_id)?;
        if row.status == RESOLVED || row.status == REJECTED {
            return Err(invalid_request("this question is closed"));
        }
        let value = body
            .get("value")
            .cloned()
            .ok_or_else(|| invalid_request("value is required"))?;
        let rationale = optional_str(body, "rationale")?.unwrap_or("");
        let rationale = bounded(rationale, "rationale", MAX_RATIONALE)?;
        let options = parse_stored_options(&row.options_json)?;
        validate_value(&row.kind, row.required != 0, &options, &value)?;
        let response_id = next_id("response");
        let now = self.now_text();
        self.conn()?
            .execute(
                "INSERT INTO responses(response_id, question_id, participant_id, value_json, rationale, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    response_id,
                    question_id,
                    session.participant_id,
                    value.to_string(),
                    rationale,
                    now
                ],
            )
            .db()?;
        self.conn()?
            .execute(
                "UPDATE questions SET status = ?1, updated_at = ?2 WHERE question_id = ?3",
                params![ANSWERED, now, question_id],
            )
            .db()?;
        self.record(
            QUESTION_ANSWERED,
            json!({
                "question_id": question_id,
                "participant_id": session.participant_id,
                "response_id": response_id,
                "status": ANSWERED,
                "region": row.region,
            }),
        )?;
        self.one_question(question_id)
    }

    fn add_comment(&self, session: &Session, body: &Value) -> Result<CommentView, CommonroomError> {
        self.require_comment(session)?;
        only_fields(body, COMMENT_FIELDS)?;
        self.require_review_open()?;
        let text = bounded(required_str(body, "body")?, "body", MAX_COMMENT)?;
        if text.trim().is_empty() {
            return Err(invalid_request("body is required"));
        }
        let question_id = optional_str(body, "question_id")?.map(str::to_string);
        let (region, quote) = if let Some(question_id) = &question_id {
            let row = self.question_row(question_id)?;
            let region = optional_str(body, "region")?.unwrap_or(&row.region);
            let quote = optional_str(body, "quote")?.unwrap_or(&row.quote);
            (region.to_string(), quote.to_string())
        } else {
            (
                required_str(body, "region")?.to_string(),
                optional_str(body, "quote")?.unwrap_or("").to_string(),
            )
        };
        if !quote.is_empty() {
            bounded(&quote, "quote", MAX_QUOTE)?;
        }
        let content = self.read_current()?;
        if markdown::locate(&content, &region).is_err() {
            return Err(
                invalid_region(format!("section '{region}' not found")).detail("region", &region)
            );
        }
        let comment_id = next_id("comment");
        let now = self.now_text();
        self.conn()?
            .execute(
                "INSERT INTO comments(comment_id, workspace_id, question_id, region, quote, body, participant_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    comment_id,
                    self.workspace_id()?,
                    question_id,
                    region,
                    quote,
                    text,
                    session.participant_id,
                    now
                ],
            )
            .db()?;
        self.record(
            COMMENT_CREATED,
            json!({
                "comment_id": comment_id,
                "question_id": question_id,
                "participant_id": session.participant_id,
                "region": region,
            }),
        )?;
        let kinds = self.participant_kinds()?;
        let row = self.comment_row(&comment_id)?;
        Ok(comment_view(&row, &content, &kinds))
    }

    fn set_question_status(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<QuestionView, CommonroomError> {
        only_fields(body, STATUS_FIELDS)?;
        self.require_review_open()?;
        let question_id = required_str(body, "question_id")?;
        let next = required_str(body, "status")?;
        let row = self.question_row(question_id)?;
        if next == ANSWERED || !matches!(next, OPEN | CLARIFY | RESOLVED | REJECTED) {
            return Err(invalid_request(
                "status must be open, needs_clarification, resolved, or rejected",
            ));
        }
        let transition = matches!(
            (row.status.as_str(), next),
            (OPEN | ANSWERED | CLARIFY, CLARIFY | OPEN)
                | (OPEN | ANSWERED | CLARIFY, RESOLVED | REJECTED)
                | (RESOLVED | REJECTED, OPEN)
        );
        if !transition {
            return Err(invalid_request("that status change is not available"));
        }
        let closing = matches!(next, RESOLVED | REJECTED);
        let allowed = if closing || matches!(row.status.as_str(), RESOLVED | REJECTED) {
            has_capability(&session.capabilities, "approve")
        } else {
            self.allows(session, "answer")
                || self.allows(session, "comment")
                || has_capability(&session.capabilities, "approve")
        };
        if !allowed {
            return Err(
                permission_denied("session cannot change this question to that status")
                    .detail("required", if closing { "approve" } else { "answer" }),
            );
        }
        let now = self.now_text();
        self.conn()?
            .execute(
                "UPDATE questions SET status = ?1, updated_at = ?2 WHERE question_id = ?3",
                params![next, now, question_id],
            )
            .db()?;
        self.record(
            QUESTION_STATUS,
            json!({
                "question_id": question_id,
                "participant_id": session.participant_id,
                "status": next,
            }),
        )?;
        self.one_question(question_id)
    }

    fn set_review_status(
        &self,
        session: &Session,
        body: &Value,
    ) -> Result<ReviewContext, CommonroomError> {
        only_fields(body, REVIEW_FIELDS)?;
        let next = required_str(body, "status")?;
        if !matches!(next, "open" | "complete" | "returned") {
            return Err(invalid_request(
                "status must be open, complete, or returned",
            ));
        }
        let current = self.review_status_value()?;
        if next != current {
            let ok = if next == "open" {
                has_capability(&session.capabilities, "admin")
            } else {
                has_capability(&session.capabilities, "approve")
            };
            if !ok {
                return Err(permission_denied(if next == "open" {
                    "reopening a review requires admin"
                } else {
                    "completing a review requires approve"
                }));
            }
            self.set_meta("review_status", next)?;
            self.record(
                REVIEW_STATUS,
                json!({
                    "participant_id": session.participant_id,
                    "status": next,
                }),
            )?;
        }
        self.review_context()
    }

    fn one_question(&self, question_id: &str) -> Result<QuestionView, CommonroomError> {
        let board = self.review_board()?;
        board
            .questions
            .into_iter()
            .find(|item| item.question_id == question_id)
            .ok_or_else(|| question_not_found("question not found"))
    }

    fn place_anchor(
        &self,
        region: &str,
        quote: &str,
        prefix: &str,
        suffix: &str,
    ) -> Result<Placement, CommonroomError> {
        let quote = bounded(quote, "quote", MAX_QUOTE)?;
        if quote.trim().is_empty() {
            return Err(invalid_request("quote is required"));
        }
        let prefix = bounded(prefix, "prefix", MAX_AFFIX)?;
        let suffix = bounded(suffix, "suffix", MAX_AFFIX)?;
        let content = self.read_current()?;
        let span = markdown::locate(&content, region).map_err(|err| locate_anchor(region, err))?;
        let section = &content[span.start..span.end];
        let hits = affix_hits(section, &quote, &prefix, &suffix);
        if hits.is_empty() {
            return Err(
                invalid_request("quote was not found in the region").detail("region", region)
            );
        }
        if hits.len() != 1 {
            return Err(
                invalid_request("quote is ambiguous in the region").detail("region", region)
            );
        }
        let at = hits[0];
        let (captured_prefix, captured_suffix) = capture(section, at, quote.len());
        Ok(Placement {
            region: span.heading.path,
            quote,
            prefix: if prefix.is_empty() {
                captured_prefix
            } else {
                prefix
            },
            suffix: if suffix.is_empty() {
                captured_suffix
            } else {
                suffix
            },
        })
    }

    fn require_review_open(&self) -> Result<(), CommonroomError> {
        let status = self.review_status_value()?;
        if status == "open" {
            Ok(())
        } else {
            Err(invalid_request(format!("review is {status}")))
        }
    }

    fn review_status_value(&self) -> Result<String, CommonroomError> {
        Ok(self.meta("review_status")?.unwrap_or_else(|| "open".into()))
    }

    fn require_cap(&self, session: &Session, capability: &str) -> Result<(), CommonroomError> {
        if has_capability(&session.capabilities, capability) {
            Ok(())
        } else {
            Err(
                permission_denied(format!("session is missing the {capability} capability"))
                    .detail("required", capability),
            )
        }
    }

    fn require_answer(&self, session: &Session) -> Result<(), CommonroomError> {
        if self.allows(session, "answer") {
            Ok(())
        } else {
            Err(
                permission_denied("session is missing the answer capability")
                    .detail("required", "answer"),
            )
        }
    }

    fn require_comment(&self, session: &Session) -> Result<(), CommonroomError> {
        if self.allows(session, "comment") {
            Ok(())
        } else {
            Err(
                permission_denied("session is missing the comment capability")
                    .detail("required", "comment"),
            )
        }
    }

    fn allows(&self, session: &Session, capability: &str) -> bool {
        has_capability(&session.capabilities, capability)
            || matches!(capability, "answer" | "comment")
                && has_capability(&session.capabilities, "propose")
    }

    fn question_rows(&self) -> Result<Vec<QuestionRow>, CommonroomError> {
        self.map_rows(
            "SELECT question_id, parent_id, kind, prompt, options_json, required, status, region,
                    quote, prefix, suffix, base_version, created_by, created_at, updated_at
             FROM questions ORDER BY created_at ASC, question_id ASC",
            [],
            |row| {
                Ok(QuestionRow {
                    question_id: row.get(0)?,
                    parent_id: row.get(1)?,
                    kind: row.get(2)?,
                    prompt: row.get(3)?,
                    options_json: row.get(4)?,
                    required: row.get(5)?,
                    status: row.get(6)?,
                    region: row.get(7)?,
                    quote: row.get(8)?,
                    prefix: row.get(9)?,
                    suffix: row.get(10)?,
                    base_version: row.get(11)?,
                    created_by: row.get(12)?,
                    created_at: row.get(13)?,
                    updated_at: row.get(14)?,
                })
            },
        )
    }

    fn question_row(&self, question_id: &str) -> Result<QuestionRow, CommonroomError> {
        self.question_rows()?
            .into_iter()
            .find(|row| row.question_id == question_id)
            .ok_or_else(|| {
                question_not_found("question not found").detail("question_id", question_id)
            })
    }

    fn question_by_key(
        &self,
        participant_id: &str,
        key: &str,
    ) -> Result<Option<QuestionRow>, CommonroomError> {
        let id: Option<String> = self
            .conn()?
            .query_row(
                "SELECT question_id FROM questions WHERE created_by = ?1 AND idempotency_key = ?2",
                params![participant_id, key],
                |row| row.get(0),
            )
            .optional()
            .db()?;
        match id {
            Some(id) => Ok(Some(self.question_row(&id)?)),
            None => Ok(None),
        }
    }

    fn response_rows(&self) -> Result<Vec<ResponseRow>, CommonroomError> {
        self.map_rows(
            "SELECT response_id, question_id, participant_id, value_json, rationale, created_at
             FROM responses ORDER BY created_at ASC, response_id ASC",
            [],
            |row| {
                Ok(ResponseRow {
                    response_id: row.get(0)?,
                    question_id: row.get(1)?,
                    participant_id: row.get(2)?,
                    value_json: row.get(3)?,
                    rationale: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
    }

    fn comment_rows(&self) -> Result<Vec<CommentRow>, CommonroomError> {
        self.map_rows(
            "SELECT comment_id, question_id, region, quote, body, participant_id, created_at
             FROM comments ORDER BY created_at ASC, comment_id ASC",
            [],
            |row| {
                Ok(CommentRow {
                    comment_id: row.get(0)?,
                    question_id: row.get(1)?,
                    region: row.get(2)?,
                    quote: row.get(3)?,
                    body: row.get(4)?,
                    participant_id: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )
    }

    fn comment_row(&self, comment_id: &str) -> Result<CommentRow, CommonroomError> {
        self.comment_rows()?
            .into_iter()
            .find(|row| row.comment_id == comment_id)
            .ok_or_else(|| invalid_request("comment not found"))
    }

    fn participant_kinds(&self) -> Result<HashMap<String, String>, CommonroomError> {
        let rows: Vec<(String, String)> =
            self.map_rows("SELECT participant_id, kind FROM participants", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        Ok(rows.into_iter().collect())
    }

    fn question_view(
        &self,
        row: &QuestionRow,
        lookups: &ReviewLookups<'_>,
    ) -> Result<QuestionView, CommonroomError> {
        let anchor = resolve_anchor(
            lookups.content,
            lookups.version,
            row.base_version,
            &row.region,
            &row.quote,
            &row.prefix,
            &row.suffix,
        );
        let responses = lookups
            .responses
            .iter()
            .filter(|item| item.question_id == row.question_id)
            .map(|item| ResponseView {
                response_id: item.response_id.clone(),
                question_id: item.question_id.clone(),
                participant_id: item.participant_id.clone(),
                participant_kind: kind_of(lookups.kinds, &item.participant_id),
                value: serde_json::from_str(&item.value_json).unwrap_or(Value::Null),
                rationale: item.rationale.clone(),
                created_at: item.created_at.clone(),
            })
            .collect();
        let comments = lookups
            .comments
            .iter()
            .filter(|item| item.question_id.as_deref() == Some(row.question_id.as_str()))
            .map(|item| comment_view(item, lookups.content, lookups.kinds))
            .collect();
        Ok(QuestionView {
            question_id: row.question_id.clone(),
            parent_id: row.parent_id.clone(),
            document_id: lookups.workspace_id.to_string(),
            kind: row.kind.clone(),
            prompt: row.prompt.clone(),
            options: parse_stored_options(&row.options_json)?,
            required: row.required != 0,
            status: row.status.clone(),
            anchor,
            created_by: row.created_by.clone(),
            created_by_kind: kind_of(lookups.kinds, &row.created_by),
            created_at: row.created_at.clone(),
            updated_at: row.updated_at.clone(),
            responses,
            comments,
        })
    }
}

fn comment_view(row: &CommentRow, content: &str, kinds: &HashMap<String, String>) -> CommentView {
    let anchor = resolve_anchor(content, 0, 0, &row.region, &row.quote, "", "");
    CommentView {
        comment_id: row.comment_id.clone(),
        question_id: row.question_id.clone(),
        region: row.region.clone(),
        quote: row.quote.clone(),
        body: row.body.clone(),
        participant_id: row.participant_id.clone(),
        participant_kind: kind_of(kinds, &row.participant_id),
        created_at: row.created_at.clone(),
        resolution: anchor.resolution,
    }
}

fn counts_of(questions: &[QuestionView], loose_comments: i64) -> ReviewCounts {
    let mut counts = ReviewCounts {
        questions: questions.len() as i64,
        open: 0,
        answered: 0,
        needs_clarification: 0,
        resolved: 0,
        rejected: 0,
        unanchored: 0,
        comments: loose_comments,
    };
    for question in questions {
        match question.status.as_str() {
            OPEN => counts.open += 1,
            ANSWERED => counts.answered += 1,
            CLARIFY => counts.needs_clarification += 1,
            RESOLVED => counts.resolved += 1,
            REJECTED => counts.rejected += 1,
            _ => {}
        }
        if question.anchor.resolution != "resolved" && question.anchor.resolution != "moved" {
            counts.unanchored += 1;
        }
        counts.comments += question.comments.len() as i64;
    }
    counts
}

fn kind_of(kinds: &HashMap<String, String>, participant_id: &str) -> String {
    kinds
        .get(participant_id)
        .cloned()
        .unwrap_or_else(|| "unknown".into())
}

fn resolve_anchor(
    content: &str,
    version: i64,
    base_version: i64,
    region: &str,
    quote: &str,
    prefix: &str,
    suffix: &str,
) -> SourceContext {
    if quote.is_empty() {
        return match markdown::locate(content, region) {
            Ok(span) => SourceContext {
                version,
                base_version,
                region: region.to_string(),
                quote: String::new(),
                resolution: "region".into(),
                resolved_region: Some(span.heading.path),
                passage: Some(window(&content[span.start..span.end], 0, 0)),
            },
            Err(_) => stale(version, base_version, region, quote),
        };
    }
    if let Ok(span) = markdown::locate(content, region) {
        let section = &content[span.start..span.end];
        if let Some(at) = choose_hit(section, quote, prefix, suffix) {
            return SourceContext {
                version,
                base_version,
                region: region.to_string(),
                quote: quote.to_string(),
                resolution: "resolved".into(),
                resolved_region: Some(span.heading.path.clone()),
                passage: Some(window(section, at, quote.len())),
            };
        }
        if raw_hits(section, quote).len() > 1 {
            return SourceContext {
                version,
                base_version,
                region: region.to_string(),
                quote: quote.to_string(),
                resolution: "ambiguous".into(),
                resolved_region: Some(span.heading.path),
                passage: None,
            };
        }
    }
    let moved_at = choose_hit(content, quote, prefix, suffix).or_else(|| {
        let hits = raw_hits(content, quote);
        if hits.len() == 1 {
            Some(hits[0])
        } else {
            None
        }
    });
    if let Some(at) = moved_at {
        return SourceContext {
            version,
            base_version,
            region: region.to_string(),
            quote: quote.to_string(),
            resolution: "moved".into(),
            resolved_region: region_at(content, at),
            passage: Some(window(content, at, quote.len())),
        };
    }
    if raw_hits(content, quote).len() > 1 {
        return SourceContext {
            version,
            base_version,
            region: region.to_string(),
            quote: quote.to_string(),
            resolution: "ambiguous".into(),
            resolved_region: None,
            passage: None,
        };
    }
    stale(version, base_version, region, quote)
}

fn choose_hit(haystack: &str, quote: &str, prefix: &str, suffix: &str) -> Option<usize> {
    let narrowed = affix_hits(haystack, quote, prefix, suffix);
    if narrowed.len() == 1 {
        return Some(narrowed[0]);
    }
    let raw = raw_hits(haystack, quote);
    if narrowed.is_empty() && raw.len() == 1 {
        return Some(raw[0]);
    }
    None
}

fn raw_hits(haystack: &str, quote: &str) -> Vec<usize> {
    affix_hits(haystack, quote, "", "")
}

fn stale(version: i64, base_version: i64, region: &str, quote: &str) -> SourceContext {
    SourceContext {
        version,
        base_version,
        region: region.to_string(),
        quote: quote.to_string(),
        resolution: "stale".into(),
        resolved_region: None,
        passage: None,
    }
}

fn region_at(content: &str, offset: usize) -> Option<String> {
    let headings = markdown::headings(content);
    let mut chosen = None;
    for heading in &headings {
        if heading.start <= offset {
            chosen = Some(heading.path.clone());
        } else {
            break;
        }
    }
    chosen
}

fn affix_hits(haystack: &str, quote: &str, prefix: &str, suffix: &str) -> Vec<usize> {
    if quote.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(quote) {
        let at = start + pos;
        if affix_matches(haystack, at, quote, prefix, suffix) {
            hits.push(at);
        }
        start = at + quote.len();
        if hits.len() > 8 || start > haystack.len() {
            break;
        }
    }
    hits
}

fn affix_matches(haystack: &str, at: usize, quote: &str, prefix: &str, suffix: &str) -> bool {
    if !prefix.is_empty() && !haystack[..at].ends_with(prefix) {
        return false;
    }
    if !suffix.is_empty() {
        let after = at + quote.len();
        if after > haystack.len() || !haystack[after..].starts_with(suffix) {
            return false;
        }
    }
    true
}

fn capture(section: &str, at: usize, quote_len: usize) -> (String, String) {
    let mut start = at.saturating_sub(40);
    while start < at && !section.is_char_boundary(start) {
        start += 1;
    }
    let prefix = section[start..at].to_string();
    let end_base = at + quote_len;
    let mut end = (end_base + 40).min(section.len());
    while end > end_base && !section.is_char_boundary(end) {
        end -= 1;
    }
    let suffix = section[end_base..end].to_string();
    (prefix, suffix)
}

fn window(section: &str, at: usize, quote_len: usize) -> String {
    let quote_end = (at + quote_len).min(section.len());
    let mut start = at.saturating_sub(120);
    while start < at && !section.is_char_boundary(start) {
        start += 1;
    }
    let mut end = (quote_end + 120).min(section.len());
    while end > quote_end && !section.is_char_boundary(end) {
        end -= 1;
    }
    section[start..end].trim().to_string()
}

fn cap_document(content: &str) -> (String, bool) {
    if content.len() <= DOCUMENT_CAP {
        return (content.to_string(), false);
    }
    let mut end = DOCUMENT_CAP;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    (content[..end].to_string(), true)
}

fn only_fields(body: &Value, allowed: &[&str]) -> Result<(), CommonroomError> {
    let Some(map) = body.as_object() else {
        return Err(invalid_request("request body must be an object"));
    };
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid_request(format!(
                "unknown field '{key}'. Question definitions cannot include executable UI"
            )));
        }
    }
    Ok(())
}

fn required_str<'a>(body: &'a Value, key: &str) -> Result<&'a str, CommonroomError> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_request(format!("{key} is required")))
}

fn optional_str<'a>(body: &'a Value, key: &str) -> Result<Option<&'a str>, CommonroomError> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            if value.chars().count() > MAX_KEY
                && matches!(key, "idempotency_key" | "parent_id" | "question_id")
            {
                return Err(invalid_request(format!("{key} is too long")));
            }
            Ok(Some(value.as_str()).filter(|item| !item.is_empty()))
        }
        Some(_) => Err(invalid_request(format!("{key} must be a string"))),
    }
}

fn expect_bool(value: &Value) -> Result<bool, CommonroomError> {
    value
        .as_bool()
        .ok_or_else(|| invalid_request("required must be a boolean"))
}

fn bounded(value: &str, name: &str, max_chars: usize) -> Result<String, CommonroomError> {
    if value.chars().count() > max_chars {
        return Err(limit_exceeded(format!("{name} is too long"))
            .detail("limit", max_chars)
            .detail("size", value.chars().count()));
    }
    Ok(value.to_string())
}

fn parse_options(body: &Value, kind: &str) -> Result<Vec<String>, CommonroomError> {
    let choices = matches!(kind, "single" | "multi");
    let Some(raw) = body.get("options") else {
        if choices {
            return Err(invalid_request("options are required for this kind"));
        }
        return Ok(Vec::new());
    };
    if !choices {
        if raw.as_array().is_some_and(|items| !items.is_empty()) {
            return Err(invalid_request("this kind does not take options"));
        }
        return Ok(Vec::new());
    }
    let Some(items) = raw.as_array() else {
        return Err(invalid_request("options must be an array of strings"));
    };
    if items.len() < 2 || items.len() > MAX_OPTIONS {
        return Err(invalid_request("options must contain 2 to 8 strings"));
    }
    let mut options = Vec::new();
    for item in items {
        let Some(text) = item.as_str() else {
            return Err(invalid_request("options must be an array of strings"));
        };
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_OPTION {
            return Err(invalid_request(
                "each option must be a short non-empty string",
            ));
        }
        if options.iter().any(|existing: &String| existing == text) {
            return Err(invalid_request("options must be unique"));
        }
        options.push(text.to_string());
    }
    Ok(options)
}

fn parse_stored_options(raw: &str) -> Result<Vec<String>, CommonroomError> {
    serde_json::from_str(raw).map_err(|err| ce_protocol::internal_error(err.to_string()))
}

fn validate_value(
    kind: &str,
    required: bool,
    options: &[String],
    value: &Value,
) -> Result<(), CommonroomError> {
    match kind {
        "text" => {
            let Some(text) = value.as_str() else {
                return Err(invalid_request("text answers must be a string"));
            };
            if text.chars().count() > MAX_TEXT {
                return Err(limit_exceeded("answer is too long").detail("limit", MAX_TEXT));
            }
            if required && text.trim().is_empty() {
                return Err(invalid_request("this question requires an answer"));
            }
        }
        "single" => {
            let Some(choice) = value.as_str() else {
                return Err(invalid_request("single-choice answers must be one option"));
            };
            if !options.iter().any(|option| option == choice) {
                return Err(invalid_request("answer is not one of the options"));
            }
        }
        "multi" => {
            let Some(choices) = value.as_array() else {
                return Err(invalid_request("multiple-choice answers must be an array"));
            };
            if required && choices.is_empty() {
                return Err(invalid_request("this question requires an answer"));
            }
            let mut seen = Vec::new();
            for choice in choices {
                let Some(text) = choice.as_str() else {
                    return Err(invalid_request("each choice must be a string"));
                };
                if !options.iter().any(|option| option == text) {
                    return Err(invalid_request("answer is not one of the options"));
                }
                if seen.contains(&text) {
                    return Err(invalid_request("choices must be unique"));
                }
                seen.push(text);
            }
        }
        "yes_no" => {
            if !value.is_boolean() {
                return Err(invalid_request("yes/no answers must be a boolean"));
            }
        }
        "approve_reject" => {
            let Some(choice) = value.as_str() else {
                return Err(invalid_request(
                    "approve/reject answers must be approve or reject",
                ));
            };
            if choice != "approve" && choice != "reject" {
                return Err(invalid_request(
                    "approve/reject answers must be approve or reject",
                ));
            }
        }
        _ => return Err(invalid_request("unsupported question kind")),
    }
    Ok(())
}

fn locate_anchor(region: &str, err: LocateError) -> CommonroomError {
    match err {
        LocateError::NotFound => {
            invalid_region(format!("section '{region}' not found")).detail("region", region)
        }
        LocateError::Ambiguous(candidates) => {
            invalid_region(format!("section '{region}' matches more than one heading"))
                .detail("region", region)
                .detail("candidates", candidates)
        }
    }
}

fn render_markdown(context: &ReviewContext) -> String {
    let review = &context.review;
    let mut out = String::new();
    out.push_str(&review.document);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n---\n\n## Review\n\n");
    out.push_str(&format!(
        "Status: {} · version {}\n\n",
        review.status, review.version
    ));
    out.push_str(
        "This appendix is an export. It is not part of the source document, and saving it does not apply a change.\n\n",
    );
    for question in &review.questions {
        out.push_str(&format!(
            "### {}\n\nRegion: {} · {} · {}\n\n{}\n\n",
            question.question_id,
            question.anchor.region,
            question.kind,
            question.status,
            question.prompt
        ));
        if let Some(passage) = &question.anchor.passage {
            out.push_str(&format!(
                "Passage ({}) :\n\n> {}\n\n",
                question.anchor.resolution,
                passage.replace('\n', "\n> ")
            ));
        } else {
            out.push_str(&format!(
                "Anchor is {} and was not attached to a passage.\n\n",
                question.anchor.resolution
            ));
        }
        if let Some(answer) = question.responses.last() {
            out.push_str(&format!("Answer: {}\n\n", answer.value));
            if !answer.rationale.is_empty() {
                out.push_str(&format!("Rationale: {}\n\n", answer.rationale));
            }
        }
        for comment in &question.comments {
            out.push_str(&format!("Comment: {}\n\n", comment.body));
        }
    }
    if !context.proposals.is_empty() {
        out.push_str("## Proposed changes\n\nThese are not part of the source until they are approved and committed.\n\n");
        for proposal in &context.proposals {
            out.push_str(&format!(
                "- {} · {} · {} · {}\n",
                proposal.proposal_id, proposal.status, proposal.target, proposal.summary
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ce_protocol::Limits;

    fn open_room(content: &str) -> (tempfile_dir::Dir, WorkspaceEngine) {
        let dir = tempfile_dir::Dir::new();
        let path = dir.0.join("notes.md");
        std::fs::write(&path, content).unwrap();
        let engine = WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap();
        (dir, engine)
    }

    mod tempfile_dir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                use std::sync::atomic::{AtomicU64, Ordering};
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let path = std::env::temp_dir().join(format!(
                    "ce-review-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    const DOC: &str = "# Storage\n\n## Decision\n\nKeep the sidecar in SQLite.\n\n## Risk\n\nA second database would split the room.\n";

    #[test]
    fn answer_survives_restart_and_a_proposal_leaves_the_file() {
        let (dir, engine) = open_room(DOC);
        let path = dir.0.join("notes.md");
        let asker = engine
            .join(
                "agent",
                "asker",
                Some(vec!["read".into(), "propose".into()]),
                None,
                "local",
                None,
            )
            .unwrap();
        let created = engine
            .create_question(
                &asker,
                &json!({
                    "kind": "yes_no",
                    "prompt": "Should the sidecar stay in SQLite?",
                    "region": "Storage/Decision",
                    "quote": "Keep the sidecar in SQLite.",
                    "idempotency_key": "q-1"
                }),
            )
            .unwrap();
        assert_eq!(created.anchor.resolution, "resolved");
        assert!(created
            .anchor
            .passage
            .as_deref()
            .unwrap()
            .contains("SQLite"));
        let again = engine
            .create_question(
                &asker,
                &json!({
                    "kind": "yes_no",
                    "prompt": "ignored",
                    "region": "Storage/Decision",
                    "quote": "Keep the sidecar in SQLite.",
                    "idempotency_key": "q-1"
                }),
            )
            .unwrap();
        assert_eq!(again.question_id, created.question_id);
        let host = engine.ensure_host("Ada", "local").unwrap();
        engine
            .answer_question(
                &host,
                &json!({
                    "question_id": created.question_id,
                    "value": true,
                    "rationale": "One writer is enough."
                }),
            )
            .unwrap();
        engine.close();
        let engine = WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap();
        let context = engine.review_context().unwrap();
        assert_eq!(context.review.answers.len(), 1);
        let answer = &context.review.answers[0];
        assert_eq!(answer.question_id, created.question_id);
        assert_eq!(answer.value, true);
        assert_eq!(answer.rationale, "One writer is enough.");
        assert!(answer.source.passage.as_deref().unwrap().contains("SQLite"));
        let before = std::fs::read_to_string(&path).unwrap();
        engine
            .propose(
                &asker.participant_id,
                "Storage/Decision",
                1,
                "record the decision",
                json!({"content": "## Decision\n\nKeep the sidecar in SQLite. Confirmed.\n"}),
                Some(&asker),
            )
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert_eq!(engine.proposals().unwrap()[0].status, "pending");
        assert_eq!(engine.status().unwrap().version, 1);
    }

    #[test]
    fn authorization_schema_and_stale_anchors_fail_clearly() {
        let (dir, engine) = open_room(DOC);
        let path = dir.0.join("notes.md");
        let reader = engine
            .join(
                "human",
                "reader",
                Some(vec!["read".into(), "observe".into()]),
                None,
                "local",
                None,
            )
            .unwrap();
        let asker = engine
            .join(
                "agent",
                "asker",
                Some(vec!["read".into(), "propose".into()]),
                None,
                "local",
                None,
            )
            .unwrap();
        let err = engine
            .create_question(
                &reader,
                &json!({"kind": "text", "prompt": "Why?", "region": "Risk", "quote": "split the room"}),
            )
            .unwrap_err();
        assert_eq!(err.code, "permission_denied");
        let err = engine
            .create_question(
                &asker,
                &json!({"kind": "widget", "prompt": "Why?", "html": "<script></script>"}),
            )
            .unwrap_err();
        assert_eq!(err.code, "invalid_request");
        let err = engine
            .create_question(
                &asker,
                &json!({
                    "kind": "text",
                    "prompt": "Where?",
                    "region": "Storage/Decision",
                    "quote": "this sentence is not in the section"
                }),
            )
            .unwrap_err();
        assert_eq!(err.code, "invalid_request");
        let created = engine
            .create_question(
                &asker,
                &json!({
                    "kind": "single",
                    "prompt": "Which store?",
                    "region": "Decision",
                    "quote": "Keep the sidecar in SQLite.",
                    "options": ["SQLite", "Postgres"]
                }),
            )
            .unwrap();
        let err = engine
            .answer_question(
                &reader,
                &json!({"question_id": created.question_id, "value": "SQLite"}),
            )
            .unwrap_err();
        assert_eq!(err.code, "permission_denied");
        let answerer = engine
            .join(
                "human",
                "reviewer",
                Some(vec!["read".into(), "answer".into()]),
                None,
                "local",
                None,
            )
            .unwrap();
        let err = engine
            .answer_question(
                &answerer,
                &json!({"question_id": created.question_id, "value": "MySQL"}),
            )
            .unwrap_err();
        assert_eq!(err.code, "invalid_request");
        engine
            .answer_question(
                &answerer,
                &json!({"question_id": created.question_id, "value": "SQLite", "rationale": "local"}),
            )
            .unwrap();
        let err = engine
            .create_question(
                &answerer,
                &json!({"kind": "text", "prompt": "no", "region": "Risk", "quote": "split the room"}),
            )
            .unwrap_err();
        assert_eq!(err.code, "permission_denied");
        std::fs::write(
            &path,
            "# Storage\n\n## Decision\n\nUse a different sentence.\n",
        )
        .unwrap();
        let view = engine.review_board().unwrap();
        assert_eq!(view.questions[0].anchor.resolution, "stale");
        assert!(view.questions[0].anchor.passage.is_none());
        assert_eq!(view.answers[0].value, "SQLite");
    }

    #[test]
    fn a_moved_quote_is_flagged_instead_of_dropped() {
        let (dir, engine) = open_room(DOC);
        let path = dir.0.join("notes.md");
        let asker = engine
            .join(
                "agent",
                "asker",
                Some(vec!["propose".into()]),
                None,
                "local",
                None,
            )
            .unwrap();
        engine
            .create_question(
                &asker,
                &json!({
                    "kind": "text",
                    "prompt": "Who owns the file?",
                    "region": "Storage/Risk",
                    "quote": "split the room",
                    "required": false
                }),
            )
            .unwrap();
        std::fs::write(
            &path,
            "# Notes\n\n## Later\n\nA second database would split the room.\n",
        )
        .unwrap();
        let view = engine.review_board().unwrap();
        assert_eq!(view.questions[0].anchor.resolution, "moved");
        assert_eq!(
            view.questions[0].anchor.resolved_region.as_deref(),
            Some("Notes/Later")
        );
    }
}
