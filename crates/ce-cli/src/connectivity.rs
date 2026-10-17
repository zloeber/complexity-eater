use std::path::{Path, PathBuf};
use std::time::Duration;

use ce_protocol::{invalid_request, permission_denied, CommonroomError};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const MODE_LOCAL: &str = "local";
pub const MODE_HOSTED: &str = "hosted";
pub const MODE_REMOTE: &str = "remote";

pub const ADAPTER_NONE: &str = "none";
pub const ADAPTER_CLOUDFLARE: &str = "cloudflare";
pub const ADAPTER_PRIVATE: &str = "private";
pub const ADAPTER_CUSTOM: &str = "custom";

const SECRET_KEYS: &[&str] = &[
    "token",
    "api_token",
    "secret",
    "password",
    "authorization",
    "credential",
    "account_key",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Connectivity {
    pub mode: String,
    pub adapter: String,
    pub workspace: String,
    pub hosted_url: String,
    pub cloudflare: CloudflareFile,
    pub private_network: PrivateFile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CloudflareFile {
    pub enabled: bool,
    pub kind: String,
    pub tunnel_name: String,
    pub hostname: String,
    pub origin_port: u16,
    pub binary: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PrivateFile {
    pub enabled: bool,
    pub binary: String,
}

impl Default for Connectivity {
    fn default() -> Self {
        Self {
            mode: MODE_LOCAL.into(),
            adapter: ADAPTER_NONE.into(),
            workspace: String::new(),
            hosted_url: String::new(),
            cloudflare: CloudflareFile::default(),
            private_network: PrivateFile::default(),
        }
    }
}

impl Default for CloudflareFile {
    fn default() -> Self {
        Self {
            enabled: false,
            kind: "quick".into(),
            tunnel_name: String::new(),
            hostname: String::new(),
            origin_port: 0,
            binary: String::new(),
        }
    }
}

impl Connectivity {
    pub fn cloudflare_binary(&self) -> &str {
        if self.cloudflare.binary.is_empty() {
            "cloudflared"
        } else {
            self.cloudflare.binary.as_str()
        }
    }

    pub fn private_binary(&self) -> &str {
        if self.private_network.binary.is_empty() {
            "tailcat"
        } else {
            self.private_network.binary.as_str()
        }
    }
}

pub fn load_connectivity(path: &Path) -> Result<Connectivity, CommonroomError> {
    if !path.is_file() {
        return Ok(Connectivity::default());
    }
    let text = std::fs::read_to_string(path).map_err(|err| {
        invalid_request(format!(
            "cannot read connectivity config {}: {err}",
            path.display()
        ))
    })?;
    parse_connectivity(&text)
}

pub fn parse_connectivity(text: &str) -> Result<Connectivity, CommonroomError> {
    let raw: toml::Value = toml::from_str(text)
        .map_err(|err| invalid_request(format!("invalid connectivity config: {err}")))?;
    reject_secret_keys(&raw)?;
    let parsed: Connectivity = toml::from_str(text)
        .map_err(|err| invalid_request(format!("invalid connectivity config: {err}")))?;
    validate_shape(&parsed)?;
    Ok(parsed)
}

pub fn save_connectivity(path: &Path, value: &Connectivity) -> Result<(), CommonroomError> {
    validate_shape(value)?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| {
                invalid_request(format!("cannot create {}: {err}", parent.display()))
            })?;
        }
    }
    let text = toml::to_string_pretty(value)
        .map_err(|err| invalid_request(format!("cannot write connectivity config: {err}")))?;
    write_private(path, text.as_bytes())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), CommonroomError> {
    let tmp = path.with_extension("toml.part");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&tmp)
            .map_err(|err| invalid_request(format!("cannot write {}: {err}", tmp.display())))?;
        use std::io::Write;
        file.write_all(bytes)
            .map_err(|err| invalid_request(format!("cannot write {}: {err}", tmp.display())))?;
    }
    std::fs::rename(&tmp, path)
        .map_err(|err| invalid_request(format!("cannot replace {}: {err}", path.display())))?;
    Ok(())
}

