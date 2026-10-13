use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ce_protocol::{decode_invitation, CommonroomError, MCP_PROTOCOL_VERSION};
use ce_transport::{connect_invitation, Connection, SocketConnection, Transport};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const NAME_FIELDS: &[(&str, &str)] = &[
    ("tools/call", "name"),
    ("prompts/get", "name"),
    ("resources/read", "uri"),
    ("resources/subscribe", "uri"),
    ("resources/unsubscribe", "uri"),
];

pub fn run_dialer(invite: &str, name: &str, state_dir: Option<&Path>) -> i32 {
    let document = match decode_invitation(invite) {
        Ok(document) => document,
        Err(err) => {
            fail(err.code, err.message);
            return 1;
        }
    };
    let secret = document
        .get("secret")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut link = match Link::open(&document) {
        Ok(link) => link,
        Err(err) => {
            fail(err.code, err.message);
            return 1;
        }
    };
    let code = match restore_or_join(&mut link, &document, &secret, name, state_dir) {
        Ok(Some(session_id)) => {
            forward(&mut link, &session_id, &document);
            0
        }
        Ok(None) => 1,
        Err(err) => {
            fail(err.code, err.message);
            1
        }
    };
    link.close();
    code
}

fn restore_or_join(
    link: &mut Link,
    document: &Value,
    secret: &str,
    name: &str,
    state_dir: Option<&Path>,
) -> Result<Option<String>, CommonroomError> {
    let path = session_path(secret, state_dir);
    if let Some(stored) = read_session(&path) {
        if stored.get("workspace_id") != document.get("workspace_id") {
            let _ = std::fs::remove_file(&path);
            fail(
                "unauthorized",
                "the session ended; ask the host for a new invitation",
            );
            return Ok(None);
        }
        let session_id = stored
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let (status, _) = link.exchange(
            "GET",
            "/workspace/status",
            None,
            Some(&[("Authorization".into(), format!("Bearer {session_id}"))]),
        )?;
        if status == 200 {
            return Ok(Some(session_id));
        }
        let _ = std::fs::remove_file(&path);
        fail(
            "unauthorized",
            "the session ended; ask the host for a new invitation",
        );
        return Ok(None);
    }
    let transport = link
        .hint
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("remote");
    let (status, body) = link.exchange(
        "POST",
        "/session/join",
        Some(&json!({
            "invite": secret,
            "name": name,
            "kind": "agent",
            "transport": transport
        })),
        None,
    )?;
    let Some(session_id) = body
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        if let Some(code) = body.get("error").and_then(Value::as_str) {
            fail(
                code,
                body.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("join failed"),
            );
        } else {
            fail("unauthorized", "join failed");
        }
        return Ok(None);
    };
    if status != 200 {
        if let Some(code) = body.get("error").and_then(Value::as_str) {
            fail(
                code,
                body.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("join failed"),
            );
        } else {
            fail("unauthorized", "join failed");
        }
        return Ok(None);
    }
    if body.get("workspace_id") != document.get("workspace_id") {
        fail(
            "invalid_request",
            "session workspace does not match the invitation",
        );
        return Ok(None);
    }
    write_session(
        &path,
        &json!({
            "session_id": session_id,
            "workspace_id": body.get("workspace_id").and_then(Value::as_str).unwrap_or(""),
            "participant_id": body.get("participant_id").and_then(Value::as_str).unwrap_or(""),
            "expires_at": body.get("expires_at").and_then(Value::as_str).unwrap_or("")
        }),
    )?;
    Ok(Some(session_id))
}

fn forward(link: &mut Link, session_id: &str, document: &Value) {
    let stop = Arc::new(AtomicBool::new(false));
    let notify_stop = Arc::clone(&stop);
    let hint = document
        .get("transport")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let session = session_id.to_string();
    let notify = std::thread::spawn(move || notify_loop(&notify_stop, hint, session));
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else {
            break;
        };
        let text = line.trim();
        if text.is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(_) => {
                write_stdout(
                    &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}}),
                );
                continue;
            }
        };
        if !message.is_object() {
            continue;
        }
        let prepared = with_protocol(&message);
        let Ok((status, body)) = link.exchange(
            "POST",
            "/mcp",
            Some(&prepared),
            Some(&mcp_headers(&prepared, session_id)),
        ) else {
            write_stdout(
                &json!({"jsonrpc": "2.0", "id": prepared.get("id").cloned().unwrap_or(Value::Null), "error": {"code": -32000, "message": "transport failed"}}),
            );
            continue;
        };
        if status == 202 {
            continue;
        }
        if body.is_object() {
            write_stdout(&body);
        }
    }
    stop.store(true, Ordering::Relaxed);
    let _ = notify.join();
}

