use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use ce_core::WorkspaceEngine;
use ce_protocol::{invalid_request, permission_denied, unauthorized, CommonroomError, Limits};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::connectivity::{
    adapter_catalog, apply_update, command_version, disable_remote, executable_on_path,
    health_checks, load_connectivity, phase, recorded_loopback_port, redact_text,
    save_connectivity, Connectivity, ConnectivityUpdate, RuntimeFacts, ADAPTER_CLOUDFLARE,
    MODE_HOSTED,
};
use crate::tunnel::TunnelProcess;

const PAGE: &str = include_str!("ui.html");

struct Gate {
    session: String,
    token: Mutex<Option<String>>,
}

pub struct UiState {
    config_path: PathBuf,
    bind_host: String,
    port: u16,
    gate: Gate,
    config: Mutex<Connectivity>,
    tunnel: Mutex<TunnelProcess>,
    version_cache: Mutex<Option<(Instant, Option<String>)>>,
}

pub struct RunningUi {
    pub local_url: String,
    pub port: u16,
    state: Arc<UiState>,
    shutdown: watch::Sender<bool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

pub struct UiOptions {
    pub host: String,
    pub port: u16,
    pub config_path: PathBuf,
    pub open_browser: bool,
}

impl RunningUi {
    pub fn banner(&self) -> String {
        render_ui_banner(&self.local_url)
    }

    pub async fn shutdown(mut self) {
        self.state.tunnel.lock().expect("tunnel").stop();
        let _ = self.shutdown.send(true);
        for task in self.tasks.drain(..) {
            let _ = tokio::time::timeout(Duration::from_secs(3), task).await;
        }
    }
}

pub fn render_ui_banner(url: &str) -> String {
    format!(
        "Complexity Eater\n\nManagement UI:\n  {url}\n\nThis page is only on this machine. Ctrl+C stops it.\nIt does not publish the room. Start a room with `ce serve`.\nA tunnel, when you start one from the page, is a child of this process.\n"
    )
}

pub async fn start_ui(options: UiOptions) -> Result<RunningUi, CommonroomError> {
    let bind_host = crate::room::loopback_host(&options.host)?;
    let config = load_connectivity(&options.config_path)?;
    let listener = TcpListener::bind((bind_host, options.port))
        .await
        .map_err(|err| invalid_request(format!("management UI did not start: {err}")))?;
    let port = listener
        .local_addr()
        .map_err(|err| invalid_request(format!("management UI did not start: {err}")))?
        .port();
    let token = random_token();
    let session = random_token();
    let state = Arc::new(UiState {
        config_path: options.config_path,
        bind_host: if bind_host == "::1" {
            "::1".into()
        } else {
            "127.0.0.1".into()
        },
        port,
        gate: Gate {
            session,
            token: Mutex::new(Some(token.clone())),
        },
        config: Mutex::new(config),
        tunnel: Mutex::new(TunnelProcess::new()),
        version_cache: Mutex::new(None),
    });
    let (shutdown, mut watch_rx) = watch::channel(false);
    let app = router(Arc::clone(&state));
    let task = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = watch_rx.changed().await;
        });
        let _ = server.await;
    });
    let local_url = if bind_host == "::1" {
        format!("http://[::1]:{port}/?token={token}")
    } else {
        format!("http://127.0.0.1:{port}/?token={token}")
    };
    crate::room::wait_until_accepts(bind_host, port);
    if options.open_browser {
        open_browser(&local_url);
    }
    Ok(RunningUi {
        local_url,
        port,
        state,
        shutdown,
        tasks: vec![task],
    })
}

