use std::sync::Arc;
use std::time::Duration;

use ce_core::WorkspaceEngine;
use ce_http::{create_router, Access};
use ce_protocol::{Limits, MCP_PROTOCOL_VERSION, SKILL_URI};
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
    assert!(names.iter().any(|name| name == "workspace_propose"));

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
