use std::io::Write;
use std::sync::Arc;

use ce_core::WorkspaceEngine;
use ce_protocol::{
    decode_invitation, invalid_request, is_remote_invitation, unsupported_version, CommonroomError,
    Limits,
};
use ce_transport::connect_invitation;
use clap::{Arg, ArgAction, Command};
use serde::Serialize;
use serde_json::{json, Value};

use crate::dialer::run_dialer;
use crate::http_client::http_exchange;
use crate::room::start_room;

const COMMANDS: &[&str] = &[
    "serve",
    "status",
    "invite",
    "join",
    "call",
    "participants",
    "leases",
    "proposals",
    "history",
    "diff",
    "mcp",
    "remove-participant",
];

pub async fn run() -> i32 {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    if argv
        .first()
        .is_some_and(|first| !COMMANDS.contains(&first.as_str()) && !first.starts_with('-'))
    {
        argv.insert(0, "serve".into());
    }
    match dispatch(argv).await {
        Ok(code) => code,
        Err(err) => {
            print_stdout(&serde_json::to_string_pretty(&err.to_value()).unwrap_or(err.message));
            1
        }
    }
}

async fn dispatch(argv: Vec<String>) -> Result<i32, CommonroomError> {
    let matches = match parser().try_get_matches_from(std::iter::once("ce".to_string()).chain(argv))
    {
        Ok(matches) => matches,
        Err(err) => {
            let _ = err.print();
            return Ok(err.exit_code());
        }
    };
    let (command, args) = matches.subcommand().unwrap_or(("serve", &matches));
    match command {
        "serve" => serve(args).await,
        "mcp" => Ok(run_dialer(
            args.get_one::<String>("invite")
                .map(String::as_str)
                .unwrap_or(""),
            args.get_one::<String>("name")
                .map(String::as_str)
                .unwrap_or("agent"),
            None,
        )),
        "join"
            if args
                .get_one::<String>("invite")
                .is_some_and(|invite| is_remote_invitation(invite)) =>
        {
            join_remote(args).await
        }
        "call" => call_remote(args).await,
        other => local(other, args),
    }
}

