use std::io::{BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use ce_protocol::{transport_unavailable, CommonroomError};
use regex::Regex;
use serde_json::{json, Value};

use crate::connection::{Connection, SocketConnection};
use crate::loopback::Transport;

const INSTALL: &str = "https://github.com/tailscale/tailcat#install";

pub fn parse_endpoint(text: &str) -> Option<String> {
    let pattern = Regex::new(r"\b(tc[A-Za-z0-9_-]{16,})\b").ok()?;
    pattern.captures(text).map(|caps| caps[1].to_string())
}

pub fn redact(text: &str) -> String {
    let pattern = Regex::new(r"\b(tc[A-Za-z0-9_-]{16,})\b").expect("address pattern");
    pattern.replace_all(text, "tc…").into_owned()
}

pub struct TailcatTransport {
    binary: String,
    timeout: Duration,
    address: Option<String>,
    port: Option<u16>,
    local_url: Option<String>,
    child: Option<Child>,
    listener: Option<TcpListener>,
    lines: Option<Receiver<String>>,
    readers: Vec<JoinHandle<()>>,
    output: Arc<Mutex<Vec<String>>>,
    drain: Option<JoinHandle<()>>,
    spawned_pid: Option<u32>,
}

impl TailcatTransport {
    pub fn new(binary: Option<String>, timeout_seconds: f64) -> Self {
        let binary = binary.unwrap_or_else(|| {
            std::env::var("CE_TAILCAT")
                .or_else(|_| std::env::var("COMMONROOM_TAILCAT"))
                .unwrap_or_else(|_| "tailcat".into())
        });
        Self {
            binary,
            timeout: Duration::from_secs_f64(timeout_seconds.max(0.05)),
            address: None,
            port: None,
            local_url: None,
            child: None,
            listener: None,
            lines: None,
            readers: Vec::new(),
            output: Arc::new(Mutex::new(Vec::new())),
            drain: None,
            spawned_pid: None,
        }
    }

    fn serve_command(&self, port: u16) -> Vec<String> {
        vec![
            self.binary.clone(),
            "serve".into(),
            "--key=new".into(),
            port.to_string(),
        ]
    }

    fn forward_command(&self, address: &str, local_port: u16, remote_port: u16) -> Vec<String> {
        vec![
            self.binary.clone(),
            "forward".into(),
            address.into(),
            format!("{local_port}:{remote_port}"),
        ]
    }

    fn require_binary(&self) -> Result<(), CommonroomError> {
        let path = Path::new(&self.binary);
        if path.is_absolute() && path.exists() {
            return Ok(());
        }
        if which(&self.binary) {
            return Ok(());
        }
        Err(
            transport_unavailable("transport plugin 'tailcat' is not available")
                .detail("install", INSTALL),
        )
    }

    fn popen(&mut self, command: &[String]) -> Result<(), CommonroomError> {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| transport_unavailable("transport process did not start"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| transport_unavailable("transport process did not start"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| transport_unavailable("transport process did not start"))?;
        // The address is printed on stderr. Both streams feed one queue so a quiet stdout cannot block startup.
        let (tx, rx) = mpsc::channel();
        self.readers.push(pump_lines(stdout, tx.clone()));
        self.readers.push(pump_lines(stderr, tx));
        self.lines = Some(rx);
        self.spawned_pid = Some(child.id());
        self.child = Some(child);
        Ok(())
    }

    fn read_endpoint(&mut self) -> Result<String, CommonroomError> {
        if self.lines.is_none() {
            return Err(transport_unavailable("transport process did not start"));
        }
        let deadline = Instant::now() + self.timeout;
        let mut collected = String::new();
        loop {
            if Instant::now() >= deadline {
                break;
            }
            let slice =
                Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now()));
            if slice.is_zero() {
                break;
            }
            let received = self.lines.as_ref().expect("lines").recv_timeout(slice);
            match received {
                Ok(line) => {
                    let endpoint = parse_endpoint(&line);
                    push_output(&self.output, &line);
                    collected.push_str(&line);
                    if let Some(endpoint) = endpoint {
                        return Ok(endpoint);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if self.child_exited() {
                        if let Some(endpoint) = self.drain_available(&mut collected) {
                            return Ok(endpoint);
                        }
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(endpoint) = self.drain_available(&mut collected) {
                        return Ok(endpoint);
                    }
                    break;
                }
            }
        }
        Err(
            transport_unavailable("transport did not publish an endpoint")
                .detail("install", INSTALL)
                .detail("output", tail(&redact(&collected), 1000)),
        )
    }

    fn drain_available(&self, collected: &mut String) -> Option<String> {
        let rx = self.lines.as_ref()?;
        let mut found = None;
        while let Ok(line) = rx.try_recv() {
            if found.is_none() {
                found = parse_endpoint(&line);
            }
            push_output(&self.output, &line);
            collected.push_str(&line);
        }
        found
    }

    fn child_exited(&mut self) -> bool {
        self.child
            .as_mut()
            .and_then(|child| child.try_wait().ok())
            .flatten()
            .is_some()
    }

    fn start_drain(&mut self) {
        let Some(rx) = self.lines.take() else {
            return;
        };
        let output = Arc::clone(&self.output);
        self.drain = Some(std::thread::spawn(move || {
            while let Ok(line) = rx.recv() {
                push_output(&output, &line);
            }
        }));
    }

    fn start_serve(&mut self, port: u16) -> Result<(), CommonroomError> {
        self.require_binary()?;
        self.port = Some(port);
        *self.output.lock().expect("output") = Vec::new();
        self.popen(&self.serve_command(port))?;
        let started = (|| {
            self.address = Some(self.read_endpoint()?);
            let exited = self
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok())
                .flatten()
                .is_some();
            if self.child.is_none() || exited {
                return Err(
                    transport_unavailable("transport exited during startup").detail(
                        "output",
                        tail(&self.output.lock().expect("output").join(""), 1000),
                    ),
                );
            }
            self.start_drain();
            std::thread::sleep(self.timeout.min(Duration::from_millis(300)));
            let exited = self
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok())
                .flatten()
                .is_some();
            if self.child.is_none() || exited {
                return Err(
                    transport_unavailable("transport exited during startup").detail(
                        "output",
                        tail(&self.output.lock().expect("output").join(""), 1000),
                    ),
                );
            }
            Ok(())
        })();
        if started.is_err() {
            self.close();
        }
        started
    }

    fn stop_and_collect(&mut self) -> String {
        self.shutdown_child();
        self.output.lock().expect("output").join("")
    }

    fn shutdown_child(&mut self) {
        self.lines.take();
        if let Some(mut child) = self.child.take() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        if let Some(drain) = self.drain.take() {
            let _ = drain.join();
        }
    }
}

