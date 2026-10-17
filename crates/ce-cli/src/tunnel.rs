use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use crate::connectivity::{cloudflared_arguments, loopback_origin, redact_text, CloudflareFile};

const URL_PATTERN: &str = r"https://[a-z0-9-]+\.trycloudflare\.com";

#[derive(Clone, Debug)]
struct TunnelLog {
    lines: Vec<String>,
    public_url: Option<String>,
    origin: String,
    kind: String,
    exit_code: Option<i32>,
}

pub struct TunnelProcess {
    child: Option<Child>,
    readers: Vec<JoinHandle<()>>,
    log: Arc<Mutex<TunnelLog>>,
}

#[derive(Clone, Debug)]
pub struct TunnelStatus {
    pub running: bool,
    pub origin: String,
    pub kind: String,
    pub public_url: Option<String>,
    pub exit_code: Option<i32>,
    pub lines: Vec<String>,
}

impl TunnelProcess {
    pub fn new() -> Self {
        Self {
            child: None,
            readers: Vec::new(),
            log: Arc::new(Mutex::new(TunnelLog {
                lines: Vec::new(),
                public_url: None,
                origin: String::new(),
                kind: String::new(),
                exit_code: None,
            })),
        }
    }

    pub fn start(&mut self, binary: &Path, file: &CloudflareFile) -> Result<(), String> {
        if self.alive() {
            return Err("the tunnel process is already running".into());
        }
        self.stop();
        let args = cloudflared_arguments(file).map_err(|err| err.message)?;
        if args.iter().any(|arg| arg.contains("--token")) {
            return Err("refusing to pass a tunnel token".into());
        }
        let mut child = Command::new(binary)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("cloudflared did not start: {err}"))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        {
            let mut log = self.log.lock().expect("tunnel log");
            *log = TunnelLog {
                lines: Vec::new(),
                public_url: None,
                origin: loopback_origin(file.origin_port),
                kind: file.kind.clone(),
                exit_code: None,
            };
        }
        let mut readers = Vec::new();
        if let Some(stderr) = stderr {
            let log = Arc::clone(&self.log);
            readers.push(std::thread::spawn(move || {
                read_lines(BufReader::new(stderr), &log);
            }));
        }
        if let Some(stdout) = stdout {
            let log = Arc::clone(&self.log);
            readers.push(std::thread::spawn(move || {
                read_lines(BufReader::new(stdout), &log);
            }));
        }
        self.child = Some(child);
        self.readers = readers;
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let status = child.wait();
            if let Ok(status) = status {
                self.log.lock().expect("tunnel log").exit_code = status.code();
            }
        }
        self.reap_readers();
    }

    pub fn alive(&mut self) -> bool {
        let status = match self.child.as_mut() {
            Some(child) => child.try_wait(),
            None => return false,
        };
        match status {
            Ok(Some(status)) => {
                self.log.lock().expect("tunnel log").exit_code = status.code();
                // The child can exit before the reader threads copy the last lines.
                self.reap_readers();
                false
            }
            Ok(None) => true,
            Err(_) => false,
        }
    }

    fn reap_readers(&mut self) {
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }

    pub fn status(&mut self) -> TunnelStatus {
        let running = self.alive();
        let log = self.log.lock().expect("tunnel log");
        TunnelStatus {
            running,
            origin: log.origin.clone(),
            kind: log.kind.clone(),
            public_url: log.public_url.clone(),
            exit_code: log.exit_code,
            lines: log.lines.clone(),
        }
    }
}

impl Drop for TunnelProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

fn tunnel_url(line: &str) -> Option<String> {
    static PATTERN: OnceLock<regex::Regex> = OnceLock::new();
    let pattern =
        PATTERN.get_or_init(|| regex::Regex::new(URL_PATTERN).expect("tunnel url pattern"));
    pattern.find(line).map(|item| item.as_str().to_string())
}

fn read_lines(reader: impl BufRead, log: &Mutex<TunnelLog>) {
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        let redacted = redact_text(&line);
        let found = tunnel_url(&line);
        let mut log = log.lock().expect("tunnel log");
        if log.lines.len() >= 40 {
            log.lines.remove(0);
        }
        log.lines.push(redacted);
        if log.public_url.is_none() {
            log.public_url = found;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::connectivity::CloudflareFile;

    fn wait_for_url(process: &mut TunnelProcess, timeout: Duration) -> Option<String> {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if let Some(url) = process.status().public_url.clone() {
                return Some(url);
            }
            if !process.alive() {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        process.status().public_url
    }

    fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
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

    #[test]
    fn a_fake_cloudflared_prints_a_url_and_stop_reaps_it() {
        let dir = std::env::temp_dir().join(format!(
            "ce-tunnel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("log");
        let binary = script(
            &dir,
            "cloudflared",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\necho \"https://widget-test.trycloudflare.com\"\nexec sleep 30\n",
                log.display()
            ),
        );
        let file = CloudflareFile {
            enabled: true,
            kind: "quick".into(),
            origin_port: 43111,
            ..CloudflareFile::default()
        };
        let mut process = TunnelProcess::new();
        process.start(&binary, &file).unwrap();
        let url = wait_for_url(&mut process, Duration::from_secs(2));
        assert_eq!(
            url.as_deref(),
            Some("https://widget-test.trycloudflare.com")
        );
        let status = process.status();
        assert!(status.running);
        assert_eq!(status.origin, "http://127.0.0.1:43111");
        assert!(status
            .lines
            .iter()
            .any(|line| line.contains("….trycloudflare.com")));
        assert!(!status.lines.iter().any(|line| line.contains("widget-test")));
        process.stop();
        assert!(!process.status().running);
        let recorded = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(recorded.contains("http://127.0.0.1:43111"), "{recorded}");
        assert!(!recorded.contains("--token"), "{recorded}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failing_cloudflared_does_not_keep_a_secret_in_the_log() {
        let dir = std::env::temp_dir().join(format!("ce-tunnel-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = script(
            &dir,
            "cloudflared",
            "#!/bin/sh\necho \"token=super-secret-value\" >&2\nexit 2\n",
        );
        let file = CloudflareFile {
            enabled: true,
            kind: "quick".into(),
            origin_port: 43112,
            ..CloudflareFile::default()
        };
        let mut process = TunnelProcess::new();
        process.start(&binary, &file).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while process.alive() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let status = process.status();
        assert!(!status.running);
        let joined = status.lines.join("\n");
        assert!(!joined.contains("super-secret-value"), "{joined}");
        assert!(joined.contains("token=…"), "{joined}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
