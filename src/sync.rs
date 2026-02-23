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
    use crate::model::{AgentKind, SessionEvent, SessionStatus};

    use super::{RetryPolicy, SyncClient, TransportProtocol};

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
}

