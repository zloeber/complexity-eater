use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ce_core::WorkspaceEngine;
use ce_http::{create_router, Access};
use ce_protocol::{Limits, MCP_PROTOCOL_VERSION, SKILL_URI};
use futures_util::StreamExt;
use serde_json::json;

struct TempDir(std::path::PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn engine(content: &str) -> (TempDir, Arc<WorkspaceEngine>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("ce-http-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("workspace.md");
    std::fs::write(&path, content).unwrap();
    let engine = WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap();
    (TempDir(dir), Arc::new(engine))
}

async fn serve(router: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}")
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[tokio::test]
async fn ingress_requires_a_session_and_discovery_is_public() {
    let (_dir, engine) = engine("# Plan\n");
    let base = serve(create_router(engine, Access::Session, None)).await;
    let client = client();
    let discovery = client
        .get(format!("{base}/.well-known/agent-workspace"))
        .send()
        .await
        .unwrap();
    assert_eq!(discovery.status(), 200);
    let body = discovery.json::<serde_json::Value>().await.unwrap();
    assert!(body["operations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "workspace.status"));
    assert_eq!(
        client
            .get(format!("{base}/protocol"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        client
            .get(format!("{base}/skills/commonroom"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    let denied = client
        .get(format!("{base}/workspace/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    let denied = denied.json::<serde_json::Value>().await.unwrap();
    assert_eq!(denied["error"], "unauthorized");
    let page = client
        .get(format!("{base}/"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!page.contains("session"));
}

#[tokio::test]
async fn owner_cookie_reviews_and_agent_cannot_bypass() {
    let (_dir, engine) = engine("# Architecture\n\n## Goals\n\nship\n");
    let host = engine.ensure_host("Zach", "local").unwrap();
    engine
        .set_transport(Some(json!({
            "type": "loopback",
            "address": "secret-endpoint",
            "port": 9,
            "encrypted": true,
            "mode": "private"
        })))
        .unwrap();
    let owner_url = serve(create_router(
        Arc::clone(&engine),
        Access::Owner,
        Some(host.clone()),
    ))
    .await;
    let ingress_url = serve(create_router(Arc::clone(&engine), Access::Session, None)).await;
    let owner = client();
    let page = owner.get(format!("{owner_url}/")).send().await.unwrap();
    let text = page.text().await.unwrap();
    assert!(!text.contains(&host.session_id));
    assert!(text.contains("id=\"theme\""));
    assert!(text.contains("prefers-color-scheme: dark"));
    assert!(text.contains("new EventSource"));
    assert!(text.contains("Members who have not left"));
    assert!(!text.contains("setInterval(() => load()"));
    let status = owner
        .get(format!("{owner_url}/workspace/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(status.status(), 200);
    let status = status.json::<serde_json::Value>().await.unwrap();
    assert_eq!(status["version"], 1);
    assert!(status["path"].is_string());
    let mcp = owner
        .post(format!("{owner_url}/mcp"))
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {}}))
        .send()
        .await
        .unwrap();
    assert_eq!(mcp.status(), 404);
    let room = owner
        .get(format!("{owner_url}/workspace/room"))
        .send()
        .await
        .unwrap();
    let room = room.json::<serde_json::Value>().await.unwrap();
    assert_eq!(room["status"]["name"], "workspace");
    assert!(room["diagnostics"]["listener"].get("address").is_none());

    let invite = engine
        .create_invite_with(
            None,
            None,
            60,
            Some(1),
            Some(vec![
                "read".into(),
                "observe".into(),
                "propose".into(),
                "checkout".into(),
                "commit".into(),
            ]),
        )
        .unwrap();
    let ingress = client();
    let joined = ingress
        .post(format!("{ingress_url}/session/join"))
        .json(&json!({"invite": invite.token, "name": "Agent A", "kind": "agent", "transport": "loopback"}))
        .send()
        .await
        .unwrap();
    assert_eq!(joined.status(), 200);
    let joined = joined.json::<serde_json::Value>().await.unwrap();
    assert_eq!(
        joined["capabilities"],
        json!(["read", "observe", "propose", "checkout", "commit"])
    );
    let session = joined["session_id"].as_str().unwrap();
    let raw = ingress
        .post(format!("{ingress_url}/workspace/commit"))
        .bearer_auth(session)
        .json(&json!({"base_version": 1, "summary": "overwrite", "content": "nope\n"}))
        .send()
        .await
        .unwrap();
    assert_eq!(raw.status(), 403);
    assert_eq!(
        raw.json::<serde_json::Value>().await.unwrap()["error"],
        "permission_denied"
    );
    let public = ingress
        .get(format!("{ingress_url}/workspace/diagnostics"))
        .bearer_auth(session)
        .send()
        .await
        .unwrap();
    let public_text = public.text().await.unwrap();
    assert!(!public_text.contains("secret-endpoint"));
    let public: serde_json::Value = serde_json::from_str(&public_text).unwrap();
    assert_eq!(public["listener"]["type"], "loopback");
    assert_eq!(public["listener"]["encrypted"], true);

    let proposed = ingress
        .post(format!("{ingress_url}/workspace/propose"))
        .bearer_auth(session)
        .json(&json!({
            "target": "section:Goals",
            "base_version": 1,
            "summary": "sharpen the goal",
            "change": {"section": "Goals", "content": "## Goals\n\nship the room\n"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        proposed.status(),
        200,
        "{}",
        proposed.text().await.unwrap_or_default()
    );
}

#[tokio::test]
async fn mcp_discovery_hides_secrets_and_filters_tools() {
    let (_dir, engine) = engine("# Architecture\n\n## Goals\n\nship\n");
    let invite = engine.create_invite().unwrap();
    let session = engine
        .join(
            "agent",
            "Agent",
            None,
            Some(&invite.token),
            "loopback",
            None,
        )
        .unwrap();
    let base = serve(create_router(engine, Access::Session, None)).await;
    let client = client();
    let response = client
        .post(format!("{base}/mcp"))
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "server/discover")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "server/discover",
            "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION}}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body = response.json::<serde_json::Value>().await.unwrap();
    assert_eq!(
        body["result"]["supportedVersions"],
        json!([MCP_PROTOCOL_VERSION])
    );
    assert_eq!(body["result"]["cacheScope"], "public");
    assert!(
        body["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/skills"].is_object()
    );
    assert!(
        body["result"]["capabilities"]["extensions"]["io.modelcontextprotocol/tasks"].is_object()
    );
    let blob = body.to_string();
    assert!(!blob.contains(&invite.token));
    assert!(!blob.contains("session_id"));

    let listed = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&session.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "tools/list")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/list",
            "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION}}
        }))
        .send()
        .await
        .unwrap();
    let listed = listed.json::<serde_json::Value>().await.unwrap();
    let names: Vec<_> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.iter().any(|name| name == "workspace_review"));
    assert!(!names.iter().any(|name| name == "handoff_approve"));
    assert!(names.iter().any(|name| name == "workspace_propose"));
    assert!(names.iter().any(|name| name == "handoff_create"));

    let skills = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&session.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "skills/list")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "skills/list",
            "params": {"_meta": {"io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION}}
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap();
    assert_eq!(skills["result"]["skills"][0]["uri"], SKILL_URI);
}

fn stream_client() -> reqwest::Client {
    reqwest::Client::builder()
        .cookie_store(true)
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap()
}

fn mcp_params(mut params: serde_json::Value) -> serde_json::Value {
    if !params.is_object() {
        params = json!({});
    }
    params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION});
    params
}

async fn collect_stream(url: String, bearer: String, accept_sse: bool) -> Arc<Mutex<String>> {
    let buf = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&buf);
    tokio::spawn(async move {
        let client = stream_client();
        let mut request = client.get(url).bearer_auth(bearer);
        if accept_sse {
            request = request
                .header("Accept", "text/event-stream")
                .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION);
        }
        let Ok(response) = request.send().await else {
            return;
        };
        if !response.status().is_success() {
            let status = response.status().as_u16();
            shared.lock().unwrap().push_str(&format!("status:{status}"));
            return;
        }
        let mut stream = response.bytes_stream();
        loop {
            match tokio::time::timeout(Duration::from_secs(6), stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    shared
                        .lock()
                        .unwrap()
                        .push_str(&String::from_utf8_lossy(&chunk));
                }
                _ => break,
            }
        }
    });
    buf
}

async fn wait_for(buf: &Mutex<String>, needle: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let text = buf.lock().unwrap().clone();
        if text.contains(needle) || Instant::now() >= deadline {
            return text;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn observe_requires_a_session_and_pushes_proposal_events() {
    let (_dir, engine) = engine("# Architecture\n\n## Goals\n\nship\n");
    let host = engine.ensure_host("Ada", "local").unwrap();
    let base = serve(create_router(
        Arc::clone(&engine),
        Access::Owner,
        Some(host.clone()),
    ))
    .await;
    let anon = client()
        .get(format!("{base}/workspace/observe"))
        .send()
        .await
        .unwrap();
    assert_eq!(anon.status(), 401);
    let owner = stream_client();
    owner.get(format!("{base}/")).send().await.unwrap();
    let response = owner
        .get(format!("{base}/workspace/observe?since_event_id=0"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/event-stream"));
    let buf = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&buf);
    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        loop {
            match tokio::time::timeout(Duration::from_secs(6), stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    shared
                        .lock()
                        .unwrap()
                        .push_str(&String::from_utf8_lossy(&chunk));
                }
                _ => break,
            }
        }
    });
    let opened = wait_for(&buf, "retry:").await;
    assert!(opened.contains("workspace.created"), "{opened}");
    let agent = engine
        .join(
            "agent",
            "writer",
            Some(vec!["propose".into(), "read".into()]),
            None,
            "local",
            None,
        )
        .unwrap();
    engine
        .propose(
            &agent.participant_id,
            "section:Goals",
            1,
            "sharpen",
            json!({"section": "Goals", "content": "## Goals\n\nsecret-proposal-body\n"}),
            Some(&agent),
        )
        .unwrap();
    let live = wait_for(&buf, "proposal.created").await;
    assert!(live.contains("proposal.created"), "{live}");
    assert!(live.contains("id:"));
    assert!(!live.contains("secret-proposal-body"), "{live}");
    assert!(!live.contains(&host.session_id));
}

#[tokio::test]
async fn a_session_cannot_observe_another_workspace() {
    let (_dir_a, engine_a) = engine("# Architecture\nA\n");
    let (_dir_b, engine_b) = engine("# Architecture\nB\n");
    let session = engine_a
        .join(
            "agent",
            "a",
            Some(vec!["observe".into(), "read".into()]),
            None,
            "local",
            None,
        )
        .unwrap();
    let base = serve(create_router(engine_b, Access::Session, None)).await;
    let response = client()
        .get(format!("{base}/workspace/observe"))
        .bearer_auth(&session.session_id)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let mcp = client().get(format!("{base}/mcp")).send().await.unwrap();
    assert_eq!(mcp.status(), 405);
}

#[tokio::test]
async fn mcp_resource_subscriptions_notify_and_reject_unauthorized_uris() {
    let (_dir, engine) = engine("# Architecture\n\n## Goals\n\nship\n");
    let session = engine
        .join(
            "agent",
            "agent",
            Some(vec![
                "read".into(),
                "observe".into(),
                "propose".into(),
                "checkout".into(),
            ]),
            None,
            "local",
            None,
        )
        .unwrap();
    let watcher = engine
        .join(
            "agent",
            "watcher",
            Some(vec!["observe".into()]),
            None,
            "local",
            None,
        )
        .unwrap();
    let base = serve(create_router(Arc::clone(&engine), Access::Session, None)).await;
    let client = stream_client();
    let denied = client
        .get(format!("{base}/mcp"))
        .header("Accept", "text/event-stream")
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), 401);
    let buf = collect_stream(format!("{base}/mcp"), session.session_id.clone(), true).await;
    let opened = wait_for(&buf, "retry:").await;
    assert!(opened.contains("retry:"), "{opened}");
    let unknown = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&session.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "resources/subscribe")
        .header("Mcp-Name", "commonroom://workspace/nope")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "resources/subscribe",
            "params": mcp_params(json!({"uri": "commonroom://workspace/nope"}))
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(unknown.status(), 400);
    let unknown = unknown.text().await.unwrap();
    assert!(unknown.contains("unknown resource"));
    assert!(!unknown.contains("secret"));
    let oversized = "commonroom://workspace/".to_string() + &"x".repeat(600);
    let too_long = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&session.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "resources/subscribe")
        .header("Mcp-Name", &oversized)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "resources/subscribe",
            "params": mcp_params(json!({"uri": oversized}))
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(too_long.status(), 400);
    let forbidden = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&watcher.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "resources/subscribe")
        .header("Mcp-Name", "commonroom://workspace/proposals")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "resources/subscribe",
            "params": mcp_params(json!({"uri": "commonroom://workspace/proposals"}))
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(forbidden.status(), 403);
    let forbidden = forbidden.text().await.unwrap();
    assert!(forbidden.contains("read"));
    assert!(!forbidden.contains("## Goals"));
    let subscribed = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&session.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "resources/subscribe")
        .header("Mcp-Name", "commonroom://workspace/proposals")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "resources/subscribe",
            "params": mcp_params(json!({"uri": "commonroom://workspace/proposals"}))
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        subscribed.status(),
        200,
        "{}",
        subscribed.text().await.unwrap_or_default()
    );
    engine
        .checkout(
            &session.participant_id,
            "section:Architecture",
            None,
            Some(&session),
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !buf.lock()
            .unwrap()
            .contains("commonroom://workspace/proposals"),
        "a lease must not notify a proposals subscription: {}",
        buf.lock().unwrap()
    );
    engine
        .propose(
            &session.participant_id,
            "section:Goals",
            1,
            "sharpen",
            json!({"section": "Goals", "content": "## Goals\n\nsecret-proposal-body\n"}),
            Some(&session),
        )
        .unwrap();
    let notes = wait_for(&buf, "notifications/resources/updated").await;
    assert!(
        notes.contains("commonroom://workspace/proposals"),
        "{notes}"
    );
    assert!(!notes.contains("secret-proposal-body"), "{notes}");
    let status = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&watcher.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "tools/call")
        .header("Mcp-Name", "workspace_status")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "tools/call",
            "params": mcp_params(json!({"name": "workspace_status", "arguments": {}}))
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(status.status(), 200);
    let status = status.text().await.unwrap();
    assert!(status.contains("isError"));
    let reader = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&session.session_id)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", "resources/read")
        .header("Mcp-Name", "commonroom://workspace/document")
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "resources/read",
            "params": mcp_params(json!({"uri": "commonroom://workspace/document"}))
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(reader.status(), 200);
    let reader = reader.text().await.unwrap();
    assert!(reader.contains("# Architecture"));
    let again = collect_stream(format!("{base}/mcp"), session.session_id.clone(), true).await;
    let replay = wait_for(&again, "notifications/resources/updated").await;
    assert!(
        replay.contains("commonroom://workspace/proposals"),
        "{replay}"
    );
}