fn parser() -> Command {
    let workspace = workspace_default();
    let name = default_name();
    Command::new("ce")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Complexity Eater. Different minds. Shared work.")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("serve")
                .about("Open a room around a local substrate")
                .arg(Arg::new("path").required(true))
                .arg(Arg::new("host").long("host").default_value("127.0.0.1"))
                .arg(
                    Arg::new("port")
                        .long("port")
                        .value_parser(clap::value_parser!(u16))
                        .default_value("8000"),
                )
                .arg(Arg::new("name").long("name").default_value(leak_str(name)))
                .arg(
                    Arg::new("tailcat")
                        .long("tailcat")
                        .action(ArgAction::SetTrue)
                        .help("Expose the room through the Tailcat plugin"),
                )
                .arg(
                    Arg::new("transport")
                        .long("transport")
                        .help("Transport plugin name"),
                )
                .arg(
                    Arg::new("ephemeral")
                        .long("ephemeral")
                        .action(ArgAction::SetTrue)
                        .help("Keep metadata only for this process"),
                )
                .arg(Arg::new("json").long("json").action(ArgAction::SetTrue)),
        )
        .subcommand(workspace_command(
            "status",
            "Show workspace status",
            &workspace,
        ))
        .subcommand(
            Command::new("invite")
                .about("Create an invitation")
                .arg(workspace_arg(&workspace))
                .arg(Arg::new("permissions").long("permissions"))
                .arg(
                    Arg::new("capabilities")
                        .long("capabilities")
                        .default_value("read,observe,propose,checkout,commit"),
                )
                .arg(Arg::new("target").long("target"))
                .arg(
                    Arg::new("expires")
                        .long("expires")
                        .value_parser(clap::value_parser!(i64))
                        .default_value("60"),
                )
                .arg(
                    Arg::new("max-uses")
                        .long("max-uses")
                        .value_parser(clap::value_parser!(i64))
                        .default_value("1"),
                )
                .arg(json_arg()),
        )
        .subcommand(
            Command::new("join")
                .about("Join with an invitation")
                .arg(Arg::new("invite").required(true))
                .arg(workspace_arg(&workspace))
                .arg(
                    Arg::new("kind")
                        .long("kind")
                        .default_value("agent")
                        .value_parser(["human", "agent"]),
                )
                .arg(Arg::new("name").long("name").default_value("participant"))
                .arg(Arg::new("hold").long("hold").action(ArgAction::SetTrue))
                .arg(
                    Arg::new("no-hold")
                        .long("no-hold")
                        .action(ArgAction::SetTrue),
                )
                .arg(json_arg()),
        )
        .subcommand(
            Command::new("call")
                .about("Send one request through an invitation")
                .arg(Arg::new("invite").required(true))
                .arg(
                    Arg::new("method")
                        .required(true)
                        .value_parser(["GET", "POST", "get", "post"]),
                )
                .arg(Arg::new("path").required(true))
                .arg(Arg::new("session").long("session"))
                .arg(Arg::new("body").long("body").help("JSON request body")),
        )
        .subcommand(workspace_command(
            "participants",
            "List participants",
            &workspace,
        ))
        .subcommand(workspace_command(
            "leases",
            "List active leases",
            &workspace,
        ))
        .subcommand(workspace_command("proposals", "List proposals", &workspace))
        .subcommand(workspace_command(
            "history",
            "Show recent events",
            &workspace,
        ))
        .subcommand(
            Command::new("diff")
                .about("Show the diff since a version")
                .arg(workspace_arg(&workspace))
                .arg(
                    Arg::new("base")
                        .long("base")
                        .required(true)
                        .value_parser(clap::value_parser!(i64)),
                )
                .arg(json_arg()),
        )
        .subcommand(
            Command::new("mcp")
                .about("Dial an invitation and speak MCP on stdio")
                .arg(Arg::new("invite").long("invite").required(true))
                .arg(Arg::new("name").long("name").default_value("agent")),
        )
        .subcommand(
            Command::new("remove-participant")
                .about("End a participant session")
                .arg(workspace_arg(&workspace))
                .arg(Arg::new("participant").long("participant").required(true)),
        )
}

fn workspace_command(name: &'static str, about: &'static str, workspace: &str) -> Command {
    Command::new(name)
        .about(about)
        .arg(workspace_arg(workspace))
        .arg(json_arg())
}

fn workspace_arg(workspace: &str) -> Arg {
    Arg::new("workspace")
        .long("workspace")
        .default_value(leak_str(workspace))
}

fn json_arg() -> Arg {
    Arg::new("json").long("json").action(ArgAction::SetTrue)
}

fn leak_str(value: impl Into<String>) -> &'static str {
    Box::leak(value.into().into_boxed_str())
}

fn workspace_default() -> String {
    std::env::var("CE_WORKSPACE")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("COMMONROOM_WORKSPACE")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "workspace.md".into())
}

fn default_name() -> String {
    std::env::var("USER")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            std::env::var("USERNAME")
                .ok()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "host".into())
}

