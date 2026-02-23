use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::model::SessionEvent;

#[derive(Debug, Clone)]
pub struct SecurityLayer {
    key_hash: String,
}

impl SecurityLayer {
    pub fn new(shared_key: &str) -> Self {
        Self {
            key_hash: hash_key(shared_key),
        }
    }

    pub fn verify_key(&self, provided_key: &str) -> bool {
        hash_key(provided_key) == self.key_hash
    }

    pub fn filter_sensitive(&self, mut event: SessionEvent) -> SessionEvent {
        event.last_lines = event
            .last_lines
            .iter()
            .map(|line| redact_line(line))
            .collect();
        event
    }
}

fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn generate_passkey_sha1(host_name: &str, session_unix_ms: u64, random_seed: u64) -> String {
    let mut hasher = Sha1::new();
    hasher.update(host_name.as_bytes());
    hasher.update(b":");
    hasher.update(session_unix_ms.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(random_seed.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

fn redact_line(line: &str) -> String {
    let mut out = line.to_string();
    let suspects = ["api_key=", "token=", "password=", "secret="];
    for suspect in suspects {
        if let Some(idx) = out.to_lowercase().find(suspect) {
            let prefix_len = idx + suspect.len();
            let prefix = &out[..prefix_len];
            out = format!("{prefix}[REDACTED]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::model::{AgentKind, SessionEvent, SessionStatus};

    use super::{generate_passkey_sha1, SecurityLayer};

    #[test]
    fn verifies_key() {
        let sec = SecurityLayer::new("abc");
        assert!(sec.verify_key("abc"));
        assert!(!sec.verify_key("abcd"));
    }

    #[test]
    fn redacts_secret_lines() {
        let sec = SecurityLayer::new("abc");
        let event = SessionEvent {
            id: "id".to_string(),
            agent: AgentKind::Claude,
            title: "t".to_string(),
            working_dir: "/tmp".to_string(),
            user: "u".to_string(),
            status: SessionStatus::Running,
            pending_action: None,
            started_at_unix_ms: 1,
            updated_at_unix_ms: 2,
            last_lines: vec!["token=mytoken".to_string()],
        };
        let filtered = sec.filter_sensitive(event);
        assert_eq!(filtered.last_lines[0], "token=[REDACTED]");
    }

    #[test]
    fn generates_sha1_passkey() {
        let key = generate_passkey_sha1("host-a", 100, 200);
        assert_eq!(key.len(), 40);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