fn reject_secret_keys(value: &toml::Value) -> Result<(), CommonroomError> {
    match value {
        toml::Value::Table(table) => {
            for (key, child) in table {
                if SECRET_KEYS
                    .iter()
                    .any(|secret| key.eq_ignore_ascii_case(secret))
                {
                    return Err(invalid_request(
                        "connectivity config cannot store credentials. Keep Cloudflare certificates in cloudflared's own directory",
                    ));
                }
                reject_secret_keys(child)?;
            }
        }
        toml::Value::Array(items) => {
            for item in items {
                reject_secret_keys(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn validate_shape(value: &Connectivity) -> Result<(), CommonroomError> {
    if !matches!(value.mode.as_str(), MODE_LOCAL | MODE_HOSTED | MODE_REMOTE) {
        return Err(invalid_request("mode must be local, hosted, or remote"));
    }
    if !matches!(
        value.adapter.as_str(),
        ADAPTER_NONE | ADAPTER_CLOUDFLARE | ADAPTER_PRIVATE | ADAPTER_CUSTOM
    ) {
        return Err(invalid_request(format!(
            "unsupported adapter '{}'",
            value.adapter
        )));
    }
    if value.adapter == ADAPTER_CUSTOM {
        return Err(invalid_request(
            "a custom HTTPS endpoint is not available. The room process accepts only loopback binds",
        ));
    }
    if !value.hosted_url.is_empty() {
        validate_hosted_url(&value.hosted_url)?;
    }
    if !value.workspace.is_empty() {
        validate_workspace_path(&value.workspace)?;
    }
    validate_cloudflare(&value.cloudflare)?;
    validate_binary_field(&value.private_network.binary)?;
    if value.mode == MODE_REMOTE && value.adapter == ADAPTER_NONE {
        return Err(invalid_request(
            "remote mode needs a tunnel or private-network adapter",
        ));
    }
    if value.cloudflare.enabled && value.adapter != ADAPTER_CLOUDFLARE {
        return Err(invalid_request(
            "the tunnel stays disabled unless the Cloudflare adapter is selected",
        ));
    }
    if value.private_network.enabled && value.adapter != ADAPTER_PRIVATE {
        return Err(invalid_request(
            "private networking stays disabled unless that adapter is selected",
        ));
    }
    Ok(())
}

fn validate_cloudflare(file: &CloudflareFile) -> Result<(), CommonroomError> {
    if file.kind != "quick" && file.kind != "existing" {
        return Err(invalid_request("cloudflare kind must be quick or existing"));
    }
    validate_binary_field(&file.binary)?;
    if !file.tunnel_name.is_empty() && !tunnel_name_ok(&file.tunnel_name) {
        return Err(invalid_request(
            "tunnel name must be letters, numbers, hyphens, or underscores",
        ));
    }
    if !file.hostname.is_empty() && !hostname_ok(&file.hostname) {
        return Err(invalid_request(
            "hostname must be a DNS name without a scheme, port, or path",
        ));
    }
    if file.kind == "existing" && file.enabled && file.tunnel_name.is_empty() {
        return Err(invalid_request(
            "an existing tunnel needs the tunnel name cloudflared already created",
        ));
    }
    if file.enabled && file.origin_port == 0 {
        return Err(invalid_request(
            "set the room's session ingress port before enabling the tunnel. The management UI port is not an origin",
        ));
    }
    Ok(())
}

pub fn validate_origin(port: u16, ui_port: u16) -> Result<(), CommonroomError> {
    if port == 0 {
        return Err(invalid_request(
            "set the room's session ingress port. The management UI is not the origin",
        ));
    }
    if port == ui_port {
        return Err(invalid_request(
            "refusing to publish the management UI. Point the tunnel at the room session ingress",
        ));
    }
    Ok(())
}

pub fn loopback_origin(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn cloudflared_arguments(file: &CloudflareFile) -> Result<Vec<String>, CommonroomError> {
    validate_cloudflare(file)?;
    if file.origin_port == 0 {
        return Err(invalid_request(
            "set the room's session ingress port before starting the tunnel",
        ));
    }
    let origin = loopback_origin(file.origin_port);
    let mut args = vec!["tunnel".to_string()];
    if file.kind == "existing" {
        if file.tunnel_name.is_empty() {
            return Err(invalid_request(
                "an existing tunnel needs the tunnel name cloudflared already created",
            ));
        }
        args.push("run".into());
        args.push("--url".into());
        args.push(origin);
        args.push(file.tunnel_name.clone());
    } else {
        args.push("--url".into());
        args.push(origin);
        args.push("--no-autoupdate".into());
    }
    if args
        .iter()
        .any(|arg| arg == "--token" || arg.starts_with("--token="))
    {
        return Err(invalid_request("refusing to pass a tunnel token"));
    }
    Ok(args)
}

fn validate_binary_field(value: &str) -> Result<(), CommonroomError> {
    if value.is_empty() {
        return Ok(());
    }
    if value.chars().any(|ch| {
        ch.is_whitespace() || matches!(ch, '"' | '\'' | '$' | ';' | '|' | '&' | '`' | '\\' | '\n')
    }) {
        return Err(invalid_request(
            "the executable must be a single path, not a shell command",
        ));
    }
    if value.contains("..") {
        return Err(invalid_request("the executable path cannot contain '..'"));
    }
    Ok(())
}

fn tunnel_name_ok(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && value.len() <= 63
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn hostname_ok(value: &str) -> bool {
    if value.len() > 253
        || value.contains("://")
        || value.contains('/')
        || value.contains(':')
        || value.contains('@')
        || value.contains('?')
        || value.contains('#')
        || value.contains('_')
    {
        return false;
    }
    let labels: Vec<&str> = value.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        })
}

fn validate_hosted_url(value: &str) -> Result<(), CommonroomError> {
    let Some(rest) = value.strip_prefix("https://") else {
        return Err(invalid_request("a hosted URL must start with https://"));
    };
    if rest.contains('@') || rest.contains('?') || rest.contains('#') || rest.contains('\\') {
        return Err(invalid_request(
            "a hosted URL cannot carry user info, a query, or a fragment",
        ));
    }
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !path.is_empty() {
        return Err(invalid_request("a hosted URL cannot include a path"));
    }
    let host = host_port
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(host_port);
    let host = host.trim_matches(|ch| ch == '[' || ch == ']');
    if host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host.ends_with(".local")
    {
        return Err(invalid_request(
            "a hosted URL is not a loopback address. Hosted coordination is not implemented",
        ));
    }
    if !hostname_ok(host) && !host.parse::<std::net::Ipv4Addr>().is_ok() {
        return Err(invalid_request("hosted URL host is not a DNS name"));
    }
    Ok(())
}

fn validate_workspace_path(value: &str) -> Result<(), CommonroomError> {
    if value.chars().any(|ch| ch == '\0' || ch == '\n') {
        return Err(invalid_request("workspace path is invalid"));
    }
    let path = Path::new(value);
    if !path.is_absolute() {
        return Err(invalid_request("workspace path must be absolute"));
    }
    if !path.is_file() {
        return Err(invalid_request(
            "workspace path must be an existing markdown file",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct ConnectivityUpdate {
    pub mode: String,
    pub adapter: String,
    pub hosted_url: String,
    pub workspace: String,
    pub cloudflare: CloudflareFile,
    pub private_network: PrivateFile,
    pub confirm: bool,
}

impl ConnectivityUpdate {
    pub fn from_json(value: &Value) -> Result<Self, CommonroomError> {
        if !value.is_object() {
            return Err(invalid_request("body must be a JSON object"));
        }
        for key in SECRET_KEYS {
            if value.get(*key).is_some() {
                return Err(invalid_request(
                    "this API does not accept credentials. Configure cloudflared outside this page",
                ));
            }
        }
        let cloudflare = value
            .get("cloudflare")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let private_network = value
            .get("private_network")
            .cloned()
            .unwrap_or_else(|| json!({}));
        Ok(Self {
            mode: string_field(value, "mode").unwrap_or_else(|| MODE_LOCAL.into()),
            adapter: string_field(value, "adapter").unwrap_or_else(|| ADAPTER_NONE.into()),
            hosted_url: string_field(value, "hosted_url").unwrap_or_default(),
            workspace: string_field(value, "workspace").unwrap_or_default(),
            confirm: value
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            cloudflare: CloudflareFile {
                enabled: cloudflare
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                kind: string_field(&cloudflare, "kind").unwrap_or_else(|| "quick".into()),
                tunnel_name: string_field(&cloudflare, "tunnel_name").unwrap_or_default(),
                hostname: string_field(&cloudflare, "hostname").unwrap_or_default(),
                origin_port: cloudflare
                    .get("origin_port")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u16,
                binary: string_field(&cloudflare, "binary").unwrap_or_default(),
            },
            private_network: PrivateFile {
                enabled: private_network
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                binary: string_field(&private_network, "binary").unwrap_or_default(),
            },
        })
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_string)
}

pub fn apply_update(
    update: ConnectivityUpdate,
    ui_port: u16,
) -> Result<Connectivity, CommonroomError> {
    let exposing = update.mode == MODE_REMOTE
        || update.adapter == ADAPTER_CLOUDFLARE
        || update.adapter == ADAPTER_PRIVATE
        || update.cloudflare.enabled
        || update.private_network.enabled;
    if exposing && !update.confirm {
        return Err(permission_denied(
            "explicit confirmation is required before exposing this machine or selecting a remote adapter",
        ));
    }
    if update.cloudflare.enabled {
        validate_origin(update.cloudflare.origin_port, ui_port)?;
    }
    let next = Connectivity {
        mode: update.mode,
        adapter: update.adapter,
        workspace: update.workspace,
        hosted_url: update.hosted_url,
        cloudflare: update.cloudflare,
        private_network: update.private_network,
    };
    validate_shape(&next)?;
    Ok(next)
}

pub fn disable_remote(current: Connectivity) -> Connectivity {
    let mut next = current;
    next.mode = MODE_LOCAL.into();
    next.adapter = ADAPTER_NONE.into();
    next.cloudflare.enabled = false;
    next.private_network.enabled = false;
    next
}

pub fn redact_text(input: &str) -> String {
    let mut text = ce_transport::redact(input);
    text = replace_pattern(
        &text,
        r"commonroom://join/[A-Za-z0-9_-]+",
        "commonroom://join/…",
    );
    text = replace_pattern(&text, r"(?i)bearer\s+[A-Za-z0-9._~+/-]+=*", "Bearer …");
    text = replace_pattern(&text, r"\bses_[A-Za-z0-9]+\b", "ses_…");
    text = replace_pattern(
        &text,
        r"eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}(?:\.[A-Za-z0-9_-]+)?",
        "…",
    );
    text = replace_pattern(
        &text,
        r"https://[a-z0-9-]+\.trycloudflare\.com",
        "https://….trycloudflare.com",
    );
    text = replace_pattern(
        &text,
        r"(?i)\b(api[_-]?token|authorization|secret|password|token)\b\s*[:=]\s*\S+",
        "$1=…",
    );
    text
}

fn replace_pattern(input: &str, pattern: &str, replacement: &str) -> String {
    match regex::Regex::new(pattern) {
        Ok(pattern) => pattern.replace_all(input, replacement).into_owned(),
        Err(_) => input.to_string(),
    }
}

pub fn executable_on_path(name: &str, path_var: Option<&str>) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        let path = PathBuf::from(name);
        return (path.is_absolute() && path.is_file()).then_some(path);
    }
    let raw = path_var
        .map(str::to_string)
        .or_else(|| std::env::var("PATH").ok())?;
    for dir in std::env::split_paths(&raw) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn command_version(binary: &Path) -> Result<String, String> {
    let child = std::process::Command::new(binary)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let output = match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => return Err(err.to_string()),
        Err(_) => {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
            return Err("version command timed out".into());
        }
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let text = redact_text(&text);
    if !output.status.success() {
        return Err(text
            .lines()
            .next()
            .unwrap_or("version command failed")
            .into());
    }
    let pattern = regex::Regex::new(r"version\s+([0-9][0-9A-Za-z._-]*)").ok();
    if let Some(pattern) = pattern {
        if let Some(found) = pattern.captures(&text) {
            return Ok(found[1].to_string());
        }
    }
    Ok(text.lines().next().unwrap_or("unknown").trim().to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub id: &'static str,
    pub state: &'static str,
    pub detail: String,
}

impl Check {
    fn new(id: &'static str, state: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            state,
            detail: redact_text(&detail.into()),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "state": self.state,
            "detail": self.detail,
        })
    }
}

pub struct RuntimeFacts {
    pub ui_listening: bool,
    pub adapter_process: &'static str,
    pub public_url_seen: bool,
    pub process_exit: Option<i32>,
    pub cloudflared_installed: bool,
    pub cloudflared_version: Option<String>,
    pub private_installed: bool,
    pub origin_accepts: bool,
    pub edge_tcp: Option<bool>,
}

pub fn health_checks(config: &Connectivity, facts: &RuntimeFacts) -> Vec<Check> {
    let mut checks = vec![
        Check::new(
            "local_process",
            if facts.ui_listening { "pass" } else { "fail" },
            if facts.ui_listening {
                "The management process is running on this machine."
            } else {
                "The management process is not running."
            },
        ),
        Check::new(
            "local_api",
            if facts.ui_listening { "pass" } else { "fail" },
            "The management API answered on the loopback listener.",
        ),
    ];
    checks.push(adapter_check(config, facts));
    checks.push(endpoint_check(config, facts));
    checks.push(Check::new(
        "tls",
        "not_checked",
        "This binary does not verify certificates. A TCP connection, or a URL printed by cloudflared, is not TLS verification.",
    ));
    checks.push(Check::new(
        "authentication",
        "unsupported",
        if config.mode == MODE_HOSTED {
            "Hosted coordination is not implemented, so there is no service account to authenticate."
        } else {
            "Tunnel and plugin connectivity do not authenticate a workspace session."
        },
    ));
    checks.push(Check::new(
        "authorization",
        "unsupported",
        "A reachable port does not grant workspace access. An agent needs an invitation, and the session is revocable.",
    ));
    checks.push(Check::new(
        "collaboration",
        if config.mode == MODE_HOSTED {
            "not_implemented"
        } else {
            "supported_locally"
        },
        if config.mode == MODE_HOSTED {
            "Document review, questions, and handoffs are not available through a hosted service. No hosted backend is implemented."
        } else {
            "Document review, anchored questions, and session handoff run in the local room process. They do not require a tunnel plugin."
        },
    ));
    checks
}

fn adapter_check(config: &Connectivity, facts: &RuntimeFacts) -> Check {
    match config.adapter.as_str() {
        ADAPTER_CLOUDFLARE => match facts.adapter_process {
            "running" => Check::new(
                "adapter",
                "pass",
                "cloudflared is a child of this management process. It is not an operating-system service.",
            ),
            "starting" => Check::new("adapter", "starting", "cloudflared is starting."),
            "exited" => Check::new(
                "adapter",
                "fail",
                format!(
                    "cloudflared exited{}. The room file and sidecar were not deleted.",
                    facts
                        .process_exit
                        .map(|code| format!(" with status {code}"))
                        .unwrap_or_default()
                ),
            ),
            _ if !facts.cloudflared_installed => Check::new(
                "adapter",
                "fail",
                "cloudflared is not installed. Install it yourself, then return here. This page will not install it.",
            ),
            _ if !config.cloudflare.enabled => Check::new(
                "adapter",
                "skipped",
                "The Cloudflare adapter is selected and not enabled. Nothing has been exposed.",
            ),
            _ => Check::new(
                "adapter",
                "skipped",
                "cloudflared is installed and the tunnel has not been started.",
            ),
        },
        ADAPTER_PRIVATE => {
            if !facts.private_installed {
                Check::new(
                    "adapter",
                    "fail",
                    "The private-network executable was not found on PATH. This page does not install or start it.",
                )
            } else if config.private_network.enabled {
                Check::new(
                    "adapter",
                    "skipped",
                    "The preference is saved. Start the room with the plugin flag. This page does not launch the plugin.",
                )
            } else {
                Check::new(
                    "adapter",
                    "skipped",
                    "Private networking is not enabled.",
                )
            }
        }
        ADAPTER_CUSTOM => Check::new(
            "adapter",
            "unsupported",
            "A custom endpoint is not implemented.",
        ),
        _ => Check::new(
            "adapter",
            "skipped",
            "No remote adapter is selected. The room stays local when you start it.",
        ),
    }
}

fn endpoint_check(config: &Connectivity, facts: &RuntimeFacts) -> Check {
    if config.mode == MODE_HOSTED {
        return Check::new(
            "remote_endpoint",
            "not_implemented",
            "There is no hosted coordination endpoint to reach.",
        );
    }
    if config.adapter != ADAPTER_CLOUDFLARE || !config.cloudflare.enabled {
        return Check::new(
            "remote_endpoint",
            "skipped",
            "Remote reachability does not apply while the tunnel is disabled. Loopback is not another machine.",
        );
    }
    if !facts.origin_accepts {
        return Check::new(
            "remote_endpoint",
            "fail",
            "Nothing accepted a connection on the loopback origin. Start the room before starting the tunnel.",
        );
    }
    if facts.public_url_seen {
        let edge = match facts.edge_tcp {
            Some(true) => " A TCP connection to port 443 completed. That is not TLS verification and not workspace authorization.",
            Some(false) => " A TCP connection to port 443 failed.",
            None => " External TCP was not probed.",
        };
        return Check::new(
            "remote_endpoint",
            "pass",
            format!("cloudflared printed a public URL.{edge} The URL is redacted in diagnostics."),
        );
    }
    if config.cloudflare.kind == "existing" && facts.adapter_process == "running" {
        return Check::new(
            "remote_endpoint",
            "degraded",
            "The named tunnel process is running. This page cannot see whether Cloudflare has registered the hostname. DNS and Access stay in your Cloudflare account.",
        );
    }
    Check::new(
        "remote_endpoint",
        "fail",
        "The tunnel process has not reported a reachable hostname.",
    )
}

pub fn phase(
    config: &Connectivity,
    facts: &RuntimeFacts,
) -> (&'static str, &'static str, &'static str) {
    if config.mode == MODE_HOSTED {
        return (
            "unsupported",
            "Unsupported",
            "Hosted coordination is not implemented. The workspace file stays on this machine.",
        );
    }
    if config.adapter == ADAPTER_CUSTOM {
        return (
            "unsupported",
            "Unsupported",
            "A custom endpoint is not available in this version.",
        );
    }
    if facts.adapter_process == "starting"
        || (facts.adapter_process == "running"
            && config.cloudflare.kind == "quick"
            && !facts.public_url_seen)
    {
        return (
            "starting",
            "Starting",
            "The tunnel process is starting. Workspace files are not part of that process.",
        );
    }
    if config.adapter == ADAPTER_CLOUDFLARE && config.cloudflare.enabled {
        if facts.adapter_process == "running"
            && (facts.public_url_seen || config.cloudflare.kind == "existing")
        {
            return (
                "remote_configured",
                "Remote access configured",
                "The tunnel process is running. A connection is not a workspace session. Invite an agent separately, and revoke that invitation to remove access.",
            );
        }
        if facts.adapter_process == "exited" || !facts.cloudflared_installed {
            return (
                "degraded",
                "Degraded",
                "Remote access is enabled in the local file and the tunnel is not healthy. The room data is still in the workspace sidecar.",
            );
        }
        return (
            "remote_configured",
            "Remote access configured",
            "The tunnel is enabled and has not been started. Nothing is published until you start it.",
        );
    }
    if config.adapter == ADAPTER_PRIVATE && config.private_network.enabled {
        if facts.private_installed {
            return (
                "remote_configured",
                "Remote access configured",
                "The private-network preference is saved. Publish a room with the plugin when you start it. The plugin does not grant workspace access.",
            );
        }
        return (
            "degraded",
            "Degraded",
            "Private networking is selected and the executable was not found.",
        );
    }
    if config.mode == MODE_LOCAL || config.adapter == ADAPTER_NONE {
        return (
            "ready_locally",
            "Ready locally",
            "This management page is on loopback. Another machine cannot reach it. Start a room with ce serve when you want agents on this machine to join.",
        );
    }
    (
        "not_configured",
        "Not configured",
        "Choose local-only, or confirm a remote adapter.",
    )
}

pub fn adapter_catalog(config: &Connectivity, facts: &RuntimeFacts) -> Value {
    json!([
        {
            "id": ADAPTER_NONE,
            "title": "Not configured",
            "availability": "available",
            "selected": config.adapter == ADAPTER_NONE,
            "account_required": false,
            "third_party_relay": false,
            "authorizes_workspace": false,
            "detail": "Keep using this machine. ce serve binds loopback unless you pass a plugin."
        },
        {
            "id": ADAPTER_CLOUDFLARE,
            "title": "Managed tunnel",
            "availability": "guided",
            "selected": config.adapter == ADAPTER_CLOUDFLARE,
            "installed": facts.cloudflared_installed,
            "version": facts.cloudflared_version,
            "account_required": config.cloudflare.kind == "existing",
            "quick_tunnel_account": "A quick tunnel still sends traffic through Cloudflare. A named tunnel needs a Cloudflare account.",
            "third_party_relay": true,
            "relays_through": "Cloudflare",
            "process": "child of ce ui",
            "authorizes_workspace": false,
            "automated_provisioning": false,
            "detail": "cloudflared is started only after you confirm. This page does not call the Cloudflare API, create DNS records, or install a service."
        },
        {
            "id": ADAPTER_PRIVATE,
            "title": "Private network",
            "availability": "optional",
            "selected": config.adapter == ADAPTER_PRIVATE,
            "installed": facts.private_installed,
            "account_required": true,
            "third_party_relay": true,
            "relays_through": "the private-network provider",
            "process": "started by ce serve, not by this page",
            "authorizes_workspace": false,
            "detail": "Tailcat is optional. Install it yourself. A missing plugin is transport_unavailable, not a public listener. A two-machine session is not part of the automated tests."
        },
        {
            "id": ADAPTER_CUSTOM,
            "title": "Custom endpoint",
            "availability": "unavailable",
            "selected": false,
            "account_required": false,
            "third_party_relay": false,
            "authorizes_workspace": false,
            "detail": "The room process refuses every bind except loopback. An administrator-configured public origin is not an adapter in this version."
        }
    ])
}

pub fn recorded_loopback_port(transport: &Value) -> Option<u16> {
    let kind = transport.get("type").and_then(Value::as_str)?;
    if kind != "loopback" {
        return None;
    }
    let address = transport
        .get("address")
        .and_then(Value::as_str)
        .unwrap_or("");
    if address != "127.0.0.1" && address != "::1" {
        return None;
    }
    transport
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|port| u16::try_from(port).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_keys_and_unknown_adapters_are_rejected() {
        let err = parse_connectivity("mode = \"local\"\ntoken = \"abc\"\n").unwrap_err();
        assert!(err.message.contains("credentials"), "{}", err.message);
        let err = parse_connectivity("adapter = \"wireguard\"\n").unwrap_err();
        assert!(
            err.message.contains("unsupported adapter"),
            "{}",
            err.message
        );
        let err = parse_connectivity("adapter = \"custom\"\n").unwrap_err();
        assert!(err.message.contains("not available"), "{}", err.message);
    }

    #[test]
    fn remote_enable_requires_confirmation_and_refuses_the_ui_port() {
        let update = ConnectivityUpdate::from_json(&json!({
            "mode": "remote",
            "adapter": "cloudflare",
            "confirm": false,
            "cloudflare": {"enabled": true, "kind": "quick", "origin_port": 9000}
        }))
        .unwrap();
        let err = apply_update(update, 8787).unwrap_err();
        assert_eq!(err.code, "permission_denied");

        let update = ConnectivityUpdate::from_json(&json!({
            "mode": "remote",
            "adapter": "cloudflare",
            "confirm": true,
            "cloudflare": {"enabled": true, "kind": "quick", "origin_port": 8787}
        }))
        .unwrap();
        let err = apply_update(update, 8787).unwrap_err();
        assert!(err.message.contains("management UI"), "{}", err.message);
    }

    #[test]
    fn quick_tunnel_arguments_target_only_the_loopback_origin() {
        let file = CloudflareFile {
            enabled: true,
            kind: "quick".into(),
            origin_port: 43123,
            ..CloudflareFile::default()
        };
        let args = cloudflared_arguments(&file).unwrap();
        assert_eq!(
            args,
            vec![
                "tunnel",
                "--url",
                "http://127.0.0.1:43123",
                "--no-autoupdate"
            ]
        );
        assert!(!args.iter().any(|arg| arg.contains("--token")));
        assert!(!args.iter().any(|arg| arg.contains("8787")));
    }

    #[test]
    fn existing_tunnel_arguments_use_the_name_and_not_a_token() {
        let file = CloudflareFile {
            enabled: true,
            kind: "existing".into(),
            tunnel_name: "room-east".into(),
            hostname: "room.example.com".into(),
            origin_port: 43123,
            ..CloudflareFile::default()
        };
        let args = cloudflared_arguments(&file).unwrap();
        assert_eq!(
            args,
            vec![
                "tunnel",
                "run",
                "--url",
                "http://127.0.0.1:43123",
                "room-east"
            ]
        );
    }

    #[test]
    fn shell_metacharacters_and_hosted_urls_are_rejected() {
        let err =
            parse_connectivity("cloudflare = { binary = \"cloudflared; id\" }\n").unwrap_err();
        assert!(
            err.message.contains("shell") || err.message.contains("invalid"),
            "{}",
            err.message
        );
        let err =
            parse_connectivity("hosted_url = \"https://user:pw@example.com/\"\n").unwrap_err();
        assert!(
            err.message.contains("user info") || err.message.contains("hosted"),
            "{}",
            err.message
        );
        let err = parse_connectivity("hosted_url = \"http://example.com\"\n").unwrap_err();
        assert!(err.message.contains("https"), "{}", err.message);
    }

    #[test]
    fn diagnostics_redact_invitations_bearers_and_tunnel_urls() {
        let raw = "invite commonroom://join/abcDEF_123 bearer ses_aabbccddeeff \
                   https://random-name.trycloudflare.com token=supersecret \
                   eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJldmlsIn0.signature tcABCDEFGHIJKLMNOP";
        let clean = redact_text(raw);
        assert!(!clean.contains("abcDEF_123"));
        assert!(!clean.contains("ses_aabbccddeeff"));
        assert!(!clean.contains("random-name"));
        assert!(!clean.contains("supersecret"));
        assert!(!clean.contains("eyJhbGci"));
        assert!(!clean.contains("tcABCDEFGHIJKLMNOP"));
        assert!(clean.contains("https://….trycloudflare.com"));
    }

    #[test]
    fn health_separates_a_running_tunnel_from_authorization() {
        let config = Connectivity {
            mode: MODE_REMOTE.into(),
            adapter: ADAPTER_CLOUDFLARE.into(),
            cloudflare: CloudflareFile {
                enabled: true,
                kind: "quick".into(),
                origin_port: 9,
                ..CloudflareFile::default()
            },
            ..Connectivity::default()
        };
        let checks = health_checks(
            &config,
            &RuntimeFacts {
                ui_listening: true,
                adapter_process: "running",
                public_url_seen: true,
                process_exit: None,
                cloudflared_installed: true,
                cloudflared_version: Some("2026.10.17".into()),
                private_installed: false,
                origin_accepts: true,
                edge_tcp: None,
            },
        );
        let auth = checks
            .iter()
            .find(|check| check.id == "authorization")
            .unwrap();
        assert_eq!(auth.state, "unsupported");
        let endpoint = checks
            .iter()
            .find(|check| check.id == "remote_endpoint")
            .unwrap();
        assert_eq!(endpoint.state, "pass");
        let tls = checks.iter().find(|check| check.id == "tls").unwrap();
        assert_eq!(tls.state, "not_checked");
        let (phase, _, _) = phase(
            &config,
            &RuntimeFacts {
                ui_listening: true,
                adapter_process: "running",
                public_url_seen: true,
                process_exit: None,
                cloudflared_installed: true,
                cloudflared_version: None,
                private_installed: false,
                origin_accepts: true,
                edge_tcp: None,
            },
        );
        assert_eq!(phase, "remote_configured");
    }

    #[test]
    fn disable_clears_remote_flags_without_touching_the_workspace_path() {
        let current = Connectivity {
            mode: MODE_REMOTE.into(),
            adapter: ADAPTER_CLOUDFLARE.into(),
            workspace: "/tmp/notes.md".into(),
            cloudflare: CloudflareFile {
                enabled: true,
                origin_port: 9,
                ..CloudflareFile::default()
            },
            ..Connectivity::default()
        };
        let next = disable_remote(current);
        assert_eq!(next.mode, MODE_LOCAL);
        assert!(!next.cloudflare.enabled);
        assert_eq!(next.workspace, "/tmp/notes.md");
    }

    #[test]
    fn loopback_port_is_kept_and_other_addresses_are_not() {
        assert_eq!(
            recorded_loopback_port(
                &json!({"type": "loopback", "address": "127.0.0.1", "port": 4242})
            ),
            Some(4242)
        );
        assert_eq!(
            recorded_loopback_port(
                &json!({"type": "tailcat", "address": "tcABCDEFGHIJKLMNOP", "port": 4242})
            ),
            None
        );
    }

    #[test]
    fn path_lookup_does_not_execute_the_file() {
        let dir = std::env::temp_dir().join(format!("ce-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let binary = dir.join("cloudflared");
        std::fs::write(&binary, "#!/bin/sh\necho ran > \"$0.ran\"\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&binary).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&binary, permissions).unwrap();
        }
        let found = executable_on_path("cloudflared", Some(dir.to_str().unwrap()));
        assert_eq!(found.unwrap(), binary);
        assert!(!dir.join("cloudflared.ran").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