async fn serve(args: &clap::ArgMatches) -> Result<i32, CommonroomError> {
    let path = args
        .get_one::<String>("path")
        .map(String::as_str)
        .unwrap_or("workspace.md");
    let ephemeral = args.get_flag("ephemeral");
    let db_dir = if ephemeral {
        let dir = std::env::temp_dir().join(format!(
            "ce-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).map_err(|err| invalid_request(err.to_string()))?;
        Some(dir)
    } else {
        None
    };
    let db_path = db_dir.as_ref().map(|dir| dir.join("workspace.db"));
    let engine = Arc::new(WorkspaceEngine::open(
        path,
        db_path,
        Limits::default(),
        None,
    )?);
    let transport = args
        .get_one::<String>("transport")
        .map(String::as_str)
        .or_else(|| args.get_flag("tailcat").then_some("tailcat"));
    let host = args
        .get_one::<String>("host")
        .map(String::as_str)
        .unwrap_or("127.0.0.1");
    let port = *args.get_one::<u16>("port").unwrap_or(&8000);
    let name = args
        .get_one::<String>("name")
        .map(String::as_str)
        .unwrap_or("host");
    let result = async {
        let mut room = start_room(Arc::clone(&engine), host, port, name, transport).await?;
        if args.get_flag("json") {
            let snapshot = room.snapshot()?;
            print_stdout(&serde_json::to_string_pretty(&snapshot).unwrap_or_default());
        } else {
            print_stdout(&room.banner()?);
        }
        wait_for_shutdown().await;
        room.shutdown().await;
        Ok::<(), CommonroomError>(())
    }
    .await;
    engine.close();
    if let Some(dir) = db_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
    result.map(|()| 0)
}

fn local(command: &str, args: &clap::ArgMatches) -> Result<i32, CommonroomError> {
    let path = args
        .get_one::<String>("workspace")
        .map(String::as_str)
        .unwrap_or("workspace.md");
    let engine = WorkspaceEngine::open(path, None, Limits::default(), None)?;
    let result = (|| {
        let as_json = args
            .try_get_one::<bool>("json")
            .ok()
            .and_then(|value| value.copied())
            .unwrap_or(false);
        match command {
            "status" => {
                let mut payload = serde_json::to_value(engine.status()?)
                    .map_err(|err| ce_protocol::internal_error(err.to_string()))?;
                payload["listener"] = engine.connection_diagnostics()?.listener;
                emit(&payload, as_json, status_text)
            }
            "invite" => {
                let capabilities = args
                    .get_one::<String>("capabilities")
                    .map(|value| {
                        value
                            .split(',')
                            .map(str::trim)
                            .filter(|item| !item.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                let invite = engine.create_invite_with(
                    args.get_one::<String>("permissions").map(String::as_str),
                    args.get_one::<String>("target").map(String::as_str),
                    *args.get_one::<i64>("expires").unwrap_or(&60),
                    Some(*args.get_one::<i64>("max-uses").unwrap_or(&1)),
                    Some(capabilities),
                )?;
                let payload = serde_json::to_value(&invite)
                    .map_err(|err| ce_protocol::internal_error(err.to_string()))?;
                emit(&payload, as_json, |item| {
                    format!(
                        "{}\n",
                        item.get("uri").and_then(Value::as_str).unwrap_or("")
                    )
                })
            }
            "join" => {
                let session = engine.join(
                    args.get_one::<String>("kind")
                        .map(String::as_str)
                        .unwrap_or("agent"),
                    args.get_one::<String>("name")
                        .map(String::as_str)
                        .unwrap_or("participant"),
                    None,
                    args.get_one::<String>("invite").map(String::as_str),
                    "local",
                    None,
                )?;
                let payload = serde_json::to_value(&session)
                    .map_err(|err| ce_protocol::internal_error(err.to_string()))?;
                emit(&payload, as_json, |item| {
                    format!(
                        "{}\n",
                        item.get("participant_id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                    )
                })
            }
            "participants" => emit_list(&engine.participants()?, as_json, people_text),
            "leases" => emit_list(&engine.leases()?, as_json, leases_text),
            "proposals" => emit_list(&engine.proposals()?, as_json, proposals_text),
            "history" => emit_list(&engine.history(50, None)?, as_json, history_text),
            "diff" => {
                let payload =
                    serde_json::to_value(engine.diff(*args.get_one::<i64>("base").unwrap_or(&0))?)
                        .map_err(|err| ce_protocol::internal_error(err.to_string()))?;
                emit(&payload, as_json, |item| {
                    item.get("diff")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string()
                })
            }
            "remove-participant" => {
                engine.leave(
                    args.get_one::<String>("participant")
                        .map(String::as_str)
                        .unwrap_or(""),
                )?;
                print_stdout("ok");
                Ok(0)
            }
            _ => Ok(1),
        }
    })();
    engine.close();
    result
}

async fn join_remote(args: &clap::ArgMatches) -> Result<i32, CommonroomError> {
    let invite = args
        .get_one::<String>("invite")
        .map(String::as_str)
        .unwrap_or("");
    let document = invitation_document(invite)?;
    let (mut plugin, mut connection, hint) = connect_invitation(&document)?;
    let secret = document.get("secret").and_then(Value::as_str).unwrap_or("");
    let (status, body) = http_exchange(
        connection.as_mut(),
        "POST",
        "/session/join",
        Some(&json!({
            "invite": secret,
            "name": args.get_one::<String>("name").map(String::as_str).unwrap_or("participant"),
            "kind": args.get_one::<String>("kind").map(String::as_str).unwrap_or("agent"),
            "transport": hint.get("type").and_then(Value::as_str).unwrap_or("remote")
        })),
        None,
    )?;
    connection.close();
    if status != 200 {
        print_stdout(&serde_json::to_string_pretty(&body).unwrap_or_default());
        plugin.close();
        return Ok(1);
    }
    if body.get("workspace_id") != document.get("workspace_id") {
        let session_id = body
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        plugin.close();
        if let Some(session_id) = session_id {
            let _ = remote(
                &document,
                "POST",
                "/workspace/leave",
                None,
                Some(&session_id),
            )?;
        }
        print_stdout(
            &serde_json::to_string_pretty(&json!({
                "error": "invalid_request",
                "message": "session workspace does not match the invitation"
            }))
            .unwrap_or_default(),
        );
        return Ok(1);
    }
    if args.get_flag("json") {
        print_stdout(&serde_json::to_string_pretty(&body).unwrap_or_default());
    } else {
        print_stdout(
            body.get("participant_id")
                .and_then(Value::as_str)
                .unwrap_or(""),
        );
    }
    let kind = hint.get("type").and_then(Value::as_str).unwrap_or("");
    let hold = args.get_flag("hold")
        || (!matches!(kind, "loopback" | "local") && !args.get_flag("no-hold"));
    if hold {
        if let Some(url) = plugin.local_url() {
            print_stdout(&format!("Local: {url}"));
            wait_for_shutdown().await;
        }
    }
    plugin.close();
    Ok(0)
}

async fn call_remote(args: &clap::ArgMatches) -> Result<i32, CommonroomError> {
    let invite = args
        .get_one::<String>("invite")
        .map(String::as_str)
        .unwrap_or("");
    if !is_remote_invitation(invite) {
        return Err(invalid_request("call requires a commonroom:// invitation"));
    }
    let document = invitation_document(invite)?;
    let method = args
        .get_one::<String>("method")
        .map(|value| value.to_ascii_uppercase())
        .unwrap_or_else(|| "GET".into());
    let path = args
        .get_one::<String>("path")
        .map(String::as_str)
        .unwrap_or("");
    if !path.starts_with('/') || path.starts_with("//") {
        return Err(invalid_request("path must be a Commonroom HTTP path"));
    }
    let body = match args.get_one::<String>("body") {
        None => None,
        Some(text) => {
            let value: Value =
                serde_json::from_str(text).map_err(|_| invalid_request("body must be JSON"))?;
            if !value.is_object() {
                return Err(invalid_request("body must be a JSON object"));
            }
            Some(value)
        }
    };
    let (status, payload) = remote(
        &document,
        &method,
        path,
        body.as_ref(),
        args.get_one::<String>("session").map(String::as_str),
    )?;
    print_stdout(
        &serde_json::to_string_pretty(&json!({"status": status, "body": payload}))
            .unwrap_or_default(),
    );
    Ok(if (200..300).contains(&status) { 0 } else { 1 })
}

fn remote(
    document: &Value,
    method: &str,
    path: &str,
    body: Option<&Value>,
    session: Option<&str>,
) -> Result<(u16, Value), CommonroomError> {
    let (mut plugin, mut connection, _hint) = connect_invitation(document)?;
    let headers =
        session.map(|session| vec![("Authorization".into(), format!("Bearer {session}"))]);
    let result = http_exchange(connection.as_mut(), method, path, body, headers.as_deref());
    connection.close();
    plugin.close();
    result
}

fn invitation_document(value: &str) -> Result<Value, CommonroomError> {
    match decode_invitation(value) {
        Ok(document) => Ok(document),
        Err(err) if err.message == "unsupported invitation version" => {
            Err(unsupported_version(err.message))
        }
        Err(err) => Err(err),
    }
}

fn emit(
    payload: &Value,
    as_json: bool,
    render: impl Fn(&Value) -> String,
) -> Result<i32, CommonroomError> {
    if as_json {
        print_stdout(&serde_json::to_string_pretty(payload).unwrap_or_default());
    } else {
        print_stdout(&render(payload));
    }
    Ok(0)
}

fn emit_list<T: Serialize>(
    items: &[T],
    as_json: bool,
    render: impl Fn(&Value) -> String,
) -> Result<i32, CommonroomError> {
    let payload =
        serde_json::to_value(items).map_err(|err| ce_protocol::internal_error(err.to_string()))?;
    emit(&payload, as_json, render)
}

fn status_text(payload: &Value) -> String {
    let listener = payload
        .get("listener")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let kind = listener
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let mode = listener
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("local");
    let encrypted = if listener
        .get("encrypted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "encrypted"
    } else {
        "not encrypted"
    };
    format!(
        "workspace={} version={} path={}\ntransport={} ({}, {})\n",
        payload
            .get("workspace_id")
            .and_then(Value::as_str)
            .unwrap_or(""),
        payload.get("version").and_then(Value::as_i64).unwrap_or(0),
        payload.get("path").and_then(Value::as_str).unwrap_or(""),
        kind,
        mode,
        encrypted
    )
}

fn people_text(payload: &Value) -> String {
    let Some(items) = payload.as_array() else {
        return "(none)\n".into();
    };
    if items.is_empty() {
        return "(none)\n".into();
    }
    items
        .iter()
        .map(|item| {
            format!(
                "{} {} {}\n",
                item.get("kind").and_then(Value::as_str).unwrap_or(""),
                item.get("name").and_then(Value::as_str).unwrap_or(""),
                item.get("participant_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            )
        })
        .collect()
}

fn leases_text(payload: &Value) -> String {
    let Some(items) = payload.as_array() else {
        return "(none)\n".into();
    };
    if items.is_empty() {
        return "(none)\n".into();
    }
    items
        .iter()
        .map(|item| {
            format!(
                "{} {} {}\n",
                item.get("lease_id").and_then(Value::as_str).unwrap_or(""),
                item.get("target").and_then(Value::as_str).unwrap_or(""),
                item.get("participant_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            )
        })
        .collect()
}

fn proposals_text(payload: &Value) -> String {
    let Some(items) = payload.as_array() else {
        return "(none)\n".into();
    };
    if items.is_empty() {
        return "(none)\n".into();
    }
    items
        .iter()
        .map(|item| {
            format!(
                "{} {} base={} {}\n",
                item.get("proposal_id")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                item.get("status").and_then(Value::as_str).unwrap_or(""),
                item.get("base_version")
                    .and_then(Value::as_i64)
                    .unwrap_or(0),
                item.get("summary").and_then(Value::as_str).unwrap_or("")
            )
        })
        .collect()
}

fn history_text(payload: &Value) -> String {
    let Some(items) = payload.as_array() else {
        return String::new();
    };
    items
        .iter()
        .map(|item| {
            format!(
                "{} {} {}\n",
                item.get("event_id").and_then(Value::as_i64).unwrap_or(0),
                item.get("event_type").and_then(Value::as_str).unwrap_or(""),
                item.get("created_at").and_then(Value::as_str).unwrap_or("")
            )
        })
        .collect()
}

fn print_stdout(text: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes());
    if !text.ends_with('\n') {
        let _ = out.write_all(b"\n");
    }
    let _ = out.flush();
}

async fn wait_for_shutdown() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install terminate handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
