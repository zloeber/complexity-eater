use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use ce_core::{Invite, Session, WorkspaceEngine};
use ce_http::{create_router, Access};
use ce_protocol::{invalid_request, paste_message, CommonroomError};
use ce_transport::{create_transport, Transport};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub struct RunningRoom {
    pub engine: Arc<WorkspaceEngine>,
    pub local_url: String,
    pub invite: Invite,
    pub host_session: Session,
    transport: Option<Box<dyn Transport>>,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
    closed: bool,
}

impl RunningRoom {
    pub fn snapshot(&self) -> Result<Value, CommonroomError> {
        let status = self.engine.status()?;
        Ok(json!({
            "name": status.name,
            "workspace_id": status.workspace_id,
            "version": status.version,
            "lifecycle": status.lifecycle,
            "local_url": self.local_url,
            "invite": self.invite.uri,
            "listener": self.engine.connection_diagnostics()?.listener,
            "participants": self.engine.participants()?,
        }))
    }

    pub fn banner(&self) -> Result<String, CommonroomError> {
        Ok(render_banner(&self.snapshot()?))
    }

    pub async fn shutdown(&mut self) {
        self.finish();
        let tasks = std::mem::take(&mut self.tasks);
        for task in tasks {
            let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
        }
    }

    fn finish(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let _ = self.engine.mark_stopped();
        let _ = self.engine.set_transport(None);
        if let Some(mut transport) = self.transport.take() {
            transport.close();
        }
        let _ = self.shutdown.send(true);
    }
}

impl Drop for RunningRoom {
    fn drop(&mut self) {
        self.finish();
        for task in self.tasks.drain(..) {
            task.abort();
        }
    }
}

pub fn render_banner(snapshot: &Value) -> String {
    let mut lines = vec![
        "Complexity Eater".to_string(),
        String::new(),
        format!(
            "Workspace: {}",
            snapshot.get("name").and_then(Value::as_str).unwrap_or("")
        ),
        format!(
            "Version: {}",
            snapshot.get("version").and_then(Value::as_i64).unwrap_or(0)
        ),
        format!(
            "Status: {}",
            snapshot
                .get("lifecycle")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
        String::new(),
        "Human:".to_string(),
        format!(
            "  {}",
            snapshot
                .get("local_url")
                .and_then(Value::as_str)
                .unwrap_or("")
        ),
        String::new(),
    ];
    let listener = snapshot
        .get("listener")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(kind) = listener
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| !kind.is_empty())
    {
        let mode = listener
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("local");
        let encrypted = listener
            .get("encrypted")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let encrypted_text = if encrypted {
            "encrypted"
        } else {
            "not encrypted"
        };
        lines.push("Transport:".to_string());
        lines.push(format!("  {kind} ({mode}, {encrypted_text})"));
        if mode == "private" && encrypted {
            lines.push("  Status: READY".to_string());
        }
        lines.push(String::new());
    }
    if let Some(invite) = snapshot
        .get("invite")
        .and_then(Value::as_str)
        .filter(|invite| !invite.is_empty())
    {
        lines.push("Invite:".to_string());
        lines.push(format!("  {invite}"));
        lines.push(String::new());
        lines.push("For an agent:".to_string());
        lines.push(paste_message(invite).trim_end().to_string());
        lines.push(String::new());
    }
    lines.push("Participants:".to_string());
    let people = snapshot.get("participants").and_then(Value::as_array);
    if people.map(|items| items.is_empty()).unwrap_or(true) {
        lines.push("  (none)".to_string());
    } else if let Some(people) = people {
        for person in people {
            let kind = person.get("kind").and_then(Value::as_str).unwrap_or("");
            let name = person.get("name").and_then(Value::as_str).unwrap_or("");
            lines.push(format!("  {kind}: {name}"));
        }
    }
    lines.join("\n") + "\n"
}

pub async fn start_room(
    engine: Arc<WorkspaceEngine>,
    host: &str,
    port: u16,
    name: &str,
    transport_name: Option<&str>,
) -> Result<RunningRoom, CommonroomError> {
    if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return Err(invalid_request(
            "refusing non-loopback bind; use a transport plugin for remote access",
        ));
    }
    let host_session = engine.ensure_host(name, "local")?;
    let owner = TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|err| invalid_request(format!("workspace server did not start: {err}")))?;
    let owner_port = owner
        .local_addr()
        .map_err(|err| invalid_request(format!("workspace server did not start: {err}")))?
        .port();
    let ingress = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|err| invalid_request(format!("workspace server did not start: {err}")))?;
    let ingress_port = ingress
        .local_addr()
        .map_err(|err| invalid_request(format!("workspace server did not start: {err}")))?
        .port();
    let (shutdown, watch_rx) = watch::channel(false);
    let mut tasks = vec![
        spawn_server(
            owner,
            create_router(
                Arc::clone(&engine),
                Access::Owner,
                Some(host_session.clone()),
            ),
            watch_rx.clone(),
        ),
        spawn_server(
            ingress,
            create_router(Arc::clone(&engine), Access::Session, None),
            watch_rx,
        ),
    ];
    let mut plugin = match transport_name {
        Some(name) => Some(create_transport(name)?),
        None => None,
    };
    let prepared = (|| {
        if let Some(plugin) = plugin.as_mut() {
            engine.set_transport(Some(plugin.expose(ingress_port)?))?;
        } else {
            engine.set_transport(Some(json!({
                "type": "loopback",
                "address": "127.0.0.1",
                "port": ingress_port,
                "encrypted": false,
                "mode": "local"
            })))?;
        }
        let invite = engine.create_invite()?;
        engine.mark_started()?;
        Ok(invite)
    })();
    let invite = match prepared {
        Ok(invite) => invite,
        Err(err) => {
            let _ = engine.set_transport(None);
            if let Some(mut plugin) = plugin {
                plugin.close();
            }
            let _ = shutdown.send(true);
            for task in tasks {
                task.abort();
            }
            return Err(err);
        }
    };
    wait_until_accepts(owner_port);
    wait_until_accepts(ingress_port);
    Ok(RunningRoom {
        engine,
        local_url: format!("http://127.0.0.1:{owner_port}"),
        invite,
        host_session,
        transport: plugin,
        shutdown,
        tasks: std::mem::take(&mut tasks),
        closed: false,
    })
}

