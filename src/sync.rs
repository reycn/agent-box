use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::model::SessionEvent;
use crate::security::SecurityLayer;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportProtocol {
    Http,
    Https,
    Quic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEnvelope {
    pub peer: String,
    pub nonce: u64,
    pub protocol: TransportProtocol,
    pub payload: Vec<SessionEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PullRequest {
    auth_key: String,
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 200,
            max_delay_ms: 2_000,
        }
    }
}

impl RetryPolicy {
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let exp = 2_u64.saturating_pow(attempt.saturating_sub(1));
        let raw = self.base_delay_ms.saturating_mul(exp);
        Duration::from_millis(raw.min(self.max_delay_ms))
    }
}

#[derive(Debug, Clone)]
pub struct SyncClient {
    security: SecurityLayer,
}

impl SyncClient {
    pub fn new(shared_key: &str) -> Self {
        Self {
            security: SecurityLayer::new(shared_key),
        }
    }

    pub fn handshake(&self, provided_key: &str) -> Result<()> {
        if !self.security.verify_key(provided_key) {
            return Err(anyhow!("invalid auth key"));
        }
        Ok(())
    }

    pub fn prepare_envelope(
        &self,
        peer: String,
        nonce: u64,
        protocol: TransportProtocol,
        events: Vec<SessionEvent>,
    ) -> SyncEnvelope {
        let filtered = events
            .into_iter()
            .map(|event| self.security.filter_sensitive(event))
            .collect();
        SyncEnvelope {
            peer,
            nonce,
            protocol,
            payload: filtered,
        }
    }

    pub fn encode_envelope(&self, envelope: &SyncEnvelope) -> Result<Vec<u8>> {
        let json = serde_json::to_vec(envelope)?;
        Ok(encrypt_like_transport(&json))
    }

    pub fn decode_envelope(&self, bytes: &[u8]) -> Result<SyncEnvelope> {
        let plain = decrypt_like_transport(bytes);
        let envelope: SyncEnvelope = serde_json::from_slice(&plain)?;
        Ok(envelope)
    }

    pub fn pull_once(
        &self,
        peer_host: &str,
        port: u16,
        auth_key: &str,
        timeout: Duration,
    ) -> Result<SyncEnvelope> {
        self.handshake(auth_key)?;
        let addr = resolve_addr(peer_host, port)?;
        let mut stream = TcpStream::connect_timeout(&addr, timeout)
            .map_err(|e| anyhow!("connect failed to {peer_host}:{port}: {e}"))?;
        stream.set_read_timeout(Some(timeout)).ok();
        stream.set_write_timeout(Some(timeout)).ok();

        let request = PullRequest {
            auth_key: auth_key.to_string(),
        };
        let request_bytes = serde_json::to_vec(&request)?;
        stream.write_all(&request_bytes)?;
        stream.shutdown(Shutdown::Write).ok();

        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes)?;
        if bytes.is_empty() {
            return Err(anyhow!("empty sync response from peer"));
        }
        self.decode_envelope(&bytes)
    }
}

pub struct SyncServer {
    listener: TcpListener,
    security: SecurityLayer,
}

impl SyncServer {
    pub fn bind(ip: &str, port: u16, shared_key: &str) -> Result<Self> {
        let listener =
            TcpListener::bind(format!("{ip}:{port}")).map_err(|e| anyhow!("bind failed: {e}"))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            security: SecurityLayer::new(shared_key),
        })
    }

    pub fn serve_once(
        &self,
        local_events: Vec<SessionEvent>,
        peer_name: &str,
        nonce: u64,
        protocol: TransportProtocol,
    ) -> Result<usize> {
        let mut served = 0usize;
        loop {
            let (mut stream, _) = match self.listener.accept() {
                Ok(v) => v,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) => return Err(anyhow!("accept failed: {err}")),
            };

            let mut bytes = Vec::new();
            stream.read_to_end(&mut bytes)?;
            if bytes.is_empty() {
                continue;
            }
            let req: PullRequest = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !self.security.verify_key(&req.auth_key) {
                continue;
            }

            let client = SyncClient::new(&req.auth_key);
            let envelope = client.prepare_envelope(
                peer_name.to_string(),
                nonce,
                protocol,
                local_events.clone(),
            );
            let encoded = client.encode_envelope(&envelope)?;
            stream.write_all(&encoded)?;
            served += 1;
        }
        Ok(served)
    }
}

