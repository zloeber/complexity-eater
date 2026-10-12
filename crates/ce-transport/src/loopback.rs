use std::collections::BTreeSet;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use ce_protocol::{transport_unavailable, CommonroomError};
use serde_json::{json, Value};

use crate::connection::{Connection, SocketConnection};

pub trait Transport: Send {
    fn listen(&mut self) -> Result<Value, CommonroomError>;
    fn connect(&mut self, description: &Value) -> Result<Box<dyn Connection>, CommonroomError>;
    fn accept(&mut self) -> Result<Box<dyn Connection>, CommonroomError>;
    fn close(&mut self);
    fn describe(&self) -> Result<Value, CommonroomError>;
    fn capabilities(&self) -> BTreeSet<&'static str>;
    fn expose(&mut self, port: u16) -> Result<Value, CommonroomError> {
        let _ = port;
        Err(transport_unavailable(
            "transport plugin cannot expose a local port",
        ))
    }
    fn local_url(&self) -> Option<String> {
        None
    }
}

pub struct LoopbackTransport {
    timeout: Duration,
    listener: Option<TcpListener>,
    description: Option<Value>,
}

impl LoopbackTransport {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            listener: None,
            description: None,
        }
    }
}

impl Transport for LoopbackTransport {
    fn capabilities(&self) -> BTreeSet<&'static str> {
        [
            "streaming",
            "bidirectional",
            "large_file_transfer",
            "multiplexing",
        ]
        .into_iter()
        .collect()
    }

    fn listen(&mut self) -> Result<Value, CommonroomError> {
        let listener =
            TcpListener::bind("127.0.0.1:0").map_err(|_| transport_unavailable("listen failed"))?;
        let _ = listener.set_nonblocking(false);
        let port = listener
            .local_addr()
            .map_err(|_| transport_unavailable("listen failed"))?
            .port();
        self.listener = Some(listener);
        self.description = Some(json!({
            "type": "loopback",
            "address": "127.0.0.1",
            "port": port,
            "encrypted": false,
            "mode": "local",
        }));
        self.describe()
    }

    fn expose(&mut self, port: u16) -> Result<Value, CommonroomError> {
        self.description = Some(json!({
            "type": "loopback",
            "address": "127.0.0.1",
            "port": port,
            "encrypted": false,
            "mode": "local",
        }));
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
        let host = description
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or("127.0.0.1");
        let port = description
            .get("port")
            .and_then(Value::as_u64)
            .ok_or_else(|| transport_unavailable("loopback description is missing a port"))?;
        let sock = TcpStream::connect_timeout(
            &format!("{host}:{port}")
                .parse()
                .map_err(|_| transport_unavailable("connect failed"))?,
            self.timeout,
        )
        .map_err(|_| transport_unavailable("connect failed"))?;
        Ok(Box::new(SocketConnection::new(sock, self.timeout)))
    }

    fn close(&mut self) {
        self.listener.take();
    }

    fn describe(&self) -> Result<Value, CommonroomError> {
        self.description
            .clone()
            .ok_or_else(|| transport_unavailable("transport is not listening"))
    }
}