#[tokio::test]
async fn leaving_closes_the_event_stream() {
    let (_dir, engine) = engine("# Architecture\n");
    let session = engine
        .join(
            "agent",
            "agent",
            Some(vec!["observe".into(), "read".into()]),
            None,
            "local",
            None,
        )
        .unwrap();
    let base = serve(create_router(Arc::clone(&engine), Access::Session, None)).await;
    let buf = collect_stream(
        format!("{base}/workspace/observe"),
        session.session_id.clone(),
        false,
    )
    .await;
    let opened = wait_for(&buf, "retry:").await;
    assert!(opened.contains("retry:"), "{opened}");
    engine.leave(&session.participant_id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline && !buf.lock().unwrap().contains("status:") {
        if buf.lock().unwrap().contains("participant.left") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    engine
        .propose(
            "nobody",
            "workspace",
            1,
            "late",
            json!({"content": "after-close-secret"}),
            None,
        )
        .ok();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let text = buf.lock().unwrap().clone();
    assert!(!text.contains("after-close-secret"), "{text}");
    assert!(!text.contains("proposal.created"), "{text}");
}

fn tasks_meta(task: bool) -> serde_json::Value {
    let mut meta = json!({"io.modelcontextprotocol/protocolVersion": MCP_PROTOCOL_VERSION});
    if task {
        meta["io.modelcontextprotocol/clientCapabilities"] = json!({
            "extensions": {"io.modelcontextprotocol/tasks": {}}
        });
    }
    meta
}

async fn mcp(
    client: &reqwest::Client,
    base: &str,
    bearer: &str,
    method: &str,
    name: Option<&str>,
    params: serde_json::Value,
) -> serde_json::Value {
    let mut request = client
        .post(format!("{base}/mcp"))
        .bearer_auth(bearer)
        .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
        .header("Mcp-Method", method);
    if let Some(name) = name {
        request = request.header("Mcp-Name", name);
    }
    let response = request
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": method,
            "params": params
        }))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body = response.json::<serde_json::Value>().await.unwrap();
    assert!(
        status.is_success() || status.is_client_error(),
        "{status} {body}"
    );
    body
}

