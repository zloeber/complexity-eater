//! `ce drop` parent watch and the room child it spawns.
//!
//! The parent inherits nothing published: the watch is two pipes. The event
//! pipe carries the ready line and one terminal line. The lifetime pipe stays
//! open in the parent until that process exits. If the parent dies before a
//! terminal event, the child sees EOF and leaves. After `picked_up` the child
//! closes its event-pipe write end and ignores the lifetime pipe, so the
//! parent's exit does not stop the room.
//!
//! The running drop records its process id beside the markdown file at
//! `<file>.commonroom.drop.pid`. The file contains only that pid. It is not a
//! secret and does not contain the invitation. The child removes it on exit.
//! `ce drop close` sends SIGTERM to the pid in that file.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use ce_core::{DropSignal, WorkspaceEngine};
use ce_protocol::{invalid_request, CommonroomError, Limits};
use clap::ArgMatches;
use serde_json::{json, Value};
use tokio::sync::Notify;

use crate::config::{load_room_file, resolve_room, ServeOverrides};
use crate::room::start_room_invited;

pub async fn run(args: &ArgMatches) -> Result<i32, CommonroomError> {
    if let Some(("close", close_args)) = args.subcommand() {
        return close_drop(close_args);
    }
    if is_drop_child() {
        return run_child(args).await;
    }
    run_parent(args).await
}

fn is_drop_child() -> bool {
    std::env::var_os("CE_DROP_CHILD").is_some() && std::env::var_os("CE_DROP_EVENT_WR").is_some()
}

fn close_drop(args: &ArgMatches) -> Result<i32, CommonroomError> {
    let workspace = args
        .get_one::<String>("workspace")
        .map(String::as_str)
        .unwrap_or("");
    let path = drop_pid_path(Path::new(workspace));
    let text = std::fs::read_to_string(&path)
        .map_err(|_| invalid_request("no drop is running for this workspace"))?;
    let pid: i32 = text
        .trim()
        .parse()
        .map_err(|_| invalid_request("drop pid file is unreadable"))?;
    if pid <= 0 || !pid_alive(pid) {
        let _ = std::fs::remove_file(&path);
        return Err(invalid_request("no drop is running for this workspace"));
    }
    let sent = unsafe { libc::kill(pid, libc::SIGTERM) };
    if sent != 0 {
        return Err(invalid_request("could not signal the drop"));
    }
    Ok(0)
}

