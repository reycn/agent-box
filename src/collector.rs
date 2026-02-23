use std::process::Command;
use std::path::Path;

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
            title: title_from_command(&command, agent, &cwd),
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

fn title_from_command(command: &str, agent: AgentKind, cwd: &str) -> String {
    let title = match agent {
        AgentKind::Claude => claude_title_from_command(command, cwd)
            .unwrap_or_else(|| summarize_command(command, 48)),
        _ => summarize_command(command, 48),
    };
    truncate_keep_right(&title, 48)
}

fn claude_title_from_command(command: &str, cwd: &str) -> Option<String> {
    let tokens = command
        .split_whitespace()
        .map(normalize_token)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();

    if let Some(path) = find_session_path_hint(&tokens) {
        if let Some(title) = read_title_from_session_file(&path) {
            return Some(format!("claude {title}"));
        }
        if let Some(hint) = summarize_session_path(&path) {
            return Some(format!("claude {hint}"));
        }
    }

    let args_title = tokens
        .iter()
        .skip(1)
        .filter(|t| !t.starts_with('-') && !looks_like_path(t))
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if !args_title.is_empty() {
        return Some(format!("claude {args_title}"));
    }

    let project = Path::new(cwd)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("session");
    Some(format!("claude {project}"))
}

fn normalize_token(token: &str) -> String {
    token
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches(',')
        .to_string()
}

fn find_session_path_hint(tokens: &[String]) -> Option<String> {
    tokens
        .iter()
        .find(|token| {
            let lower = token.to_lowercase();
            looks_like_path(token)
                && (lower.contains("session")
                    || lower.contains("transcript")
                    || lower.contains(".cursor")
                    || lower.contains(".claude")
                    || lower.contains(".happy"))
        })
        .cloned()
}

fn looks_like_path(token: &str) -> bool {
    token.contains('/') || token.contains('\\')
}

fn read_title_from_session_file(path: &str) -> Option<String> {
    let p = Path::new(path);
    if !p.exists() || !p.is_file() {
        return None;
    }
    let content = std::fs::read_to_string(p).ok()?;
    extract_json_title(&content)
}

fn extract_json_title(content: &str) -> Option<String> {
    // Lightweight extraction for JSON/JSONL-like payloads that carry a "title" field.
    let key_pos = content.find("\"title\"")?;
    let after_key = &content[key_pos + "\"title\"".len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();
    if !after_colon.starts_with('"') {
        return None;
    }
    let mut escaped = false;
    let mut out = String::new();
    for ch in after_colon[1..].chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            break;
        }
        out.push(ch);
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn summarize_session_path(path: &str) -> Option<String> {
    let p = Path::new(path);
    let file = p
        .file_stem()
        .and_then(|v| v.to_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    if file.contains("session") || file.contains("transcript") {
        return Some(file.to_string());
    }
    let parent = p
        .parent()
        .and_then(|v| v.file_name())
        .and_then(|v| v.to_str())
        .unwrap_or("session");
    Some(format!("{parent}:{file}"))
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
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        claude_title_from_command, detect_agent_kind, extract_json_title, summarize_command,
        title_from_command,
    };
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
            AgentKind::Claude,
            "/tmp/project",
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

    #[test]
    fn extracts_json_title_value() {
        let text = r#"{"id":"1","title":"Refactor auth middleware","x":1}"#;
        let title = extract_json_title(text).expect("title should exist");
        assert_eq!(title, "Refactor auth middleware");
    }

    #[test]
    fn claude_title_reads_session_file_when_present() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("agent-box-title-{unique}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("session.json");
        fs::write(&path, r#"{"title":"Bug bash - login flow"}"#).expect("write file");

        let cmd = format!("claude --session {}", path.display());
        let title = claude_title_from_command(&cmd, "/tmp/project").expect("title");
        assert!(title.contains("Bug bash - login flow"));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}

