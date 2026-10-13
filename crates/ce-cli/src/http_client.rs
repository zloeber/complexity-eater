use ce_protocol::CommonroomError;
use ce_transport::Connection;
use serde_json::{json, Value};

pub fn http_exchange(
    connection: &mut dyn Connection,
    method: &str,
    path: &str,
    json_body: Option<&Value>,
    headers: Option<&[(String, String)]>,
) -> Result<(u16, Value), CommonroomError> {
    let mut payload = Vec::new();
    let mut header_map = vec![
        ("Host".to_string(), "commonroom".to_string()),
        ("Connection".to_string(), "close".to_string()),
        ("Accept".to_string(), "application/json".to_string()),
    ];
    if let Some(body) = json_body {
        payload = serde_json::to_vec(body).unwrap_or_default();
        header_map.push(("Content-Type".to_string(), "application/json".to_string()));
        header_map.push(("Content-Length".to_string(), payload.len().to_string()));
    }
    if let Some(headers) = headers {
        for (key, value) in headers {
            if let Some(existing) = header_map
                .iter_mut()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
            {
                existing.1.clone_from(value);
            } else {
                header_map.push((key.clone(), value.clone()));
            }
        }
    }
    let mut raw = format!("{method} {path} HTTP/1.1\r\n");
    for (key, value) in &header_map {
        raw.push_str(key);
        raw.push_str(": ");
        raw.push_str(value);
        raw.push_str("\r\n");
    }
    raw.push_str("\r\n");
    let mut bytes = raw.into_bytes();
    bytes.extend_from_slice(&payload);
    connection.send(&bytes)?;
    let data = read_http(connection, 2_000_000);
    Ok(parse_http(&data))
}

fn read_http(connection: &mut dyn Connection, max_bytes: usize) -> Vec<u8> {
    let mut blob = Vec::new();
    while blob.len() < max_bytes {
        let chunk = match connection.receive(65536) {
            Ok(chunk) if !chunk.is_empty() => chunk,
            _ => break,
        };
        blob.extend_from_slice(&chunk);
        if response_complete(&blob) {
            break;
        }
    }
    blob
}

fn response_complete(blob: &[u8]) -> bool {
    let Some(split) = find_header_end(blob) else {
        return false;
    };
    let Some(length) = content_length(&blob[..split]) else {
        return false;
    };
    blob.len() >= split + 4 + length
}

fn find_header_end(blob: &[u8]) -> Option<usize> {
    blob.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(head: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(head);
    for line in text.split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn parse_http(blob: &[u8]) -> (u16, Value) {
    let Some(split) = find_header_end(blob) else {
        return (0, json!({"message": String::from_utf8_lossy(blob)}));
    };
    let head = &blob[..split];
    let mut body = &blob[split + 4..];
    let status_line = head.split(|byte| *byte == b'\r').next().unwrap_or(head);
    let status_line = String::from_utf8_lossy(status_line);
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    if let Some(length) = content_length(head) {
        body = &body[..body.len().min(length)];
    }
    if body.is_empty() {
        return (status, json!({}));
    }
    match serde_json::from_slice(body) {
        Ok(value) => (status, value),
        Err(_) => (status, json!({"message": String::from_utf8_lossy(body)})),
    }
}

pub struct ChunkDecoder {
    pending: Vec<u8>,
    remaining: Option<usize>,
    done: bool,
}

impl ChunkDecoder {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            remaining: None,
            done: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn push(&mut self, data: &[u8]) -> Vec<u8> {
        if data.is_empty() || self.done {
            return Vec::new();
        }
        self.pending.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            if self.done {
                break;
            }
            if let Some(remaining) = self.remaining {
                if remaining == 0 {
                    if self.pending.len() < 2 {
                        break;
                    }
                    if self.pending.starts_with(b"\r\n") {
                        self.pending.drain(..2);
                        self.remaining = None;
                        continue;
                    }
                    break;
                }
                let take = remaining.min(self.pending.len());
                if take == 0 {
                    break;
                }
                out.extend_from_slice(&self.pending[..take]);
                self.pending.drain(..take);
                self.remaining = Some(remaining - take);
                continue;
            }
            let Some(split) = self.pending.windows(2).position(|window| window == b"\r\n") else {
                break;
            };
            let line = String::from_utf8_lossy(&self.pending[..split]).to_string();
            self.pending.drain(..split + 2);
            let size_text = line.split(';').next().unwrap_or("0").trim();
            let size = usize::from_str_radix(size_text, 16).unwrap_or(0);
            if size == 0 {
                self.done = true;
                break;
            }
            self.remaining = Some(size);
        }
        out
    }
}

pub fn take_sse_json(buffer: &mut String) -> Vec<Value> {
    let mut messages = Vec::new();
    while let Some(split) = buffer.find("\n\n") {
        let frame: String = buffer.drain(..split + 2).collect();
        let mut data = String::new();
        for line in frame.split('\n') {
            let line = line.trim_end_matches('\r');
            if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        if data.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            if value.is_object() {
                messages.push(value);
            }
        }
    }
    messages
}

pub fn header_complete(blob: &[u8]) -> Option<(u16, bool, Option<usize>, Vec<u8>)> {
    let split = find_header_end(blob)?;
    let head = &blob[..split];
    let status_line = head.split(|byte| *byte == b'\r').next().unwrap_or(head);
    let status_line = String::from_utf8_lossy(status_line);
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut chunked = false;
    for line in String::from_utf8_lossy(head).split("\r\n").skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
        {
            chunked = true;
        }
    }
    let length = content_length(head);
    let body = blob[split + 4..].to_vec();
    Some((status, chunked, length, body))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::time::Duration;

    use ce_transport::SocketConnection;
    use serde_json::json;

    use super::http_exchange;

    #[test]
    fn exchange_reads_a_content_length_response() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            sock.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 4096];
            let n = sock.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let body = br#"{"session_id":"ses_test","workspace_id":"workspace_x"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(resp.as_bytes()).unwrap();
            sock.write_all(body).unwrap();
            req
        });
        let sock = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut connection = SocketConnection::new(sock, Duration::from_secs(2));
        let (status, body) = http_exchange(
            &mut connection,
            "POST",
            "/session/join",
            Some(&json!({"invite": "x"})),
            None,
        )
        .unwrap();
        let req = server.join().unwrap();
        assert!(req.contains("POST /session/join HTTP/1.1"), "{req}");
        assert!(req.contains("Host: commonroom"), "{req}");
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["session_id"], "ses_test");
    }

    #[test]
    fn chunk_decoder_reassembles_split_chunks() {
        let mut decoder = super::ChunkDecoder::new();
        assert!(decoder.push(b"5\r").is_empty());
        assert_eq!(decoder.push(b"\nhel"), b"hel");
        let rest = decoder.push(b"lo\r\n0\r\n\r\n");
        assert_eq!(rest, b"lo");
        assert!(decoder.is_done());
    }

    #[test]
    fn sse_frames_ignore_comments_and_keep_partial_json() {
        let mut buffer = ": keepalive\n\ndata: {\"method\":\"notifications/resources/updated\"}\n\ndata: {\"method\"".to_string();
        let messages = super::take_sse_json(&mut buffer);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["method"], "notifications/resources/updated");
        assert!(buffer.starts_with("data:"));
    }
}