fn with_protocol(message: &Value) -> Value {
    let mut prepared = message.clone();
    let mut params = prepared
        .get("params")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let mut meta = params
        .get("_meta")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(map) = meta.as_object_mut() {
        map.entry("io.modelcontextprotocol/protocolVersion")
            .or_insert_with(|| Value::String(MCP_PROTOCOL_VERSION.into()));
        map.entry("io.modelcontextprotocol/clientCapabilities")
            .or_insert_with(|| json!({}));
    }
    if let Some(map) = params.as_object_mut() {
        map.insert("_meta".into(), meta);
    }
    if let Some(map) = prepared.as_object_mut() {
        map.insert("params".into(), params);
    }
    prepared
}

fn mcp_headers(message: &Value, session_id: &str) -> Vec<(String, String)> {
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let meta = params.get("_meta").cloned().unwrap_or_else(|| json!({}));
    let version = meta
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(MCP_PROTOCOL_VERSION);
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let mut headers = vec![
        ("Authorization".into(), format!("Bearer {session_id}")),
        ("MCP-Protocol-Version".into(), version.to_string()),
        ("Mcp-Method".into(), method.to_string()),
        (
            "Accept".into(),
            "application/json, text/event-stream".into(),
        ),
    ];
    if let Some((_, field)) = NAME_FIELDS.iter().find(|(name, _)| *name == method) {
        if let Some(value) = params.get(*field).and_then(Value::as_str) {
            headers.push(("Mcp-Name".into(), value.to_string()));
        }
    }
    headers
}

fn session_path(secret: &str, state_dir: Option<&Path>) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    let digest = hex::encode(hasher.finalize());
    state_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(default_state_dir)
        .join("commonroom")
        .join("sessions")
        .join(digest)
}

fn default_state_dir() -> PathBuf {
    if let Ok(value) = std::env::var("XDG_STATE_HOME") {
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }
    ce_core::home_dir().join(".local").join("state")
}

fn read_session(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let payload: Value = serde_json::from_str(&text).ok()?;
    if payload
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .is_empty()
    {
        return None;
    }
    Some(payload)
}

