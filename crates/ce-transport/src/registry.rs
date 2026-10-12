use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use ce_protocol::{transport_unavailable, CommonroomError};
use serde_json::Value;

use crate::connection::Connection;
use crate::loopback::{LoopbackTransport, Transport};
use crate::tailcat::TailcatTransport;

type Factory = Box<dyn Fn() -> Box<dyn Transport> + Send + Sync>;

fn factories() -> &'static Mutex<HashMap<String, Factory>> {
    static FACTORIES: OnceLock<Mutex<HashMap<String, Factory>>> = OnceLock::new();
    FACTORIES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn ensure_builtins() {
    let mut guard = factories().lock().expect("transports");
    if !guard.contains_key("loopback") {
        guard.insert(
            "loopback".into(),
            Box::new(|| Box::new(LoopbackTransport::new(Duration::from_secs(5)))),
        );
    }
    if !guard.contains_key("tailcat") {
        guard.insert(
            "tailcat".into(),
            Box::new(|| Box::new(TailcatTransport::new(None, 20.0))),
        );
    }
}

pub fn register_transport<F>(name: &str, factory: F)
where
    F: Fn() -> Box<dyn Transport> + Send + Sync + 'static,
{
    factories()
        .lock()
        .expect("transports")
        .insert(name.to_string(), Box::new(factory));
}

pub fn registered_transports() -> Vec<String> {
    ensure_builtins();
    let mut names: Vec<_> = factories()
        .lock()
        .expect("transports")
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

pub fn create_transport(name: &str) -> Result<Box<dyn Transport>, CommonroomError> {
    ensure_builtins();
    let guard = factories().lock().expect("transports");
    if let Some(factory) = guard.get(name) {
        return Ok(factory());
    }
    Err(
        transport_unavailable(format!("no transport plugin named '{name}' is installed"))
            .detail("transport", name)
            .detail("entry_point_group", "commonroom.transports"),
    )
}

pub fn invitation_hints(document: &Value) -> Vec<Value> {
    if let Some(hints) = document.get("transports").and_then(Value::as_array) {
        let hints: Vec<_> = hints
            .iter()
            .filter(|hint| hint.is_object())
            .cloned()
            .collect();
        if !hints.is_empty() {
            return hints;
        }
    }
    if let Some(hint) = document.get("transport") {
        if hint.is_object() {
            return vec![hint.clone()];
        }
    }
    Vec::new()
}

pub type InvitationLink = (Box<dyn Transport>, Box<dyn Connection>, Value);

pub fn connect_invitation(document: &Value) -> Result<InvitationLink, CommonroomError> {
    let mut errors = Vec::new();
    for hint in invitation_hints(document) {
        let name = hint.get("type").and_then(Value::as_str).unwrap_or("");
        let mut plugin = match create_transport(name) {
            Ok(plugin) => plugin,
            Err(err) => {
                errors.push(err.message);
                continue;
            }
        };
        match plugin.connect(&hint) {
            Ok(connection) => return Ok((plugin, connection, hint)),
            Err(err) => {
                plugin.close();
                return Err(err);
            }
        }
    }
    Err(
        transport_unavailable("no advertised transport plugin is available")
            .detail("tried", errors),
    )
}

pub fn create_loopback(timeout: Duration) -> LoopbackTransport {
    LoopbackTransport::new(timeout)
}
