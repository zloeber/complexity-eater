use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ce_cli::start_room;
use ce_core::WorkspaceEngine;
use ce_protocol::{decode_invitation, encode_invitation, Limits, MCP_PROTOCOL_VERSION};
use serde_json::{json, Value};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(content: &str) -> (TempDir, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "ce-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("idea.md");
    std::fs::write(&path, content).unwrap();
    (TempDir(dir), path)
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ce")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin()).args(args).output().expect("run ce")
}

fn mcp(uri: &str, state: &std::path::Path, message: Option<&str>) -> std::process::Output {
    let mut child = Command::new(bin())
        .args(["mcp", "--invite", uri, "--name", "Agent B"])
        .env("XDG_STATE_HOME", state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp");
    if let Some(message) = message {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(message.as_bytes())
            .unwrap();
    } else if let Some(stdin) = child.stdin.take() {
        drop(stdin);
    }
    child.wait_with_output().expect("mcp")
}

#[test]
fn help_and_version_name_the_product() {
    let help = run(&["--help"]);
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(help.status.success());
    assert!(text.contains("Complexity Eater"));
    assert!(text.contains("serve"));
    assert!(text.contains("mcp"));
    let version = run(&["--version"]);
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains("0.1.0"));
}

#[test]
fn status_hides_the_transport_address_and_history_hides_the_secret() {
    let (_dir, path) = scratch("# Architecture\n");
    let engine = WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap();
    engine
        .set_transport(Some(json!({
            "type": "loopback",
            "address": "secret-endpoint",
            "port": 9,
            "encrypted": false,
            "mode": "local"
        })))
        .unwrap();
    let invite = engine.create_invite().unwrap();
    engine.close();
    let status = run(&["status", "--workspace", path.to_str().unwrap(), "--json"]);
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(!text.contains("secret-endpoint"));
    let body: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(body["listener"]["type"], "loopback");
    let history = run(&["history", "--workspace", path.to_str().unwrap(), "--json"]);
    let history_text = String::from_utf8_lossy(&history.stdout);
    assert!(!history_text.contains(&invite.token));
}

#[test]
fn join_rejects_malformed_and_unsupported_invitations() {
    let malformed = run(&[
        "join",
        "commonroom://join/$$$$",
        "--name",
        "Agent",
        "--no-hold",
    ]);
    assert_eq!(malformed.status.code(), Some(1));
    let body: Value = serde_json::from_slice(&malformed.stdout).unwrap();
    assert_eq!(body["error"], "invalid_request");
    let document = json!({
        "protocol": "commonroom",
        "version": "9",
        "transport": {"type": "loopback", "address": "127.0.0.1", "port": 1},
        "workspace_id": "workspace_x",
        "secret": "secret",
        "expires_at": "2099-01-01T00:00:00+00:00",
        "capabilities": ["read"]
    });
    let unsupported = run(&[
        "join",
        &document.to_string(),
        "--name",
        "Agent",
        "--no-hold",
    ]);
    assert_eq!(unsupported.status.code(), Some(1));
    let body: Value = serde_json::from_slice(&unsupported.stdout).unwrap();
    assert_eq!(body["error"], "unsupported_version");
}

#[tokio::test]
async fn dialer_joins_once_and_a_restart_reuses_the_session() {
    let (_dir, path) = scratch("# Architecture\n\n## Goals\n\nship\n");
    let engine = Arc::new(WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap());
    let mut room = start_room(Arc::clone(&engine), "127.0.0.1", 0, "Zach", None)
        .await
        .expect("room");
    let state = _dir.0.join("state");
    let uri = room.invite.uri.clone();
    let token = room.invite.token.clone();
    let state_dir = state.clone();
    let discover = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {}})
    );
    let first = tokio::task::spawn_blocking(move || mcp(&uri, &state_dir, Some(&discover)))
        .await
        .unwrap();
    assert_eq!(
        first.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&first.stderr)
    );
    let discovered: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(
        discovered["result"]["supportedVersions"],
        json!([MCP_PROTOCOL_VERSION])
    );
    assert!(!String::from_utf8_lossy(&first.stdout).contains(&token));
    assert!(!String::from_utf8_lossy(&first.stderr).contains(&token));
    assert!(!String::from_utf8_lossy(&first.stderr).contains("ses_"));

    let uri = room.invite.uri.clone();
    let state_dir = state.clone();
    let tools = format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    );
    let listed = tokio::task::spawn_blocking(move || mcp(&uri, &state_dir, Some(&tools)))
        .await
        .unwrap();
    assert_eq!(
        listed.status.code(),
        Some(0),
        "stderr={}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    let names: Vec<_> = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_string())
        .collect();
    assert!(!names.iter().any(|name| name == "workspace_review"));
    assert!(names.iter().any(|name| name == "workspace_propose"));
    assert_eq!(engine.list_invites().unwrap()[0].used_count, 1);
    room.shutdown().await;
    engine.close();
}

