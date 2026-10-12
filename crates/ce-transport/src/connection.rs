use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::Duration;

use ce_protocol::{transport_unavailable, CommonroomError};

pub trait Connection: Send {
    fn send(&mut self, data: &[u8]) -> Result<(), CommonroomError>;
    fn receive(&mut self, max_bytes: usize) -> Result<Vec<u8>, CommonroomError>;
    fn close(&mut self);
}

pub struct SocketConnection {
    sock: Option<TcpStream>,
}

impl SocketConnection {
    pub fn new(sock: TcpStream, timeout: Duration) -> Self {
        let _ = sock.set_read_timeout(Some(timeout));
        let _ = sock.set_write_timeout(Some(timeout));
        let _ = sock.set_nodelay(true);
        Self { sock: Some(sock) }
    }
}

impl Connection for SocketConnection {
    fn send(&mut self, data: &[u8]) -> Result<(), CommonroomError> {
        let sock = self
            .sock
            .as_mut()
            .ok_or_else(|| transport_unavailable("connection is closed"))?;
        let mut view = data;
        while !view.is_empty() {
            match sock.write(view) {
                Ok(0) => return Err(transport_unavailable("connection is closed")),
                Ok(sent) => view = &view[sent..],
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err(transport_unavailable("send failed")),
            }
        }
        sock.flush()
            .map_err(|_| transport_unavailable("send failed"))?;
        Ok(())
    }

    fn receive(&mut self, max_bytes: usize) -> Result<Vec<u8>, CommonroomError> {
        let sock = self
            .sock
            .as_mut()
            .ok_or_else(|| transport_unavailable("connection is closed"))?;
        let mut buf = vec![0u8; max_bytes.max(1)];
        match sock.read(&mut buf) {
            Ok(0) => Ok(Vec::new()),
            Ok(n) => {
                buf.truncate(n);
                Ok(buf)
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::TimedOut
                    || err.kind() == std::io::ErrorKind::WouldBlock =>
            {
                Err(transport_unavailable("receive timed out"))
            }
            Err(_) => Err(transport_unavailable("receive failed")),
        }
    }

    fn close(&mut self) {
        if let Some(sock) = self.sock.take() {
            let _ = sock.shutdown(Shutdown::Both);
        }
    }
}

pub fn read_exact(
    connection: &mut dyn Connection,
    size: usize,
) -> Result<Vec<u8>, CommonroomError> {
    let mut collected = Vec::with_capacity(size);
    while collected.len() < size {
        let chunk = connection.receive((size - collected.len()).min(65536))?;
        if chunk.is_empty() {
            return Err(transport_unavailable(
                "connection closed before the payload finished",
            ));
        }
        collected.extend_from_slice(&chunk);
    }
    collected.truncate(size);
    Ok(collected)
}