async fn run_parent(args: &ArgMatches) -> Result<i32, CommonroomError> {
    let path = args
        .get_one::<String>("path")
        .map(String::as_str)
        .unwrap_or("");
    let name = args
        .get_one::<String>("name")
        .map(String::as_str)
        .unwrap_or("");
    let for_minutes = *args.get_one::<i64>("for").unwrap_or(&60);
    let pickups = *args.get_one::<i64>("pickups").unwrap_or(&1);
    let idle_minutes = *args.get_one::<i64>("idle").unwrap_or(&15);
    let (event_read, event_write) =
        std::io::pipe().map_err(|err| invalid_request(err.to_string()))?;
    let (life_read, life_write) =
        std::io::pipe().map_err(|err| invalid_request(err.to_string()))?;
    unset_cloexec(event_read.as_raw_fd());
    unset_cloexec(event_write.as_raw_fd());
    unset_cloexec(life_read.as_raw_fd());
    unset_cloexec(life_write.as_raw_fd());
    let mut child_args = vec![
        "drop".to_string(),
        path.to_string(),
        "--name".to_string(),
        name.to_string(),
        "--for".to_string(),
        for_minutes.to_string(),
        "--pickups".to_string(),
        pickups.to_string(),
        "--idle".to_string(),
        idle_minutes.to_string(),
    ];
    if args.get_flag("ephemeral") {
        child_args.push("--ephemeral".into());
    }
    if args.get_flag("tailcat") {
        child_args.push("--tailcat".into());
    }
    let exe = std::env::current_exe().map_err(|err| invalid_request(err.to_string()))?;
    let mut command = Command::new(exe);
    command
        .args(&child_args)
        .env("CE_DROP_CHILD", "1")
        .env("CE_DROP_EVENT_RD", event_read.as_raw_fd().to_string())
        .env("CE_DROP_EVENT_WR", event_write.as_raw_fd().to_string())
        .env("CE_DROP_LIFE_RD", life_read.as_raw_fd().to_string())
        .env("CE_DROP_LIFE_WR", life_write.as_raw_fd().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|err| invalid_request(err.to_string()))?;
    drop(event_write);
    drop(life_read);
    let _lifetime = life_write;
    let mut reader = BufReader::new(event_read);
    let ready = match read_json_line(&mut reader) {
        Ok(Some(value)) => value,
        Ok(None) | Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(1);
        }
    };
    let invite = ready
        .get("invite")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let human = ready
        .get("human")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if invite.is_empty() || human.is_empty() {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(1);
    }
    if let Some(banner) = ready.get("banner").and_then(Value::as_str) {
        let mut err = std::io::stderr();
        let _ = err.write_all(banner.as_bytes());
        let _ = err.flush();
    }
    write_stdout_line(&json!({
        "event": "started",
        "pid": child.id(),
        "invite": invite,
        "human": human,
    }))?;
    let terminal = match read_json_line(&mut reader) {
        Ok(Some(value)) => value,
        Ok(None) | Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(1);
        }
    };
    let event = terminal.get("event").and_then(Value::as_str).unwrap_or("");
    let public = match event {
        "picked_up" => json!({
            "event": "picked_up",
            "participant": terminal.get("participant").and_then(Value::as_str).unwrap_or(""),
            "at": terminal.get("at").and_then(Value::as_str).unwrap_or(""),
            "pickups": terminal.get("pickups").and_then(Value::as_i64).unwrap_or(0),
            "pickups_remaining": terminal.get("pickups_remaining").and_then(Value::as_i64).unwrap_or(0),
        }),
        "expired" => json!({"event": "expired"}),
        "closed" => json!({"event": "closed"}),
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(1);
        }
    };
    write_stdout_line(&public)?;
    match event {
        "picked_up" => {
            drop(child);
            Ok(0)
        }
        "expired" => {
            let _ = child.wait();
            Ok(2)
        }
        "closed" => {
            let _ = child.wait();
            Ok(3)
        }
        _ => Ok(1),
    }
}

async fn run_child(args: &ArgMatches) -> Result<i32, CommonroomError> {
    let (mut event, life) = take_child_pipes()?;
    let parent_gone = Arc::new(AtomicBool::new(false));
    let notify = Arc::new(Notify::new());
    let gone = Arc::clone(&parent_gone);
    let wake = Arc::clone(&notify);
    std::thread::spawn(move || {
        let mut reader = life;
        let mut buf = [0u8; 16];
        let _ = reader.read(&mut buf);
        gone.store(true, Ordering::SeqCst);
        wake.notify_one();
    });
    let result = run_child_room(args, &mut event, &parent_gone, &notify).await;
    drop(event);
    result
}