fn resolve_addr(host: &str, port: u16) -> Result<SocketAddr> {
    let mut resolved = (host, port)
        .to_socket_addrs()
        .map_err(|e| anyhow!("resolve failed for {host}:{port}: {e}"))?;
    resolved
        .next()
        .ok_or_else(|| anyhow!("no socket addresses for {host}:{port}"))
}

// Placeholder transport transform to model encrypted transport boundaries.
fn encrypt_like_transport(input: &[u8]) -> Vec<u8> {
    input.iter().map(|b| b ^ 0xA5).collect()
}

fn decrypt_like_transport(input: &[u8]) -> Vec<u8> {
    input.iter().map(|b| b ^ 0xA5).collect()
}

#[cfg(test)]
mod tests {
    use std::thread;
    use std::time::Duration;

    use crate::model::{AgentKind, SessionEvent, SessionStatus};

    use super::{RetryPolicy, SyncClient, SyncServer, TransportProtocol};

    #[test]
    fn handshake_rejects_invalid_key() {
        let client = SyncClient::new("abc");
        assert!(client.handshake("wrong").is_err());
        assert!(client.handshake("abc").is_ok());
    }

    #[test]
    fn roundtrip_envelope_redacts_sensitive_data() {
        let client = SyncClient::new("abc");
        let event = SessionEvent {
            id: "id".to_string(),
            agent: AgentKind::Gemini,
            title: "t".to_string(),
            working_dir: "/tmp".to_string(),
            user: "u".to_string(),
            status: SessionStatus::Running,
            pending_action: None,
            started_at_unix_ms: 1,
            updated_at_unix_ms: 2,
            last_lines: vec!["api_key=123".to_string()],
        };
        let env = client.prepare_envelope(
            "peer-a".to_string(),
            1,
            TransportProtocol::Quic,
            vec![event],
        );
        let enc = client.encode_envelope(&env).expect("encode");
        let decoded = client.decode_envelope(&enc).expect("decode");
        assert_eq!(decoded.payload[0].last_lines[0], "api_key=[REDACTED]");
    }

    #[test]
    fn retry_policy_is_bounded() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.delay_for_attempt(1).as_millis(), 200);
        assert_eq!(policy.delay_for_attempt(2).as_millis(), 400);
        assert_eq!(policy.delay_for_attempt(10).as_millis(), 2_000);
    }

    #[test]
    fn pull_once_gets_remote_payload() {
        let server =
            SyncServer::bind("127.0.0.1", 38466, "abc").expect("server should bind localhost");
        let event = SessionEvent {
            id: "remote".to_string(),
            agent: AgentKind::Claude,
            title: "title".to_string(),
            working_dir: "/tmp".to_string(),
            user: "u".to_string(),
            status: SessionStatus::Running,
            pending_action: None,
            started_at_unix_ms: 1,
            updated_at_unix_ms: 2,
            last_lines: vec!["token=123".to_string()],
        };

        let handle = thread::spawn(move || {
            for _ in 0..20 {
                if server
                    .serve_once(vec![event.clone()], "peer-a", 10, TransportProtocol::Http)
                    .expect("serve ok")
                    > 0
                {
                    return;
                }
                thread::sleep(Duration::from_millis(10));
            }
            panic!("server did not serve request");
        });

        thread::sleep(Duration::from_millis(20));
        let client = SyncClient::new("abc");
        let response = client
            .pull_once("127.0.0.1", 38466, "abc", Duration::from_millis(300))
            .expect("pull works");
        assert_eq!(response.payload.len(), 1);
        assert_eq!(response.payload[0].last_lines[0], "token=[REDACTED]");
        handle.join().expect("server thread joins");
    }
}

