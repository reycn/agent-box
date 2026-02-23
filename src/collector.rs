use std::process::Command;

use crate::model::{AgentKind, SessionEvent, SessionStatus};
use crate::unix_ms_now;

pub trait Collector {
    fn collect(&self) -> Vec<SessionEvent>;
}

#[derive(Debug, Default)]
pub struct MockCollector;

impl MockCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for MockCollector {
    fn collect(&self) -> Vec<SessionEvent> {
        let now = unix_ms_now();
        vec![
            SessionEvent {
                id: "local-claude-1".to_string(),
                agent: AgentKind::Claude,
                title: "refactor parser".to_string(),
                working_dir: "/workspace/app".to_string(),
                user: "local".to_string(),
                status: SessionStatus::Running,
                pending_action: Some("Approve write".to_string()),
                started_at_unix_ms: now.saturating_sub(40_000),
                updated_at_unix_ms: now,
                last_lines: vec![
                    "inspecting cli parser".to_string(),
                    "preparing patch".to_string(),
                ],
            },
            SessionEvent {
                id: "local-gemini-1".to_string(),
                agent: AgentKind::Gemini,
                title: "test stabilization".to_string(),
                working_dir: "/workspace/app".to_string(),
                user: "local".to_string(),
                status: SessionStatus::WaitingInput,
                pending_action: Some("Confirm run".to_string()),
                started_at_unix_ms: now.saturating_sub(80_000),
                updated_at_unix_ms: now,
                last_lines: vec!["awaiting confirmation".to_string()],
            },
        ]
    }
}

#[derive(Debug, Default)]
pub struct LocalProcessCollector;

impl LocalProcessCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for LocalProcessCollector {
    fn collect(&self) -> Vec<SessionEvent> {
        collect_local_process_sessions()
    }
}

fn collect_local_process_sessions() -> Vec<SessionEvent> {
    let output = match Command::new("ps").args(["-axo", "pid=,command="]).output() {
        Ok(v) if v.status.success() => v,
        _ => return Vec::new(),
    };

    let now = unix_ms_now();
    let user = std::env::var("USER").unwrap_or_else(|_| "local".to_string());
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/".to_string());
    let ps = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();

    for line in ps.lines() {
        let raw = line.trim();
        if raw.is_empty() {
            continue;
        }
        let mut fields = raw.split_whitespace();
        let pid_token = match fields.next() {
            Some(v) => v,
            None => continue,
        };
        let pid = match pid_token.parse::<u32>() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let command = raw
            .strip_prefix(pid_token)
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        if command.is_empty() {
            continue;
        }
        if command.contains("agent-box") {
            continue;
        }

        let Some(agent) = detect_agent_kind(&command) else {
            continue;
        };

        sessions.push(SessionEvent {
            id: format!("proc-{pid}"),
            agent,
            title: title_from_command(&command),
            working_dir: cwd.clone(),
            user: user.clone(),
            status: SessionStatus::Running,
            pending_action: None,
            started_at_unix_ms: now,
            updated_at_unix_ms: now,
            last_lines: vec![
                format!("pid={pid}"),
                format!("cmd: {}", truncate_text(&command, 64)),
            ],
        });
    }

    sessions
}

fn detect_agent_kind(command: &str) -> Option<AgentKind> {
    let lower = command.to_lowercase();
    if contains_exec_token(&lower, "claude") {
        return Some(AgentKind::Claude);
    }
    if contains_exec_token(&lower, "codex") || contains_exec_token(&lower, "openai") {
        return Some(AgentKind::Codex);
    }
    if contains_exec_token(&lower, "gemini") {
        return Some(AgentKind::Gemini);
    }
    None
}

fn contains_exec_token(command: &str, needle: &str) -> bool {
    command
        .split_whitespace()
        .any(|token| token == needle || token.ends_with(&format!("/{needle}")))
}

fn title_from_command(command: &str) -> String {
    let title = command.split_whitespace().take(5).collect::<Vec<_>>().join(" ");
    truncate_text(&title, 48)
}

fn truncate_text(input: &str, limit: usize) -> String {
    if input.chars().count() <= limit {
        return input.to_string();
    }
    let take = limit.saturating_sub(3);
    let mut out = String::new();
    for c in input.chars().take(take) {
        out.push(c);
    }
    out.push_str("...");
    out
}

#[cfg(test)]
mod tests {
    use super::{detect_agent_kind, title_from_command};
    use crate::model::AgentKind;

    #[test]
    fn detects_known_agent_processes() {
        assert_eq!(detect_agent_kind("claude"), Some(AgentKind::Claude));
        assert_eq!(
            detect_agent_kind("/usr/local/bin/codex --sandbox"),
            Some(AgentKind::Codex)
        );
        assert_eq!(detect_agent_kind("gemini --version"), Some(AgentKind::Gemini));
        assert_eq!(detect_agent_kind("bash -lc ls"), None);
    }

    #[test]
    fn title_is_truncated() {
        let title = title_from_command(
            "claude this is a very very very very very very long command string",
        );
        assert!(title.len() <= 48);
    }
}