async fn run_child_room(
    args: &ArgMatches,
    event: &mut Option<File>,
    parent_gone: &AtomicBool,
    notify: &Notify,
) -> Result<i32, CommonroomError> {
    let path = args
        .get_one::<String>("path")
        .map(String::as_str)
        .unwrap_or("");
    let name = args
        .get_one::<String>("name")
        .map(String::as_str)
        .unwrap_or("host");
    let for_minutes = *args.get_one::<i64>("for").unwrap_or(&60);
    let pickups = *args.get_one::<i64>("pickups").unwrap_or(&1);
    let idle_minutes = *args.get_one::<i64>("idle").unwrap_or(&15);
    let idle_seconds = idle_minutes
        .checked_mul(60)
        .ok_or_else(|| invalid_request("idle is too large"))?;
    let ephemeral = args.get_flag("ephemeral");
    let db_dir = if ephemeral {
        let dir = std::env::temp_dir().join(format!(
            "ce-drop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|stamp| stamp.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).map_err(|err| invalid_request(err.to_string()))?;
        Some(dir)
    } else {
        None
    };
    let db_path = db_dir.as_ref().map(|dir| dir.join("workspace.db"));
    let engine = match WorkspaceEngine::open(path, db_path, Limits::default(), None) {
        Ok(engine) => Arc::new(engine),
        Err(err) => {
            if let Some(dir) = &db_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            return Err(err);
        }
    };
    let file = load_room_file(Path::new(path), None)?;
    let settings = resolve_room(
        file,
        ServeOverrides {
            mode: None,
            host: Some("127.0.0.1".into()),
            port: Some(0),
            name: Some(name.to_string()),
            transport: args.get_flag("tailcat").then(|| "tailcat".into()),
            baton_max_minutes: None,
            baton_default_minutes: None,
            baton_max_renewals: None,
        },
        name,
    )?;
    let room = async {
        engine.configure_room(
            &settings.mode,
            settings.baton_max_minutes,
            settings.baton_default_minutes,
            settings.baton_max_renewals,
        )?;
        engine.open_drop(for_minutes, pickups, idle_seconds)?;
        start_room_invited(
            Arc::clone(&engine),
            &settings.host,
            settings.port,
            &settings.name,
            settings.transport.as_deref(),
            Some(for_minutes),
        )
        .await
    }
    .await;
    let mut room = match room {
        Ok(room) => room,
        Err(err) => {
            engine.close();
            if let Some(dir) = &db_dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            return Err(err);
        }
    };
    let pid_file = PidFile::create(Path::new(path))?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|err| invalid_request(err.to_string()))?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|err| invalid_request(err.to_string()))?;
    let ready = json!({
        "event": "ready",
        "invite": room.invite.uri,
        "human": room.local_url,
        "banner": room.banner()?,
    });
    if !write_event(event, &ready) {
        room.shutdown().await;
        drop(pid_file);
        engine.close();
        if let Some(dir) = &db_dir {
            let _ = std::fs::remove_dir_all(dir);
        }
        return Ok(1);
    }
    let host = room.host_session.clone();
    watch(
        Arc::clone(&engine),
        &host,
        event,
        parent_gone,
        notify,
        &mut term,
        &mut interrupt,
    )
    .await;
    room.shutdown().await;
    drop(pid_file);
    engine.close();
    if let Some(dir) = db_dir {
        let _ = std::fs::remove_dir_all(dir);
    }
    Ok(0)
}

async fn watch(
    engine: Arc<WorkspaceEngine>,
    host: &ce_core::Session,
    event: &mut Option<File>,
    parent_gone: &AtomicBool,
    notify: &Notify,
    term: &mut tokio::signal::unix::Signal,
    interrupt: &mut tokio::signal::unix::Signal,
) {
    let mut sent = false;
    loop {
        if parent_gone.load(Ordering::SeqCst) && !sent {
            return;
        }
        tokio::select! {
            _ = notify.notified(), if !sent => {
                if parent_gone.load(Ordering::SeqCst) && !sent {
                    return;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {
                if parent_gone.load(Ordering::SeqCst) && !sent {
                    return;
                }
                match engine.drop_tick() {
                    Ok(Some(tick)) => {
                        if !sent {
                            if let Some(signal) = tick.signal {
                                let wrote = send_terminal(event, &signal);
                                sent = true;
                                if !wrote || tick.exit {
                                    return;
                                }
                            }
                        } else if tick.exit {
                            return;
                        }
                    }
                    Ok(None) | Err(_) => return,
                }
            }
            _ = term.recv() => {
                finish_for_signal(&engine, host, event, &mut sent);
                return;
            }
            _ = interrupt.recv() => {
                finish_for_signal(&engine, host, event, &mut sent);
                return;
            }
        }
    }
}

fn finish_for_signal(
    engine: &WorkspaceEngine,
    host: &ce_core::Session,
    event: &mut Option<File>,
    sent: &mut bool,
) {
    if *sent {
        return;
    }
    let _ = engine.drop_close(host);
    let _ = send_terminal(event, &DropSignal::Closed);
    *sent = true;
}

fn send_terminal(event: &mut Option<File>, signal: &DropSignal) -> bool {
    let value = match signal {
        DropSignal::PickedUp(pickup) => json!({
            "event": "picked_up",
            "participant": pickup.participant,
            "at": pickup.at,
            "pickups": pickup.pickups,
            "pickups_remaining": pickup.pickups_remaining,
        }),
        DropSignal::Expired => json!({"event": "expired"}),
        DropSignal::Closed => json!({"event": "closed"}),
    };
    write_event(event, &value)
}

fn write_event(event: &mut Option<File>, value: &Value) -> bool {
    let mut line = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(_) => {
            *event = None;
            return false;
        }
    };
    line.push(b'\n');
    let ok = match event.as_mut() {
        Some(file) => file.write_all(&line).is_ok() && file.flush().is_ok(),
        None => false,
    };
    if !ok || value.get("event").and_then(Value::as_str) != Some("ready") {
        *event = None;
    }
    ok
}

fn read_json_line(reader: &mut BufReader<impl Read>) -> std::io::Result<Option<Value>> {
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        return Ok(None);
    }
    match serde_json::from_str(line.trim()) {
        Ok(value) => Ok(Some(value)),
        Err(_) => Ok(None),
    }
}

