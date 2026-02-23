use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentKind {
    Claude,
    Codex,
    Gemini,
    Unknown,
}

impl AgentKind {
    pub fn as_label(&self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
            AgentKind::Gemini => "gemini",
            AgentKind::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionStatus {
    Running,
    WaitingInput,
    Success,
    Failed,
    Stopped,
}

impl SessionStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SessionStatus::Success | SessionStatus::Failed | SessionStatus::Stopped
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionEvent {
    pub id: String,
    pub agent: AgentKind,
    pub title: String,
    pub working_dir: String,
    pub user: String,
    pub status: SessionStatus,
    pub pending_action: Option<String>,
    pub started_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub last_lines: Vec<String>,
}

impl SessionEvent {
    pub fn new_running(
        id: String,
        title: String,
        working_dir: String,
        user: String,
        last_lines: Vec<String>,
        now: u64,
    ) -> Self {
        Self {
            id,
            agent: AgentKind::Unknown,
            title,
            working_dir,
            user,
            status: SessionStatus::Running,
            pending_action: None,
            started_at_unix_ms: now,
            updated_at_unix_ms: now,
            last_lines,
        }
    }

    pub fn can_transition_to(&self, next: SessionStatus) -> bool {
        use SessionStatus::*;
        match (self.status, next) {
            (Success, _) | (Failed, _) | (Stopped, _) => false,
            (Running, Running | WaitingInput | Success | Failed | Stopped) => true,
            (WaitingInput, WaitingInput | Running | Stopped) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Default)]
pub struct RuntimeStateStore {
    sessions: HashMap<String, SessionEvent>,
}

impl RuntimeStateStore {
    pub fn clear(&mut self) {
        self.sessions.clear();
    }

    pub fn upsert(&mut self, incoming: SessionEvent) -> bool {
        if let Some(existing) = self.sessions.get_mut(&incoming.id) {
            if incoming.updated_at_unix_ms < existing.updated_at_unix_ms {
                return false;
            }
            if !existing.can_transition_to(incoming.status) && incoming.status != existing.status {
                return false;
            }
            *existing = incoming;
            return true;
        }
        self.sessions.insert(incoming.id.clone(), incoming);
        true
    }

    pub fn all(&self) -> Vec<SessionEvent> {
        let mut items: Vec<_> = self.sessions.values().cloned().collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items
    }

    pub fn get(&self, id: &str) -> Option<&SessionEvent> {
        self.sessions.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, status: SessionStatus, ts: u64) -> SessionEvent {
        SessionEvent {
            id: id.to_string(),
            agent: AgentKind::Claude,
            title: "demo".to_string(),
            working_dir: "/tmp".to_string(),
            user: "alice".to_string(),
            status,
            pending_action: None,
            started_at_unix_ms: 1,
            updated_at_unix_ms: ts,
            last_lines: vec!["hello".to_string()],
        }
    }

    #[test]
    fn rejects_older_updates() {
        let mut store = RuntimeStateStore::default();
        assert!(store.upsert(event("a", SessionStatus::Running, 20)));
        assert!(!store.upsert(event("a", SessionStatus::Running, 19)));
    }

    #[test]
    fn rejects_invalid_transition_from_terminal_state() {
        let mut store = RuntimeStateStore::default();
        assert!(store.upsert(event("a", SessionStatus::Success, 20)));
        assert!(!store.upsert(event("a", SessionStatus::Running, 21)));
    }
}

