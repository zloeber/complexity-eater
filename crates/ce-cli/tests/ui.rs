use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ce_cli::{start_ui, UiOptions};
use serde_json::{json, Value};

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch() -> TempDir {
    let dir = std::env::temp_dir().join(format!(
        "ce-ui-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

fn http(
    port: u16,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
    body: Option<&str>,
) -> String {
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(cookie) = cookie {
        request.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    if let Some(origin) = origin {
        request.push_str(&format!("Origin: {origin}\r\n"));
    }
    if let Some(body) = body {
        request.push_str("Content-Type: application/json\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
        request.push_str("\r\n");
        request.push_str(body);
    } else {
        request.push_str("\r\n");
    }
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

fn status_of(response: &str) -> u16 {
    response
        .split_whitespace()
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0)
}

fn header<'a>(response: &'a str, name: &str) -> Option<&'a str> {
    for line in response.lines() {
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            if key.eq_ignore_ascii_case(name) {
                return Some(value.trim());
            }
        }
    }
    None
}

fn body_of(response: &str) -> String {
    let raw = response.split("\r\n\r\n").nth(1).unwrap_or("");
    let chunked = header(response, "transfer-encoding")
        .unwrap_or("")
        .eq_ignore_ascii_case("chunked");
    if !chunked {
        return raw.to_string();
    }
    let mut rest = raw;
    let mut out = String::new();
    while let Some((size, after)) = rest.split_once("\r\n") {
        let size = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if size == 0 || after.len() < size {
            break;
        }
        out.push_str(&after[..size]);
        rest = after.get(size + 2..).unwrap_or("");
    }
    out
}

fn script(dir: &std::path::Path, body: &str) -> PathBuf {
    let path = dir.join("cloudflared");
    std::fs::write(&path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
    path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn management_ui_stays_on_loopback_and_redacts_the_tunnel() {
    let dir = scratch();
    match start_ui(UiOptions {
        host: "0.0.0.0".into(),
        port: 0,
        config_path: dir.0.join("connectivity.toml"),
        open_browser: false,
    })
    .await
    {
        Err(err) => assert!(err.message.contains("non-loopback"), "{}", err.message),
        Ok(ui) => {
            ui.shutdown().await;
            panic!("non-loopback bind was accepted");
        }
    }

    let ui = start_ui(UiOptions {
        host: "127.0.0.1".into(),
        port: 0,
        config_path: dir.0.join("connectivity.toml"),
        open_browser: false,
    })
    .await
    .unwrap();
    assert!(ui.local_url.starts_with("http://127.0.0.1:"));
    assert!(ui.local_url.contains("token="));
    let locked = http(ui.port, "GET", "/", None, None, None);
    assert_eq!(status_of(&locked), 200);
    assert!(locked.contains("printed by"));
    assert!(!locked.contains("trycloudflare"));

    let path = ui
        .local_url
        .trim_start_matches("http://")
        .split_once('/')
        .map(|(_, rest)| format!("/{rest}"))
        .unwrap();
    let opened = http(ui.port, "GET", &path, None, None, None);
    assert_eq!(status_of(&opened), 303, "{opened}");
    let cookie = header(&opened, "set-cookie").unwrap().to_string();
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Strict"));
    let again = http(ui.port, "GET", &path, None, None, None);
    assert_eq!(status_of(&again), 401);

    let overview = http(ui.port, "GET", "/api/overview", Some(&cookie), None, None);
    assert_eq!(status_of(&overview), 200, "{overview}");
    let overview_body: Value = serde_json::from_str(&body_of(&overview)).unwrap();
    assert_eq!(overview_body["phase"], "ready_locally");
    assert_eq!(overview_body["collaboration"]["hosted"], "not_implemented");

    let denied = http(
        ui.port,
        "POST",
        "/api/connectivity",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(
            r#"{"mode":"remote","adapter":"cloudflare","confirm":false,"cloudflare":{"enabled":true,"kind":"quick","origin_port":9}}"#,
        ),
    );
    assert_eq!(status_of(&denied), 403, "{denied}");

    let foreign = http(
        ui.port,
        "POST",
        "/api/connectivity/disable",
        Some(&cookie),
        Some("http://evil.example"),
        Some(r#"{"confirm":true}"#),
    );
    assert_eq!(status_of(&foreign), 403, "{foreign}");

    let unknown = http(
        ui.port,
        "POST",
        "/api/connectivity",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(r#"{"mode":"local","adapter":"wireguard","confirm":true}"#),
    );
    assert_eq!(status_of(&unknown), 400, "{unknown}");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap().port();
    let binary = script(
        &dir.0,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"cloudflared version 2026.10.17\"; exit 0; fi\necho \"https://widget-test.trycloudflare.com\"\nexec sleep 30\n",
    );
    let save = json!({
        "mode": "remote",
        "adapter": "cloudflare",
        "confirm": true,
        "cloudflare": {
            "enabled": true,
            "kind": "quick",
            "origin_port": origin,
            "binary": binary.display().to_string()
        }
    });
    let saved = http(
        ui.port,
        "POST",
        "/api/connectivity",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(&save.to_string()),
    );
    assert_eq!(status_of(&saved), 200, "{saved}");

    let started = http(
        ui.port,
        "POST",
        "/api/tunnel/start",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(r#"{"confirm":true}"#),
    );
    assert_eq!(status_of(&started), 200, "{started}");
    let started_body: Value = serde_json::from_str(&body_of(&started)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut seen = started_body["public_url"].as_str().map(str::to_string);
    while seen.is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(30));
        let polled = http(ui.port, "GET", "/api/overview", Some(&cookie), None, None);
        let polled: Value = serde_json::from_str(&body_of(&polled)).unwrap();
        seen = polled["public_url"].as_str().map(str::to_string);
    }
    assert_eq!(
        seen.as_deref(),
        Some("https://widget-test.trycloudflare.com")
    );

    let report = http(
        ui.port,
        "GET",
        "/api/diagnostics",
        Some(&cookie),
        None,
        None,
    );
    let report_body = body_of(&report);
    assert!(!report_body.contains("widget-test"), "{report_body}");
    assert!(report_body.contains("trycloudflare.com"));
    let checks: Value = serde_json::from_str(&report_body).unwrap();
    let text = checks["text"].as_str().unwrap();
    assert!(text.contains("\"authorization\""));
    assert!(text.contains("unsupported"));

    let notes = dir.0.join("notes.md");
    std::fs::write(&notes, "# Notes\n").unwrap();
    let settings = json!({"workspace": notes.display().to_string()});
    let set = http(
        ui.port,
        "POST",
        "/api/settings",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(&settings.to_string()),
    );
    assert_eq!(status_of(&set), 200, "{set}");
    let invited = http(
        ui.port,
        "POST",
        "/api/access/invite",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(r#"{"confirm":true}"#),
    );
    assert_eq!(status_of(&invited), 200, "{invited}");
    let invited: Value = serde_json::from_str(&body_of(&invited)).unwrap();
    let uri = invited["uri"].as_str().unwrap().to_string();
    let invite_id = invited["invite_id"].as_str().unwrap();
    let listed = http(ui.port, "GET", "/api/access", Some(&cookie), None, None);
    assert!(!body_of(&listed).contains(&uri), "{}", body_of(&listed));
    let revoked = http(
        ui.port,
        "POST",
        "/api/access/revoke",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(&json!({"confirm": true, "invite_id": invite_id}).to_string()),
    );
    assert_eq!(status_of(&revoked), 200, "{revoked}");

    let disabled = http(
        ui.port,
        "POST",
        "/api/connectivity/disable",
        Some(&cookie),
        Some(&format!("http://127.0.0.1:{}", ui.port)),
        Some(r#"{"confirm":true}"#),
    );
    assert_eq!(status_of(&disabled), 200, "{disabled}");
    let disabled: Value = serde_json::from_str(&body_of(&disabled)).unwrap();
    assert_eq!(disabled["phase"], "remote_disabled");
    assert!(disabled["public_url"].is_null());
    drop(listener);
    ui.shutdown().await;
}

#[test]
fn cli_prints_the_ui_and_stops_on_term() {
    let help = Command::new(env!("CARGO_BIN_EXE_ce"))
        .arg("ui")
        .arg("--help")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(help.status.success(), "{text}");
    assert!(text.contains("--no-browser"));
    assert!(text.contains("127.0.0.1"));

    let dir = scratch();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ce"))
        .args([
            "ui",
            "--host",
            "127.0.0.1",
            "--port",
            "0",
            "--no-browser",
            "--config",
        ])
        .arg(dir.0.join("connectivity.toml"))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        let mut buf = [0u8; 256];
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline && !text.contains("Management UI:") {
            match stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => text.push_str(&String::from_utf8_lossy(&buf[..n])),
                Err(_) => break,
            }
        }
        text
    });
    let banner = reader.join().unwrap();
    assert!(banner.contains("http://127.0.0.1:"), "{banner}");
    assert!(banner.contains("Ctrl+C"));
    let _ = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let finished = child.wait().unwrap();
    assert!(finished.success(), "{finished}");
}
