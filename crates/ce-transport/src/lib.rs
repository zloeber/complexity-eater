mod connection;
mod loopback;
mod registry;
mod tailcat;

pub use connection::{read_exact, Connection, SocketConnection};
pub use loopback::{LoopbackTransport, Transport};
pub use registry::{
    connect_invitation, create_loopback, create_transport, invitation_hints, register_transport,
    registered_transports,
};
pub use tailcat::{parse_endpoint, redact, TailcatTransport};

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut framed = (payload.len() as u32).to_be_bytes().to_vec();
        framed.extend_from_slice(payload);
        framed
    }

    #[test]
    fn loopback_round_trip_timeout_and_reconnect() {
        let mut transport = create_loopback(Duration::from_millis(300));
        let endpoint = transport.listen().unwrap();
        let err = match transport.accept() {
            Err(err) => err,
            Ok(_) => panic!("expected accept to time out"),
        };
        assert!(err.message.contains("timed out"), "{}", err.message);

        let endpoint_clone = endpoint.clone();
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..2 {
                let mut connection = transport.accept().unwrap();
                let size = u32::from_be_bytes(
                    read_exact(connection.as_mut(), 4)
                        .unwrap()
                        .try_into()
                        .unwrap(),
                ) as usize;
                let body = read_exact(connection.as_mut(), size).unwrap();
                seen.push(body);
                connection.send(b"ack").unwrap();
                connection.close();
            }
            seen
        });
        for payload in [b"one".as_slice(), b"two".as_slice()] {
            let mut client = create_transport("loopback").unwrap();
            let mut connection = client.connect(&endpoint_clone).unwrap();
            connection.send(&frame(payload)).unwrap();
            assert_eq!(read_exact(connection.as_mut(), 3).unwrap(), b"ack");
            connection.close();
            assert!(connection.send(b"again").is_err());
        }
        let seen = server.join().unwrap();
        assert_eq!(seen, vec![b"one".to_vec(), b"two".to_vec()]);

        let mut missing = create_transport("loopback").unwrap();
        assert!(missing
            .connect(&json!({"type": "loopback", "address": "127.0.0.1", "port": 1}))
            .is_err());
    }

    #[test]
    fn loopback_large_concurrent_payloads() {
        let mut transport = create_loopback(Duration::from_secs(5));
        let endpoint = transport.listen().unwrap();
        let server = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..2 {
                let mut connection = transport.accept().unwrap();
                let size = u32::from_be_bytes(
                    read_exact(connection.as_mut(), 4)
                        .unwrap()
                        .try_into()
                        .unwrap(),
                ) as usize;
                seen.push(read_exact(connection.as_mut(), size).unwrap());
                connection.send(b"ack").unwrap();
                connection.close();
            }
            seen
        });
        let payloads = [vec![b'a'; 200_000], vec![b'b'; 180_000]];
        let mut threads = Vec::new();
        for payload in payloads.clone() {
            let endpoint = endpoint.clone();
            threads.push(std::thread::spawn(move || {
                let mut client = create_transport("loopback").unwrap();
                let mut connection = client.connect(&endpoint).unwrap();
                connection.send(&frame(&payload)).unwrap();
                assert_eq!(read_exact(connection.as_mut(), 3).unwrap(), b"ack");
                connection.close();
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let mut seen = server.join().unwrap();
        seen.sort();
        let mut expected = payloads.to_vec();
        expected.sort();
        assert_eq!(seen, expected);
    }

    #[test]
    fn custom_plugin_registers_without_the_engine() {
        struct Marker;
        impl Transport for Marker {
            fn listen(&mut self) -> Result<serde_json::Value, ce_protocol::CommonroomError> {
                Ok(json!({"type": "marker"}))
            }
            fn connect(
                &mut self,
                _description: &serde_json::Value,
            ) -> Result<Box<dyn Connection>, ce_protocol::CommonroomError> {
                Err(ce_protocol::transport_unavailable(
                    "marker does not dial in this test",
                ))
            }
            fn accept(&mut self) -> Result<Box<dyn Connection>, ce_protocol::CommonroomError> {
                Err(ce_protocol::transport_unavailable("marker has no listener"))
            }
            fn close(&mut self) {}
            fn describe(&self) -> Result<serde_json::Value, ce_protocol::CommonroomError> {
                Ok(json!({"type": "marker"}))
            }
            fn capabilities(&self) -> std::collections::BTreeSet<&'static str> {
                ["streaming"].into_iter().collect()
            }
        }
        register_transport("marker", || Box::new(Marker));
        let plugin = create_transport("marker").unwrap();
        assert_eq!(plugin.describe().unwrap()["type"], "marker");
        let mut plugin = create_transport("marker").unwrap();
        assert!(plugin.connect(&json!({"type": "marker"})).is_err());
    }

    #[test]
    fn core_crates_do_not_name_transport_vendors() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for relative in [
            "crates/ce-core/src",
            "crates/ce-http/src",
            "crates/ce-protocol/src",
        ] {
            for entry in walk(&root.join(relative)) {
                let source = std::fs::read_to_string(&entry)
                    .unwrap()
                    .to_ascii_lowercase();
                for vendor in ["tailcat", "libp2p", "iroh", "webrtc"] {
                    assert!(
                        !source.contains(vendor),
                        "{} names {vendor}",
                        entry.display()
                    );
                }
            }
        }
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    files.extend(walk(&path));
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                    files.push(path);
                }
            }
        }
        files
    }
}
