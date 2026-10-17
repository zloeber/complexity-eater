use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ce_protocol::decode_invitation;
use serde_json::Value;

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn workspace() -> (TempDir, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "ce-drop-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|stamp| stamp.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("note.md");
    std::fs::write(&path, "# Note\n\nSECRET-DROP-BODY\n").unwrap();
    (TempDir(dir), path)
}

struct Watch {
    child: std::process::Child,
    stdout: BufReader<std::process::ChildStdout>,
    stderr: Arc<Mutex<String>>,
}

impl Watch {
    fn spawn(path: &std::path::Path, extra: &[&str]) -> Self {
        let mut args = vec![
            "drop",
            path.to_str().unwrap(),
            "--name",
            "Host",
            "--idle",
            "0",
        ];
        args.extend_from_slice(extra);
        let mut child = Command::new(env!("CARGO_BIN_EXE_ce"))
            .args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let stderr = child.stderr.take().unwrap();
        let err_buf = Arc::new(Mutex::new(String::new()));
        let stored = Arc::clone(&err_buf);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                stored.lock().unwrap().push_str(&line);
                line.clear();
            }
        });
        Self {
            child,
            stdout,
            stderr: err_buf,
        }
    }

    fn line(&mut self) -> Value {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).unwrap();
        assert!(
            read > 0,
            "drop stdout closed: {}",
            self.stderr.lock().unwrap()
        );
        serde_json::from_str(line.trim()).unwrap_or_else(|err| {
            panic!(
                "stdout was not json ({err}): {line} stderr {}",
                self.stderr.lock().unwrap()
            )
        })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn pid_alive(pid: i32) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn wait_until(mut ready: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if ready() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

fn http(
    address: &str,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = body.unwrap_or("");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if bearer.is_some() || body.starts_with('{') {
        request.push_str("Content-Type: application/json\r\n");
    }
    if let Some(bearer) = bearer {
        request.push_str(&format!("Authorization: Bearer {bearer}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, payload) = response.split_once("\r\n\r\n").unwrap_or(("", ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    (status, payload.to_string())
}

#[test]
fn drop_close_without_a_room_reports_that_nothing_is_running() {
    let output = Command::new(env!("CARGO_BIN_EXE_ce"))
        .args(["drop", "close", "--workspace", "missing-note.md"])
        .output()
        .unwrap();
    assert_ne!(output.status.code(), Some(0));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("no drop is running"),
        "stdout {text} stderr {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_pickup_leaves_the_child_running_until_drop_close() {
    let (_dir, path) = workspace();
    let mut watch = Watch::spawn(&path, &["--pickups", "1", "--for", "60"]);
    let started = watch.line();
    assert_eq!(started["event"], "started");
    let invite = started["invite"].as_str().unwrap();
    assert!(invite.starts_with("commonroom://join/"));
    assert!(started["human"]
        .as_str()
        .unwrap()
        .starts_with("http://127.0.0.1:"));
    assert!(
        wait_until(|| watch.stderr.lock().unwrap().contains("Human:")),
        "banner missing: {}",
        watch.stderr.lock().unwrap()
    );
    let banner = watch.stderr.lock().unwrap().clone();
    assert!(banner.contains("Invite:"), "{banner}");
    let room_pid = started["pid"].as_i64().unwrap() as i32;
    assert!(pid_alive(room_pid));

    let document = decode_invitation(invite).unwrap();
    let address = format!(
        "{}:{}",
        document["transport"]["address"].as_str().unwrap(),
        document["transport"]["port"].as_u64().unwrap()
    );
    let secret = document["secret"].as_str().unwrap();
    let body = serde_json::json!({
        "invite": secret,
        "name": "Ada",
        "kind": "agent",
        "transport": "loopback"
    })
    .to_string();
    let (status, joined) = http(&address, "POST", "/session/join", None, Some(&body));
    assert_eq!(status, 200, "{joined}");
    let joined: Value = serde_json::from_str(&joined).unwrap();
    let session = joined["session_id"].as_str().unwrap();
    let (status, body) = http(&address, "GET", "/workspace/read", Some(session), None);
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("SECRET-DROP-BODY"));

    let picked = watch.line();
    assert_eq!(picked["event"], "picked_up");
    assert_eq!(picked["participant"], "Ada");
    assert_eq!(picked["pickups"], 1);
    assert_eq!(picked["pickups_remaining"], 0);
    assert!(picked.get("invite").is_none());
    assert!(!picked.to_string().contains("SECRET-DROP-BODY"));
    assert!(!picked.to_string().contains("commonroom://join/"));
    let status = watch.child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    assert!(pid_alive(room_pid), "child exited with the parent");

    let close = Command::new(env!("CARGO_BIN_EXE_ce"))
        .args(["drop", "close", "--workspace", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        close.status.success(),
        "{}",
        String::from_utf8_lossy(&close.stdout)
    );
    assert!(
        wait_until(|| !pid_alive(room_pid)),
        "drop close did not stop the child"
    );
    let pid_path = path.with_file_name("note.md.commonroom.drop.pid");
    assert!(
        wait_until(|| !pid_path.exists()),
        "pid file was left behind"
    );
}

#[test]
fn drop_unread_deadline_exits_the_child_and_the_parent_returns_2() {
    let (_dir, path) = workspace();
    let mut watch = Watch::spawn(&path, &["--for", "60"]);
    let started = watch.line();
    assert_eq!(started["event"], "started");
    let db = path.with_file_name("note.md.commonroom.db");
    assert!(wait_until(|| db.exists()), "sidecar was not created");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.busy_timeout(Duration::from_secs(3)).unwrap();
    conn.execute(
        "UPDATE drop_state SET deadline = ?1",
        ["2000-01-01T00:00:00.000000+00:00"],
    )
    .unwrap();
    let expired = watch.line();
    assert_eq!(expired, serde_json::json!({"event": "expired"}));
    let status = watch.child.wait().unwrap();
    assert_eq!(status.code(), Some(2));
    let room_pid = started["pid"].as_i64().unwrap() as i32;
    assert!(
        wait_until(|| !pid_alive(room_pid)),
        "expired drop left the child running"
    );
}

#[test]
fn drop_close_before_pickup_prints_closed_and_exits_3() {
    let (_dir, path) = workspace();
    let mut watch = Watch::spawn(&path, &["--for", "60"]);
    let started = watch.line();
    assert_eq!(started["event"], "started");
    let close = Command::new(env!("CARGO_BIN_EXE_ce"))
        .args(["drop", "close", "--workspace", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        close.status.success(),
        "{}",
        String::from_utf8_lossy(&close.stdout)
    );
    let closed = watch.line();
    assert_eq!(closed["event"], "closed");
    assert!(closed.get("invite").is_none());
    let status = watch.child.wait().unwrap();
    assert_eq!(status.code(), Some(3));
}

#[test]
fn drop_parent_death_before_pickup_stops_the_child() {
    let (_dir, path) = workspace();
    let mut watch = Watch::spawn(&path, &["--for", "60"]);
    let started = watch.line();
    let room_pid = started["pid"].as_i64().unwrap() as i32;
    assert!(pid_alive(room_pid));
    watch.child.kill().unwrap();
    let _ = watch.child.wait();
    assert!(
        wait_until(|| !pid_alive(room_pid)),
        "child stayed up after the parent died"
    );
}