fn write_stdout_line(value: &Value) -> Result<(), CommonroomError> {
    let line =
        serde_json::to_string(value).map_err(|err| ce_protocol::internal_error(err.to_string()))?;
    let mut out = std::io::stdout();
    writeln!(out, "{line}").map_err(|err| invalid_request(err.to_string()))?;
    out.flush()
        .map_err(|err| invalid_request(err.to_string()))?;
    Ok(())
}

fn take_child_pipes() -> Result<(Option<File>, File), CommonroomError> {
    let event_rd = parse_fd("CE_DROP_EVENT_RD")?;
    let event_wr = parse_fd("CE_DROP_EVENT_WR")?;
    let life_rd = parse_fd("CE_DROP_LIFE_RD")?;
    let life_wr = parse_fd("CE_DROP_LIFE_WR")?;
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        libc::close(event_rd);
        libc::close(life_wr);
    }
    let event = unsafe { File::from_raw_fd(event_wr) };
    let life = unsafe { File::from_raw_fd(life_rd) };
    cloexec(event.as_raw_fd());
    cloexec(life.as_raw_fd());
    for key in [
        "CE_DROP_CHILD",
        "CE_DROP_EVENT_RD",
        "CE_DROP_EVENT_WR",
        "CE_DROP_LIFE_RD",
        "CE_DROP_LIFE_WR",
    ] {
        std::env::remove_var(key);
    }
    Ok((Some(event), life))
}

fn parse_fd(key: &str) -> Result<RawFd, CommonroomError> {
    let raw = std::env::var(key).map_err(|_| invalid_request("drop watch pipe is missing"))?;
    raw.parse::<RawFd>()
        .map_err(|_| invalid_request("drop watch pipe is missing"))
}

fn unset_cloexec(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC);
        }
    }
}

fn cloexec(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
        }
    }
}

fn pid_alive(pid: i32) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// `<workspace>.commonroom.drop.pid` beside the markdown file.
fn drop_pid_path(workspace: &Path) -> PathBuf {
    let mut name = workspace
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "workspace".into());
    name.push(".commonroom.drop.pid");
    workspace.with_file_name(name)
}

struct PidFile {
    path: PathBuf,
    pid: u32,
}

impl PidFile {
    fn create(workspace: &Path) -> Result<Self, CommonroomError> {
        let path = drop_pid_path(workspace);
        let pid = std::process::id();
        std::fs::write(&path, format!("{pid}\n"))
            .map_err(|err| invalid_request(err.to_string()))?;
        Ok(Self { path, pid })
    }
}

impl Drop for PidFile {
    fn drop(&mut self) {
        if let Ok(text) = std::fs::read_to_string(&self.path) {
            if text.trim() == self.pid.to_string() {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}