pub fn open_browser(url: &str) {
    if !url.starts_with("http://127.0.0.1:")
        && !url.starts_with("http://localhost:")
        && !url.starts_with("http://[::1]:")
    {
        return;
    }
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
    } else {
        std::process::Command::new("xdg-open")
    };
    if cfg!(target_os = "windows") {
        command.args(["/C", "start", "", url]);
    } else {
        command.arg(url);
    }
    let _ = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn router(state: Arc<UiState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/overview", get(overview))
        .route(
            "/api/connectivity",
            get(overview).post(save_connectivity_route),
        )
        .route("/api/connectivity/disable", post(disable_route))
        .route("/api/connectivity/test", post(test_route))
        .route("/api/tunnel/start", post(start_tunnel))
        .route("/api/tunnel/stop", post(stop_tunnel))
        .route("/api/tunnel/reconnect", post(reconnect_tunnel))
        .route("/api/access", get(access_list))
        .route("/api/access/invite", post(access_invite))
        .route("/api/access/revoke", post(access_revoke))
        .route("/api/diagnostics", get(diagnostics))
        .route("/api/settings", post(save_settings))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024))
        .with_state(state)
}

async fn index(State(state): State<Arc<UiState>>, request: Request) -> Response {
    if let Some(response) = host_guard(&state, &request) {
        return response;
    }
    if let Some(token) = query_param(request.uri().query(), "token") {
        let mut slot = state.gate.token.lock().expect("ui token");
        if slot.as_deref() == Some(token.as_str()) {
            *slot = None;
            let session = state.gate.session.clone();
            return (
                StatusCode::SEE_OTHER,
                [
                    (header::LOCATION, "/".to_string()),
                    (header::SET_COOKIE, cookie_header(&session)),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                "See other",
            )
                .into_response();
        }
        return error_response(unauthorized("open the URL printed by ce ui"));
    }
    if !cookie_ok(&state, request.headers()) {
        return (
            StatusCode::OK,
            [(header::CACHE_CONTROL, "no-store")],
            Html(
                "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Complexity Eater</title><p>Open the management URL printed by <code>ce ui</code>. This page is only on this machine.</p></html>",
            ),
        )
            .into_response();
    }
    ([(header::CACHE_CONTROL, "no-store")], Html(PAGE)).into_response()
}

async fn overview(State(state): State<Arc<UiState>>, request: Request) -> Response {
    if let Some(response) = authorized(&state, &request, false) {
        return response;
    }
    Json(snapshot(&state, false)).into_response()
}

async fn save_connectivity_route(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    let value = match json_object(&bytes) {
        Ok(value) => value,
        Err(err) => return error_response(err),
    };
    let update = match ConnectivityUpdate::from_json(&value) {
        Ok(update) => update,
        Err(err) => return error_response(err),
    };
    let mut update = update;
    if update.workspace.is_empty() {
        update.workspace = state.config.lock().expect("config").workspace.clone();
    }
    let next = match apply_update(update, state.port) {
        Ok(next) => next,
        Err(err) => return error_response(err),
    };
    if let Err(err) = save_connectivity(&state.config_path, &next) {
        return error_response(err);
    }
    *state.config.lock().expect("config") = next;
    Json(snapshot(&state, false)).into_response()
}

async fn save_settings(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    let value = match json_object(&bytes) {
        Ok(value) => value,
        Err(err) => return error_response(err),
    };
    let workspace = value
        .get("workspace")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let mut next = state.config.lock().expect("config").clone();
    next.workspace = workspace;
    if let Err(err) = crate::connectivity::validate_shape(&next) {
        return error_response(err);
    }
    if let Err(err) = save_connectivity(&state.config_path, &next) {
        return error_response(err);
    }
    *state.config.lock().expect("config") = next;
    Json(snapshot(&state, false)).into_response()
}

async fn disable_route(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    if let Err(response) = require_confirm(&bytes) {
        return *response;
    }
    state.tunnel.lock().expect("tunnel").stop();
    let next = disable_remote(state.config.lock().expect("config").clone());
    if let Err(err) = save_connectivity(&state.config_path, &next) {
        return error_response(err);
    }
    *state.config.lock().expect("config") = next;
    let mut body = snapshot(&state, false);
    body["phase"] = json!("remote_disabled");
    body["phase_label"] = json!("Remote access disabled");
    body["phase_detail"] = json!(
        "The tunnel child was stopped and the remote preference was cleared. A named Cloudflare tunnel, DNS record, or login certificate remains in the Cloudflare account until you delete it there. The workspace file was not deleted."
    );
    body["tone"] = json!("attention");
    Json(body).into_response()
}

async fn test_route(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    let _ = bytes;
    Json(snapshot(&state, true)).into_response()
}

async fn start_tunnel(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    if let Err(response) = require_confirm(&bytes) {
        return *response;
    }
    let config = state.config.lock().expect("config").clone();
    if config.adapter != ADAPTER_CLOUDFLARE || !config.cloudflare.enabled {
        return error_response(invalid_request(
            "enable the Cloudflare adapter and confirm it before starting a tunnel",
        ));
    }
    if let Err(err) =
        crate::connectivity::validate_origin(config.cloudflare.origin_port, state.port)
    {
        return error_response(err);
    }
    if !port_accepts(config.cloudflare.origin_port) {
        return error_response(invalid_request(
            "nothing is listening on the loopback origin. Start the room, then start the tunnel. Nothing was published",
        ));
    }
    let Some(binary) = executable_on_path(config.cloudflare_binary(), None) else {
        return error_response(invalid_request(
            "cloudflared is not installed. Install it from Cloudflare's documentation, then try again. This page does not install it",
        ));
    };
    let started = {
        let mut tunnel = state.tunnel.lock().expect("tunnel");
        tunnel.start(&binary, &config.cloudflare)
    };
    if let Err(message) = started {
        return error_response(invalid_request(message));
    }
    Json(snapshot(&state, false)).into_response()
}

async fn stop_tunnel(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    if let Err(response) = require_confirm(&bytes) {
        return *response;
    }
    state.tunnel.lock().expect("tunnel").stop();
    Json(snapshot(&state, false)).into_response()
}

async fn reconnect_tunnel(State(state): State<Arc<UiState>>, request: Request) -> Response {
    state.tunnel.lock().expect("tunnel").stop();
    start_tunnel(State(state), request).await
}

async fn access_list(State(state): State<Arc<UiState>>, request: Request) -> Response {
    if let Some(response) = authorized(&state, &request, false) {
        return response;
    }
    match with_workspace(&state, |engine| {
        let invites = engine.list_invites()?;
        Ok(json!({
            "invites": invites.into_iter().map(|invite| json!({
                "invite_id": invite.invite_id,
                "capabilities": invite.capabilities,
                "expires_at": invite.expires_at,
                "max_uses": invite.max_uses,
                "used_count": invite.used_count,
                "revoked_at": invite.revoked_at,
            })).collect::<Vec<_>>()
        }))
    }) {
        Ok(body) => Json(body).into_response(),
        Err(err) => error_response(err),
    }
}

async fn access_invite(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    if let Err(response) = require_confirm(&bytes) {
        return *response;
    }
    match with_workspace(&state, |engine| {
        let invite = engine.create_invite_with(
            None,
            None,
            Limits::default().default_invite_minutes,
            Some(1),
            Some(vec![
                "read".into(),
                "observe".into(),
                "propose".into(),
                "checkout".into(),
                "commit".into(),
            ]),
        )?;
        Ok(json!({
            "uri": invite.uri,
            "invite_id": invite.invite_id,
            "capabilities": ["read", "observe", "propose", "checkout", "commit"],
            "note": "Shown once. This is not a tunnel URL. Revoke the invitation to end unused access."
        }))
    }) {
        Ok(body) => Json(body).into_response(),
        Err(err) => error_response(err),
    }
}

async fn access_revoke(State(state): State<Arc<UiState>>, request: Request) -> Response {
    let bytes = match read_body(request, &state).await {
        Ok(bytes) => bytes,
        Err(response) => return *response,
    };
    if let Err(response) = require_confirm(&bytes) {
        return *response;
    }
    let value = match json_object(&bytes) {
        Ok(value) => value,
        Err(err) => return error_response(err),
    };
    let invite_id = value
        .get("invite_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if invite_id.is_empty() {
        return error_response(invalid_request("invite_id is required"));
    }
    match with_workspace(&state, |engine| {
        engine.revoke_invite(None, Some(&invite_id))?;
        Ok(json!({"status": "revoked", "invite_id": invite_id}))
    }) {
        Ok(body) => Json(body).into_response(),
        Err(err) => error_response(err),
    }
}

async fn diagnostics(State(state): State<Arc<UiState>>, request: Request) -> Response {
    if let Some(response) = authorized(&state, &request, false) {
        return response;
    }
    let mut body = snapshot(&state, false);
    body.as_object_mut().expect("object").remove("public_url");
    let text = redact_text(&serde_json::to_string_pretty(&body).unwrap_or_default());
    Json(json!({ "text": text })).into_response()
}

fn snapshot(state: &UiState, edge: bool) -> Value {
    let config = state.config.lock().expect("config").clone();
    let mut tunnel = state.tunnel.lock().expect("tunnel");
    let status = tunnel.status();
    drop(tunnel);
    let installed = executable_on_path(config.cloudflare_binary(), None);
    let version = installed
        .as_ref()
        .and_then(|path| cached_version(state, path));
    let private_installed = executable_on_path(config.private_binary(), None).is_some();
    let recorded = workspace_port(&config.workspace);
    let origin_accepts =
        config.cloudflare.origin_port != 0 && port_accepts(config.cloudflare.origin_port);
    let edge_tcp = if edge {
        probe_edge(&status.public_url, &config.cloudflare.hostname)
    } else {
        None
    };
    let facts = RuntimeFacts {
        ui_listening: true,
        adapter_process: if status.running {
            "running"
        } else if status.exit_code.is_some() {
            "exited"
        } else {
            "stopped"
        },
        public_url_seen: status.public_url.is_some(),
        process_exit: status.exit_code,
        cloudflared_installed: installed.is_some(),
        cloudflared_version: version.clone(),
        private_installed,
        origin_accepts,
        edge_tcp,
    };
    let (phase_id, phase_label, phase_detail) = phase(&config, &facts);
    let tone = match phase_id {
        "ready_locally" => "local",
        "degraded" | "unsupported" | "remote_disabled" => "attention",
        _ => "",
    };
    let mut warnings = vec![
        "Loopback is this machine only. Binding to 127.0.0.1 does not make the room reachable elsewhere.".to_string(),
        "A tunnel or plugin can make the session ingress reachable. It does not authorize an agent.".to_string(),
        "Hosted coordination is not implemented. Offline handoff packages are not implemented.".to_string(),
        "Cf-Access identity headers are ignored. Join with an invitation.".to_string(),
    ];
    if config.mode == MODE_HOSTED {
        warnings.push(
            "The saved hosted URL is a note. This application cannot authenticate to a hosted service."
                .into(),
        );
    }
    let public_url = status.running.then(|| status.public_url.clone()).flatten();
    if public_url.is_some() {
        warnings.push(
            "A public hostname is not a private network and is not an invitation. Treat it as a secret."
                .into(),
        );
    }
    let room_detail = match &recorded {
        Some(port) => format!(
            "The workspace file recorded loopback ingress port {port}. Participants in the sidecar are counted when the file can be opened. The room process is separate from this page."
        ),
        None => "No loopback ingress is recorded. Start a room with ce serve. Agents on this machine use the invitation that command prints.".into(),
    };
    json!({
        "phase": phase_id,
        "phase_label": phase_label,
        "phase_detail": phase_detail,
        "tone": tone,
        "mode": config.mode,
        "adapter": config.adapter,
        "management_bind": format!("http://{}:{}/ on loopback. The session ingress is a different port and is the only origin a tunnel may use.", state.bind_host, state.port),
        "room_detail": room_detail,
        "warnings": warnings,
        "config": {
            "mode": config.mode,
            "adapter": config.adapter,
            "workspace": config.workspace,
            "hosted_url": config.hosted_url,
            "cloudflare": {
                "enabled": config.cloudflare.enabled,
                "kind": config.cloudflare.kind,
                "tunnel_name": config.cloudflare.tunnel_name,
                "hostname": config.cloudflare.hostname,
                "origin_port": config.cloudflare.origin_port,
            },
            "private_network": {"enabled": config.private_network.enabled}
        },
        "cloudflared": {
            "installed": installed.is_some(),
            "version": version,
        },
        "private_installed": private_installed,
        "recorded_ingress_port": recorded,
        "public_url": public_url,
        "tunnel_origin": status.origin,
        "tunnel_kind": status.kind,
        "tunnel_process": if status.running {
            "child process running"
        } else if status.exit_code.is_some() {
            "child process exited"
        } else {
            "not started"
        },
        "checks": health_checks(&config, &facts).into_iter().map(|check| check.to_json()).collect::<Vec<_>>(),
        "adapters": adapter_catalog(&config, &facts),
        "config_file": state.config_path.display().to_string(),
        "remains_after_disable": [
            "A named Cloudflare tunnel and its DNS record, if you created them",
            "The cloudflared login certificate in cloudflared's own directory",
            "The markdown file and its .commonroom.db sidecar"
        ],
        "hosted": {
            "availability": "not_implemented",
            "provider_endpoint": null,
            "saved_url": config.hosted_url,
        },
        "collaboration": {
            "review": if config.mode == MODE_HOSTED { "not_implemented" } else { "supported_locally" },
            "questions": if config.mode == MODE_HOSTED { "not_implemented" } else { "supported_locally" },
            "handoff": if config.mode == MODE_HOSTED { "not_implemented" } else { "supported_locally" },
            "offline_package": "not_implemented",
            "hosted": "not_implemented"
        },
        "identity": "Workspace identity is the invitation session. Cloudflare Access headers are not read.",
        "tunnel_lines": status.lines,
    })
}

fn probe_edge(public_url: &Option<String>, hostname: &str) -> Option<bool> {
    let host = public_url
        .as_deref()
        .and_then(|url| url.strip_prefix("https://"))
        .map(|rest| rest.split('/').next().unwrap_or(rest).to_string())
        .filter(|host| !host.is_empty())
        .or_else(|| (!hostname.is_empty()).then(|| hostname.to_string()))?;
    if host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let address = format!("{host}:443");
    let Ok(address) = address.parse() else {
        return None;
    };
    Some(std::net::TcpStream::connect_timeout(&address, Duration::from_millis(800)).is_ok())
}

fn cached_version(state: &UiState, binary: &Path) -> Option<String> {
    let mut cache = state.version_cache.lock().expect("version");
    if let Some((at, value)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(30) {
            return value.clone();
        }
    }
    let value = command_version(binary).ok();
    *cache = Some((Instant::now(), value.clone()));
    value
}

fn workspace_port(path: &str) -> Option<u16> {
    if path.is_empty() || !Path::new(path).is_file() {
        return None;
    }
    let engine = WorkspaceEngine::open(path, None, Limits::default(), None).ok()?;
    let transport = engine.current_transport().ok().flatten()?;
    let port = recorded_loopback_port(&transport);
    engine.close();
    port
}

fn with_workspace(
    state: &UiState,
    body: impl FnOnce(&WorkspaceEngine) -> Result<Value, CommonroomError>,
) -> Result<Value, CommonroomError> {
    let path = state.config.lock().expect("config").workspace.clone();
    if path.is_empty() {
        return Err(invalid_request(
            "set the workspace file in Settings before managing invitations",
        ));
    }
    let engine = WorkspaceEngine::open(&path, None, Limits::default(), None)?;
    let result = body(&engine);
    engine.close();
    result
}

fn port_accepts(port: u16) -> bool {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
}

async fn read_body(request: Request, state: &UiState) -> Result<Bytes, Box<Response>> {
    if let Some(response) = authorized(state, &request, true) {
        return Err(Box::new(response));
    }
    let bytes = axum::body::to_bytes(request.into_body(), 64 * 1024)
        .await
        .map_err(|_| Box::new(error_response(invalid_request("body is not readable"))))?;
    Ok(bytes)
}

fn require_confirm(bytes: &Bytes) -> Result<(), Box<Response>> {
    let value = json_object(bytes).map_err(|err| Box::new(error_response(err)))?;
    if value.get("confirm").and_then(Value::as_bool) != Some(true) {
        return Err(Box::new(error_response(permission_denied(
            "explicit confirmation is required",
        ))));
    }
    Ok(())
}

fn json_object(bytes: &Bytes) -> Result<Value, CommonroomError> {
    if bytes.is_empty() {
        return Ok(json!({}));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| invalid_request("body must be JSON"))?;
    if !value.is_object() {
        return Err(invalid_request("body must be a JSON object"));
    }
    Ok(value)
}

fn authorized(state: &UiState, request: &Request, mutating: bool) -> Option<Response> {
    if let Some(response) = host_guard(state, request) {
        return Some(response);
    }
    if !cookie_ok(state, request.headers()) {
        return Some(error_response(unauthorized(
            "open the URL printed by ce ui",
        )));
    }
    if !mutating {
        return None;
    }
    if request
        .headers()
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        == Some("cross-site")
    {
        return Some(error_response(permission_denied(
            "cross-site request rejected",
        )));
    }
    if let Some(origin) = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        let allowed = [
            format!("http://127.0.0.1:{}", state.port),
            format!("http://localhost:{}", state.port),
            format!("http://[::1]:{}", state.port),
        ];
        if !allowed.iter().any(|item| item == origin) {
            return Some(error_response(permission_denied(
                "cross-site request rejected",
            )));
        }
    }
    None
}