impl Transport for TailcatTransport {
    fn capabilities(&self) -> std::collections::BTreeSet<&'static str> {
        [
            "direct_peer_connection",
            "relay_support",
            "streaming",
            "bidirectional",
            "encrypted",
            "large_file_transfer",
        ]
        .into_iter()
        .collect()
    }

    fn expose(&mut self, port: u16) -> Result<Value, CommonroomError> {
        self.start_serve(port)?;
        self.describe()
    }

    fn listen(&mut self) -> Result<Value, CommonroomError> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|_| transport_unavailable("listen failed"))?;
        let port = listener
            .local_addr()
            .map_err(|_| transport_unavailable("listen failed"))?
            .port();
        self.listener = Some(listener);
        self.start_serve(port)?;
        self.describe()
    }

    fn accept(&mut self) -> Result<Box<dyn Connection>, CommonroomError> {
        let listener = self
            .listener
            .as_ref()
            .ok_or_else(|| transport_unavailable("listen before accept"))?;
        let _ = listener.set_nonblocking(true);
        let deadline = Instant::now() + self.timeout;
        loop {
            match listener.accept() {
                Ok((sock, _)) => {
                    let _ = sock.set_nonblocking(false);
                    return Ok(Box::new(SocketConnection::new(sock, self.timeout)));
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(transport_unavailable("accept timed out"));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => return Err(transport_unavailable("accept failed")),
            }
        }
    }

    fn connect(&mut self, description: &Value) -> Result<Box<dyn Connection>, CommonroomError> {
        let address = description
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or("");
        let remote_port = description.get("port").and_then(Value::as_u64);
        if !address.starts_with("tc") || remote_port.is_none() {
            return Err(transport_unavailable(
                "transport description is missing an endpoint",
            ));
        }
        let remote_port = remote_port.unwrap() as u16;
        let local_port = ephemeral_port()?;
        self.require_binary()?;
        self.address = Some(address.to_string());
        self.port = Some(remote_port);
        *self.output.lock().expect("output") = Vec::new();
        self.popen(&self.forward_command(address, local_port, remote_port))?;
        self.start_drain();
        if !wait_until_accepts(local_port, self.timeout) {
            let output = self.stop_and_collect();
            return Err(
                transport_unavailable("transport forward did not open a local port")
                    .detail("output", tail(&redact(&output), 1000)),
            );
        }
        self.local_url = Some(format!("http://127.0.0.1:{local_port}"));
        match TcpStream::connect_timeout(
            &format!("127.0.0.1:{local_port}")
                .parse()
                .expect("local addr"),
            self.timeout,
        ) {
            Ok(sock) => Ok(Box::new(SocketConnection::new(sock, self.timeout))),
            Err(_) => {
                self.close();
                Err(transport_unavailable("connect failed"))
            }
        }
    }

    fn close(&mut self) {
        self.listener.take();
        self.shutdown_child();
    }

    fn describe(&self) -> Result<Value, CommonroomError> {
        let address = self
            .address
            .clone()
            .ok_or_else(|| transport_unavailable("transport has no endpoint yet"))?;
        let port = self
            .port
            .ok_or_else(|| transport_unavailable("transport has no endpoint yet"))?;
        Ok(json!({
            "type": "tailcat",
            "address": address,
            "port": port,
            "encrypted": true,
            "mode": "private",
        }))
    }

    fn local_url(&self) -> Option<String> {
        self.local_url.clone()
    }
}