#[tokio::test]
async fn handoff_survives_restart_and_notifies_only_authorized_subscribers() {
    let (_other_dir, other_engine) = engine("# Other\n");
    let (dir, engine) = engine("# Plan\n");
    let maker = engine
        .join(
            "agent",
            "maker",
            Some(vec!["read".into(), "propose".into(), "commit".into()]),
            None,
            "local",
            None,
        )
        .unwrap();
    let worker = engine
        .join(
            "agent",
            "worker",
            Some(vec!["read".into(), "commit".into()]),
            None,
            "local",
            None,
        )
        .unwrap();
    let stranger = engine
        .join(
            "agent",
            "stranger",
            Some(vec!["read".into()]),
            None,
            "local",
            None,
        )
        .unwrap();
    let created = engine
        .create_handoff(
            &maker.participant_id,
            ce_core::NewHandoff {
                summary: "write the note",
                constraints: "keep it short",
                artifact_refs: &[],
                idempotency_key: Some("note-1"),
                correlation_id: Some("corr-note"),
            },
        )
        .unwrap();
    engine
        .accept_handoff(&worker.participant_id, &created.handoff.handoff_id)
        .unwrap();
    let path = engine.workspace_path();
    let db = engine.db_path();
    let maker_session = maker.session_id.clone();
    let worker_session = worker.session_id.clone();
    let stranger_session = stranger.session_id.clone();
    let task_id = created.task.task_id.clone();
    let handoff_id = created.handoff.handoff_id.clone();
    engine.close();
    drop(engine);
    let reopened =
        Arc::new(WorkspaceEngine::open(&path, Some(db), Limits::default(), None).unwrap());
    let base = serve(create_router(Arc::clone(&reopened), Access::Session, None)).await;
    let client = client();

    let missing = mcp(
        &client,
        &base,
        &maker_session,
        "tasks/get",
        Some(&task_id),
        json!({"taskId": task_id, "_meta": tasks_meta(false)}),
    )
    .await;
    assert_eq!(missing["error"]["code"], -32021);

    let hidden = mcp(
        &client,
        &base,
        &stranger_session,
        "tasks/get",
        Some(&task_id),
        json!({"taskId": task_id, "_meta": tasks_meta(true)}),
    )
    .await;
    assert_eq!(hidden["error"]["code"], -32602);

    let legacy = client
        .post(format!("{base}/mcp"))
        .bearer_auth(&maker_session)
        .header("MCP-Protocol-Version", "2025-11-25")
        .header("Mcp-Method", "tasks/get")
        .header("Mcp-Name", &task_id)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tasks/get",
            "params": {
                "taskId": task_id,
                "_meta": {"io.modelcontextprotocol/protocolVersion": "2025-11-25"}
            }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(legacy.status(), 400);
    assert_eq!(
        legacy.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        -32022
    );

    let opened = mcp(
        &client,
        &base,
        &maker_session,
        "tools/call",
        Some("handoff_create"),
        json!({
            "name": "handoff_create",
            "arguments": {"summary": "second note", "idempotency_key": "note-2"},
            "_meta": tasks_meta(true)
        }),
    )
    .await;
    assert_eq!(opened["result"]["resultType"], "task");
    assert_eq!(opened["result"]["status"], "working");
    let replay = mcp(
        &client,
        &base,
        &maker_session,
        "tools/call",
        Some("handoff_create"),
        json!({
            "name": "handoff_create",
            "arguments": {"summary": "second note", "idempotency_key": "note-2"},
            "_meta": tasks_meta(true)
        }),
    )
    .await;
    assert_eq!(replay["result"]["taskId"], opened["result"]["taskId"]);

    let other_base = serve(create_router(other_engine, Access::Session, None)).await;
    let crossed = mcp(
        &client,
        &other_base,
        &maker_session,
        "tasks/get",
        Some(&task_id),
        json!({"taskId": task_id, "_meta": tasks_meta(true)}),
    )
    .await;
    assert_eq!(crossed["error"]["code"], -32000);

    let listen = collect_listen(
        format!("{base}/mcp"),
        maker_session.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "subscriptions/listen",
            "params": {
                "notifications": {
                    "taskIds": [task_id],
                    "toolsListChanged": true,
                    "resourceSubscriptions": ["commonroom://workspace/proposals"]
                },
                "_meta": tasks_meta(true)
            }
        }),
    );
    let ack = wait_for(&listen, "subscriptions/acknowledged").await;
    assert!(ack.contains(&task_id), "{ack}");
    assert!(!ack.contains("toolsListChanged"), "{ack}");
    let stranger_listen = collect_listen(
        format!("{base}/mcp"),
        stranger_session.clone(),
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "subscriptions/listen",
            "params": {
                "notifications": {"taskIds": [task_id]},
                "_meta": tasks_meta(true)
            }
        }),
    );
    let stranger_ack = wait_for(&stranger_listen, "subscriptions/acknowledged").await;
    assert!(!stranger_ack.contains(&task_id), "{stranger_ack}");

    let completed = mcp(
        &client,
        &base,
        &worker_session,
        "tools/call",
        Some("handoff_complete"),
        json!({
            "name": "handoff_complete",
            "arguments": {"handoff_id": handoff_id, "result": "shipped the note"},
            "_meta": tasks_meta(false)
        }),
    )
    .await;
    assert_eq!(completed["result"]["isError"], false);
    let note = wait_for(&listen, "notifications/tasks").await;
    assert!(note.contains("completed"), "{note}");
    assert!(note.contains("shipped the note"), "{note}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !stranger_listen
            .lock()
            .unwrap()
            .contains("notifications/tasks"),
        "{}",
        stranger_listen.lock().unwrap()
    );

    let current = mcp(
        &client,
        &base,
        &maker_session,
        "tasks/get",
        Some(&task_id),
        json!({"taskId": task_id, "_meta": tasks_meta(true)}),
    )
    .await;
    assert_eq!(current["result"]["status"], "completed");
    assert_eq!(current["result"]["resultType"], "complete");
    let again = mcp(
        &client,
        &base,
        &worker_session,
        "tools/call",
        Some("handoff_complete"),
        json!({
            "name": "handoff_complete",
            "arguments": {"handoff_id": handoff_id, "result": "shipped the note again"},
            "_meta": tasks_meta(false)
        }),
    )
    .await;
    assert_eq!(again["result"]["isError"], false);
    let stored = reopened.get_handoff(&handoff_id).unwrap();
    assert_eq!(stored.result_text.as_deref(), Some("shipped the note"));
    let _ = dir;
}

fn collect_listen(url: String, bearer: String, body: serde_json::Value) -> Arc<Mutex<String>> {
    let buf = Arc::new(Mutex::new(String::new()));
    let shared = Arc::clone(&buf);
    tokio::spawn(async move {
        let client = stream_client();
        let Ok(response) = client
            .post(url)
            .bearer_auth(bearer)
            .header("MCP-Protocol-Version", MCP_PROTOCOL_VERSION)
            .header("Mcp-Method", "subscriptions/listen")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
        else {
            return;
        };
        if !response.status().is_success() {
            shared
                .lock()
                .unwrap()
                .push_str(&format!("status:{}", response.status()));
            return;
        }
        let mut stream = response.bytes_stream();
        loop {
            match tokio::time::timeout(Duration::from_secs(6), stream.next()).await {
                Ok(Some(Ok(chunk))) => {
                    shared
                        .lock()
                        .unwrap()
                        .push_str(&String::from_utf8_lossy(&chunk));
                }
                _ => break,
            }
        }
    });
    buf
}