#[tokio::test]
async fn failed_dial_does_not_spend_the_invitation() {
    let (_dir, path) = scratch("# Architecture\n");
    let engine = Arc::new(WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap());
    engine
        .set_transport(Some(json!({
            "type": "loopback",
            "address": "127.0.0.1",
            "port": 1,
            "encrypted": false,
            "mode": "local"
        })))
        .unwrap();
    let invite = engine.create_invite().unwrap();
    let failed = run(&[
        "join",
        &invite.uri,
        "--name",
        "Agent",
        "--no-hold",
        "--json",
    ]);
    assert_eq!(
        failed.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&failed.stdout)
    );
    assert_eq!(engine.list_invites().unwrap()[0].used_count, 0);

    let mut room = start_room(engine.clone(), "127.0.0.1", 0, "Zach", None)
        .await
        .expect("room");
    let document = decode_invitation(&room.invite.uri).unwrap();
    let mut hint = document["transport"].clone();
    hint["port"] = json!(1);
    let bad = encode_invitation(&json!({
        "transport": hint,
        "workspace_id": document["workspace_id"],
        "secret": document["secret"],
        "expires_at": document["expires_at"],
        "capabilities": document["capabilities"]
    }))
    .unwrap();
    let dial = mcp(&bad, &_dir.0.join("state"), None);
    assert_eq!(dial.status.code(), Some(1));
    let err: Value = serde_json::from_slice(&dial.stderr).unwrap();
    assert_eq!(err["error"], "transport_unavailable");
    assert!(!String::from_utf8_lossy(&dial.stderr).contains(&room.invite.token));
    assert_eq!(engine.list_invites().unwrap()[0].used_count, 0);
    room.shutdown().await;
    engine.close();
}

#[tokio::test]
async fn call_joins_proposes_and_rejects_a_second_join() {
    let (_dir, path) = scratch("# Architecture\n\n## Goals\n\nship\n");
    let engine = Arc::new(WorkspaceEngine::open(&path, None, Limits::default(), None).unwrap());
    let mut room = start_room(engine.clone(), "127.0.0.1", 0, "Zach", None)
        .await
        .expect("room");
    let uri = room.invite.uri.clone();
    let joined = tokio::task::spawn_blocking(move || {
        run(&["join", &uri, "--name", "Agent A", "--json", "--no-hold"])
    })
    .await
    .unwrap();
    assert_eq!(
        joined.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&joined.stdout)
    );
    let joined_body: Value = serde_json::from_slice(&joined.stdout).unwrap();
    let session_id = joined_body["session_id"].as_str().unwrap().to_string();
    let uri = room.invite.uri.clone();
    let denied =
        tokio::task::spawn_blocking(move || run(&["call", &uri, "GET", "/workspace/status"]))
            .await
            .unwrap();
    assert_eq!(denied.status.code(), Some(1));
    let denied: Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(denied["status"], 401);

    let body = json!({
        "target": "section:Goals",
        "base_version": 1,
        "summary": "sharpen the goal",
        "change": {"section": "Goals", "content": "## Goals\n\nship the room\n"}
    })
    .to_string();
    let uri = room.invite.uri.clone();
    let session = session_id.clone();
    let proposed = tokio::task::spawn_blocking(move || {
        run(&[
            "call",
            &uri,
            "POST",
            "/workspace/propose",
            "--session",
            &session,
            "--body",
            &body,
        ])
    })
    .await
    .unwrap();
    assert_eq!(
        proposed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&proposed.stdout)
    );
    let proposed: Value = serde_json::from_slice(&proposed.stdout).unwrap();
    let proposal_id = proposed["body"]["proposal_id"]
        .as_str()
        .unwrap()
        .to_string();
    engine
        .review_proposal(
            &proposal_id,
            &room.host_session.participant_id,
            true,
            Some(&room.host_session),
        )
        .unwrap();
    let commit_body = json!({"proposal_id": proposal_id}).to_string();
    let uri = room.invite.uri.clone();
    let session = session_id.clone();
    let committed = tokio::task::spawn_blocking(move || {
        run(&[
            "call",
            &uri,
            "POST",
            "/workspace/commit",
            "--session",
            &session,
            "--body",
            &commit_body,
        ])
    })
    .await
    .unwrap();
    assert_eq!(
        committed.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&committed.stdout)
    );
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("ship the room"), "{text}");

    let uri = room.invite.uri.clone();
    let again = tokio::task::spawn_blocking(move || {
        run(&["join", &uri, "--name", "Agent A", "--json", "--no-hold"])
    })
    .await
    .unwrap();
    assert_eq!(again.status.code(), Some(1));
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(again["error"], "invitation_redeemed");
    room.shutdown().await;
    engine.close();
}

#[test]
fn serve_prints_a_ready_snapshot_without_the_ingress_address() {
    let (_dir, path) = scratch("# Architecture\n");
    let mut child = Command::new(bin())
        .args([
            "serve",
            path.to_str().unwrap(),
            "--port",
            "0",
            "--json",
            "--ephemeral",
            "--name",
            "Zach",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = stdout;
        let mut data = Vec::new();
        let mut buf = [0u8; 256];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    data.extend_from_slice(&buf[..n]);
                    let _ = tx.send(data.clone());
                }
            }
        }
    });
    let start = Instant::now();
    let mut latest = Vec::new();
    let snapshot = loop {
        if let Ok(data) = rx.recv_timeout(Duration::from_millis(200)) {
            latest = data;
            if let Ok(value) = serde_json::from_slice::<Value>(&latest) {
                break value;
            }
        }
        if start.elapsed() > Duration::from_secs(8) {
            panic!("snapshot not ready: {}", String::from_utf8_lossy(&latest));
        }
    };
    assert!(snapshot["invite"]
        .as_str()
        .unwrap()
        .starts_with("commonroom://join/"));
    assert!(snapshot["local_url"]
        .as_str()
        .unwrap()
        .starts_with("http://127.0.0.1:"));
    assert!(snapshot["listener"].get("address").is_none());
    assert_eq!(snapshot["listener"]["type"], "loopback");
    assert_eq!(snapshot["name"], "idea");
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let _ = child.wait();
}