fn write_session(path: &Path, payload: &Value) -> Result<(), CommonroomError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| ce_protocol::internal_error(err.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    std::fs::write(path, payload.to_string())
        .map_err(|err| ce_protocol::internal_error(err.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn fail(code: impl AsRef<str>, message: impl AsRef<str>) {
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "{}",
        json!({"error": code.as_ref(), "message": message.as_ref()})
    );
    let _ = err.flush();
}

fn write_stdout(payload: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{payload}");
    let _ = out.flush();
}

enum StreamEnd {
    Stop,
    Retry,
}

fn notify_loop(stop: &AtomicBool, hint: Value, session_id: String) {
    while !stop.load(Ordering::Relaxed) {
        match read_notifications(stop, &hint, &session_id) {
            StreamEnd::Stop => break,
            StreamEnd::Retry => {
                for _ in 0..5 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
            }
        }
    }
}

fn read_notifications(stop: &AtomicBool, hint: &Value, session_id: &str) -> StreamEnd {
    let mut connection = match open_notify(hint) {
        Ok(connection) => connection,
        Err(_) => {
            return if stop.load(Ordering::Relaxed) {
                StreamEnd::Stop
            } else {
                StreamEnd::Retry
            };
        }
    };
    if connection
        .send(notify_request(session_id).as_bytes())
        .is_err()
    {
        return StreamEnd::Retry;
    }
    let mut raw = Vec::new();
    let (status, chunked, length, mut pending) = loop {
        if stop.load(Ordering::Relaxed) {
            return StreamEnd::Stop;
        }
        match connection.receive(8192) {
            Ok(chunk) if chunk.is_empty() => return StreamEnd::Retry,
            Ok(chunk) => {
                raw.extend_from_slice(&chunk);
                if let Some(parsed) = crate::http_client::header_complete(&raw) {
                    break parsed;
                }
                if raw.len() > 65_536 {
                    return StreamEnd::Stop;
                }
            }
            Err(_) => {
                if stop.load(Ordering::Relaxed) {
                    return StreamEnd::Stop;
                }
            }
        }
    };
    if matches!(status, 401 | 403 | 404 | 405) {
        return StreamEnd::Stop;
    }
    if status != 200 {
        return StreamEnd::Retry;
    }
    let mut decoder = crate::http_client::ChunkDecoder::new();
    let mut text = String::new();
    let mut received = 0usize;
    loop {
        if stop.load(Ordering::Relaxed) {
            return StreamEnd::Stop;
        }
        let decoded = if chunked {
            let decoded = decoder.push(&pending);
            pending.clear();
            decoded
        } else {
            std::mem::take(&mut pending)
        };
        if !decoded.is_empty() {
            received += decoded.len();
            text.push_str(&String::from_utf8_lossy(&decoded));
            emit_notifications(&mut text);
        }
        if chunked && decoder.is_done() {
            return StreamEnd::Retry;
        }
        if length.is_some_and(|length| !chunked && received >= length) {
            return StreamEnd::Stop;
        }
        match connection.receive(8192) {
            Ok(chunk) if chunk.is_empty() => return StreamEnd::Retry,
            Ok(chunk) => pending = chunk,
            Err(_) => {}
        }
    }
}

fn emit_notifications(buffer: &mut String) {
    for message in crate::http_client::take_sse_json(buffer) {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        if method.starts_with("notifications/") {
            write_stdout(&message);
        }
    }
}

fn notify_request(session_id: &str) -> String {
    format!(
        "GET /mcp HTTP/1.1\r\nHost: commonroom\r\nAuthorization: Bearer {session_id}\r\nAccept: text/event-stream\r\nMCP-Protocol-Version: {MCP_PROTOCOL_VERSION}\r\n\r\n"
    )
}

fn open_notify(hint: &Value) -> Result<Box<dyn Connection>, CommonroomError> {
    let host = hint
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or("127.0.0.1");
    if matches!(host, "127.0.0.1" | "localhost") {
        if let Some(port) = hint.get("port").and_then(Value::as_u64) {
            let address = format!("{host}:{port}")
                .parse()
                .map_err(|_| ce_protocol::transport_unavailable("connect failed"))?;
            let sock = TcpStream::connect_timeout(&address, Duration::from_secs(2))
                .map_err(|_| ce_protocol::transport_unavailable("connect failed"))?;
            return Ok(Box::new(SocketConnection::new(
                sock,
                Duration::from_millis(200),
            )));
        }
    }
    let (_plugin, connection, _hint) = connect_invitation(&json!({"transport": hint}))?;
    Ok(connection)
}

struct Link {
    plugin: Box<dyn Transport>,
    hint: Value,
    connection: Option<Box<dyn Connection>>,
}

impl Link {
    fn open(document: &Value) -> Result<Self, CommonroomError> {
        let (plugin, connection, hint) = connect_invitation(document)?;
        Ok(Self {
            plugin,
            hint,
            connection: Some(connection),
        })
    }

    fn exchange(
        &mut self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        headers: Option<&[(String, String)]>,
    ) -> Result<(u16, Value), CommonroomError> {
        if self.connection.is_none() {
            self.connection = Some(self.reconnect()?);
        }
        let result = {
            let connection = self.connection.as_mut().expect("connection");
            crate::http_client::http_exchange(connection.as_mut(), method, path, body, headers)
        };
        if let Some(mut connection) = self.connection.take() {
            connection.close();
        }
        result
    }

    fn close(&mut self) {
        if let Some(mut connection) = self.connection.take() {
            connection.close();
        }
        self.plugin.close();
    }

    fn reconnect(&mut self) -> Result<Box<dyn Connection>, CommonroomError> {
        if let Some(url) = self.plugin.local_url() {
            if let Some(port) = url_port(&url) {
                let sock = TcpStream::connect_timeout(
                    &format!("127.0.0.1:{port}").parse().expect("local addr"),
                    Duration::from_secs(5),
                )
                .map_err(|_| ce_protocol::transport_unavailable("connect failed"))?;
                return Ok(Box::new(SocketConnection::new(
                    sock,
                    Duration::from_secs(5),
                )));
            }
        }
        self.plugin.connect(&self.hint)
    }
}

fn url_port(url: &str) -> Option<u16> {
    let rest = url.split("://").nth(1)?;
    let host = rest.split('/').next()?;
    host.rsplit_once(':')?.1.parse().ok()
}

use std::io::BufRead;
