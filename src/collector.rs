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
                format!("cmd: {}", summarize_command(&command, 64)),
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
    summarize_command(command, 48)
}

fn summarize_command(command: &str, limit: usize) -> String {
    let mut tokens = command.split_whitespace();
    let exe = tokens.next().unwrap_or_default();
    let exe_name = exe.rsplit('/').next().unwrap_or(exe);
    let tail_args = tokens.collect::<Vec<_>>();

    let summary = if tail_args.is_empty() {
        exe_name.to_string()
    } else {
        let kept_tail = if tail_args.len() > 3 {
            tail_args[tail_args.len() - 3..].join(" ")
        } else {
            tail_args.join(" ")
        };
        format!("{exe_name} {kept_tail}")
    };

    truncate_keep_right(&summary, limit)
}

fn truncate_keep_right(input: &str, limit: usize) -> String {
    if input.chars().count() <= limit {
        return input.to_string();
    }
    let take = limit.saturating_sub(3);
    let tail: String = input
        .chars()
        .rev()
        .take(take)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{tail}")
}

#[cfg(test)]
mod tests {
    use super::{detect_agent_kind, summarize_command, title_from_command};
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
            "/opt/tools/claude this is a very very very very very very long command string with extra tail",
        );
        assert!(title.len() <= 48);
    }

    #[test]
    fn summarize_command_prefers_executable_and_tail() {
        let s = summarize_command(
            "/home/rongxin/.nvm/versions/node/v20.8.1/bin/node /a/b/c/d/e/f/g.js --foo --bar",
            64,
        );
        assert!(s.contains("node"));
        assert!(s.contains("--foo") || s.contains("--bar"));
    }
}