fn pump_lines(stream: impl Read + Send + 'static, tx: mpsc::Sender<String>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if tx.send(std::mem::take(&mut line)).is_err() {
                break;
            }
        }
    })
}

fn push_output(output: &Mutex<Vec<String>>, line: &str) {
    let mut guard = output.lock().expect("output");
    guard.push(redact(line));
    if guard.len() > 40 {
        let drain = guard.len() - 30;
        guard.drain(0..drain);
    }
}

fn tail(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    text.chars().skip(count - max_chars).collect()
}

fn which(binary: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(binary).is_file())
}

fn ephemeral_port() -> Result<u16, CommonroomError> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|_| transport_unavailable("connect failed"))?;
    let port = listener
        .local_addr()
        .map_err(|_| transport_unavailable("connect failed"))?
        .port();
    drop(listener);
    Ok(port)
}

fn wait_until_accepts(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().expect("addr"),
            Duration::from_millis(200),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_binary(dir: &std::path::Path, body: &str) -> String {
        let path = dir.join("tailcat");
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        path.display().to_string()
    }

    #[test]
    fn missing_binary_is_unavailable() {
        let mut plugin = TailcatTransport::new(Some("/no/such/tailcat-binary".into()), 1.0);
        let err = plugin.expose(9).unwrap_err();
        assert_eq!(err.code, "transport_unavailable");
        assert!(err.message.contains("not available"));
    }

    #[test]
    fn exit_after_address_is_not_ready() {
        let dir = std::env::temp_dir().join(format!("ce-tailcat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = fake_binary(
            &dir,
            "#!/bin/sh\necho 'Server listening with new address: tcABCDEFGHIJKLMNOPQRSTUV'\nexit 1\n",
        );
        let mut plugin = TailcatTransport::new(Some(binary), 2.0);
        let err = plugin.expose(9).unwrap_err();
        assert!(
            err.message.contains("exited during startup"),
            "{}",
            err.message
        );
        plugin.close();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn forward_failure_kills_the_child() {
        let dir = std::env::temp_dir().join(format!(
            "ce-tailcat-forward-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = fake_binary(&dir, "#!/bin/sh\nexec sleep 30\n");
        let mut plugin = TailcatTransport::new(Some(binary), 0.4);
        let err = match plugin
            .connect(&json!({"type": "tailcat", "address": "tcABCDEFGHIJKLMNOPQRSTUV", "port": 9}))
        {
            Err(err) => err,
            Ok(_) => panic!("expected forward to fail"),
        };
        assert!(err.message.contains("did not open"), "{}", err.message);
        let pid = plugin.spawned_pid.expect("child pid");
        plugin.close();
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        assert!(!alive, "child {pid} still alive");
        assert!(!redact("noise tcSECRETADDRESSVALUE1234 end").contains("tcSECRETADDRESSVALUE1234"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stderr_address_becomes_ready_without_blocking() {
        let dir = std::env::temp_dir().join(format!("ce-tailcat-stderr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = fake_binary(
            &dir,
            "#!/bin/sh\necho '# Server listening with new address: tcABCDEFGHIJKLMNOPQRSTUV' >&2\nexec sleep 30\n",
        );
        let started = Instant::now();
        let mut plugin = TailcatTransport::new(Some(binary), 2.0);
        let description = plugin.expose(9).expect("stderr address should be enough");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "startup blocked"
        );
        assert_eq!(description["address"], "tcABCDEFGHIJKLMNOPQRSTUV");
        assert_eq!(description["encrypted"], true);
        plugin.close();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn silent_process_stops_at_the_timeout() {
        let dir = std::env::temp_dir().join(format!("ce-tailcat-silent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = fake_binary(&dir, "#!/bin/sh\nexec sleep 30\n");
        let started = Instant::now();
        let mut plugin = TailcatTransport::new(Some(binary), 0.4);
        let err = plugin.expose(9).unwrap_err();
        assert!(err.message.contains("did not publish"), "{}", err.message);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "startup blocked"
        );
        plugin.close();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