fn host_guard(state: &UiState, request: &Request) -> Option<Response> {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if host_allowed(host, state.port) {
        None
    } else {
        Some(error_response(invalid_request(
            "host is not the local management listener",
        )))
    }
}

fn host_allowed(value: &str, port: u16) -> bool {
    let value = value.trim();
    let (name, seen_port) = if let Some(rest) = value.strip_prefix('[') {
        let Some((name, rest)) = rest.split_once(']') else {
            return false;
        };
        let seen = rest.strip_prefix(':').and_then(|text| text.parse().ok());
        (name, seen)
    } else if let Some((name, port_text)) = value.rsplit_once(':') {
        (name, port_text.parse().ok())
    } else {
        (value, None)
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1") && seen_port == Some(port)
}

fn cookie_ok(state: &UiState, headers: &HeaderMap) -> bool {
    cookie(headers, "ce_ui_session").as_deref() == Some(state.gate.session.as_str())
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let value = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in value.split(';') {
        let mut pieces = part.trim().splitn(2, '=');
        if pieces.next()?.trim() == name {
            return Some(pieces.next()?.trim().to_string());
        }
    }
    None
}

fn cookie_header(session: &str) -> String {
    format!("ce_ui_session={session}; HttpOnly; SameSite=Strict; Path=/")
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if parts.next()? == name {
            return Some(parts.next().unwrap_or("").to_string());
        }
    }
    None
}

fn error_response(err: CommonroomError) -> Response {
    let status = StatusCode::from_u16(err.http_status).unwrap_or(StatusCode::BAD_REQUEST);
    (status, Json(err.to_value())).into_response()
}

fn random_token() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 18];
    let mut file = std::fs::File::open("/dev/urandom").expect("urandom");
    file.read_exact(&mut bytes).expect("urandom");
    hex::encode(bytes)
}