fn wait_until_accepts(port: u16) {
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_millis(50),
        )
        .is_ok()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn spawn_server(
    listener: TcpListener,
    router: Router,
    mut shutdown: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let server = axum::serve(listener, router).with_graceful_shutdown(async move {
            let _ = shutdown.changed().await;
        });
        let _ = server.await;
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_hides_the_endpoint_and_names_the_product() {
        let text = render_banner(&json!({
            "name": "idea",
            "version": 3,
            "lifecycle": "ACTIVE",
            "local_url": "http://127.0.0.1:8000",
            "invite": "commonroom://join/abc",
            "listener": {"type": "loopback", "mode": "local", "encrypted": false},
            "participants": [{"kind": "human", "name": "Zach"}]
        }));
        assert!(text.starts_with("Complexity Eater\n"));
        assert!(text.contains("commonroom://join/abc"));
        assert!(text.contains("Zach"));
        assert!(!text.contains("tailcat://"));
        assert!(!text.contains("Status: READY"));
        assert!(text.contains("args: [\"mcp\", \"--invite\", \"commonroom://join/abc\"]"));
        let private = render_banner(&json!({
            "name": "idea",
            "version": 3,
            "lifecycle": "ACTIVE",
            "local_url": "http://127.0.0.1:8000",
            "invite": "commonroom://join/abc",
            "listener": {"type": "private", "mode": "private", "encrypted": true},
            "participants": []
        }));
        assert!(private.contains("Status: READY"));
        assert!(private.contains("127.0.0.1:8000"));
    }

    #[tokio::test]
    async fn local_room_uses_loopback_without_a_plugin_binary() {
        let dir = std::env::temp_dir().join(format!("ce-room-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("idea.md");
        std::fs::write(&path, "# Architecture\n").unwrap();
        let engine = Arc::new(
            WorkspaceEngine::open(&path, None, ce_protocol::Limits::default(), None).unwrap(),
        );
        let refused = match start_room(Arc::clone(&engine), "0.0.0.0", 0, "Zach", None).await {
            Err(err) => err,
            Ok(_) => panic!("non-loopback bind was accepted"),
        };
        assert!(refused.message.contains("refusing non-loopback bind"));
        let mut room = match start_room(Arc::clone(&engine), "127.0.0.1", 0, "Zach", None).await {
            Ok(room) => room,
            Err(err) => panic!("{err}"),
        };
        let document = ce_protocol::decode_invitation(&room.invite.uri).unwrap();
        assert_eq!(document["transport"]["type"], "loopback");
        assert_eq!(document["transport"]["address"], "127.0.0.1");
        let banner = room.banner().unwrap();
        assert!(banner.contains("Zach"));
        assert!(!banner.contains("Status: READY"));
        assert!(!banner.contains("tailcat://"));
        room.shutdown().await;
        engine.close();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn byte_client_can_join_the_ingress() {
        let dir = std::env::temp_dir().join(format!("ce-byte-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("idea.md");
        std::fs::write(&path, "# Architecture\n").unwrap();
        let engine = Arc::new(
            WorkspaceEngine::open(&path, None, ce_protocol::Limits::default(), None).unwrap(),
        );
        let mut room = match start_room(Arc::clone(&engine), "127.0.0.1", 0, "Zach", None).await {
            Ok(room) => room,
            Err(err) => panic!("{err}"),
        };
        let document = ce_protocol::decode_invitation(&room.invite.uri).unwrap();
        let secret = document["secret"].as_str().unwrap().to_string();
        let (status, body) = tokio::task::spawn_blocking(move || {
            let (mut plugin, mut connection, _hint) =
                ce_transport::connect_invitation(&document).unwrap();
            let result = crate::http_client::http_exchange(
                connection.as_mut(),
                "POST",
                "/session/join",
                Some(&json!({
                    "invite": secret,
                    "name": "Agent",
                    "kind": "agent",
                    "transport": "loopback"
                })),
                None,
            );
            connection.close();
            plugin.close();
            result
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(status, 200, "{body}");
        assert!(body["session_id"].as_str().unwrap().starts_with("ses_"));
        room.shutdown().await;
        engine.close();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
