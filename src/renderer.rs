use crate::model::{AgentKind, SessionEvent, SessionStatus};

#[derive(Debug, Default)]
pub struct TerminalRenderer;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_ORANGE: &str = "\x1b[38;5;208m";
const ANSI_GRAY: &str = "\x1b[90m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_BLACK: &str = "\x1b[30m";
const ANSI_BG_ORANGE: &str = "\x1b[48;5;208m";
const ANSI_BG_BLUE: &str = "\x1b[44m";
const ANSI_BG_WHITE: &str = "\x1b[47m";
const ANSI_BG_GRAY: &str = "\x1b[100m";

impl TerminalRenderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render_many(&self, sessions: Vec<SessionEvent>) -> String {
        self.render_many_with_frame(sessions, 0)
    }

    pub fn render_many_with_frame(&self, sessions: Vec<SessionEvent>, frame: usize) -> String {
        sessions
            .iter()
            .map(|s| self.render_session_with_frame(s, frame))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn render_session(&self, s: &SessionEvent) -> String {
        self.render_session_with_frame(s, 0)
    }

    pub fn render_session_with_frame(&self, s: &SessionEvent, frame: usize) -> String {
        let title_bg = bg_for_agent(s.agent);
        let status_color = color_for_status(s.status);
        let icon = agent_icon(s.agent);
        let status_icon = status_icon(s.status, frame);

        let mut out = String::new();
        out.push_str(&format!(
            "{title_bg}{ANSI_BLACK}[{icon} {}]{ANSI_RESET}\n",
            truncate(&s.title, 32)
        ));
        out.push_str(&format!(
            "{ANSI_GRAY}  dir {} @ {}{ANSI_RESET}\n",
            truncate(&s.user, 20),
            truncate(&s.working_dir, 40)
        ));
        out.push_str(&format!(
            "  {ANSI_BOLD}{status_color}{}  {}{ANSI_RESET}\n",
            status_icon,
            format_status(s.status)
        ));

        if let Some(action) = &s.pending_action {
            out.push_str(&format!(
                "  {ANSI_CYAN}{ANSI_BOLD}⏳ {}{ANSI_RESET}\n",
                truncate(action, 48)
            ));
        }

        for line in s.last_lines.iter().take(2) {
            out.push_str(&format!(
                "{ANSI_DIM}{ANSI_GRAY}  > {}{ANSI_RESET}\n",
                truncate(line, 56)
            ));
        }
        out.trim_end().to_string()
    }
}

fn agent_icon(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => "◆",
        AgentKind::Codex => "◎",
        AgentKind::Gemini => "✦",
        AgentKind::Unknown => "?",
    }
}

fn format_status(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Running => "RUNNING",
        SessionStatus::WaitingInput => "WAITING_INPUT",
        SessionStatus::Success => "SUCCESS",
        SessionStatus::Failed => "FAILED",
        SessionStatus::Stopped => "STOPPED",
    }
}

fn status_icon(status: SessionStatus, frame: usize) -> &'static str {
    match status {
        SessionStatus::Running => {
            const FRAMES: [&str; 4] = ["◴", "◷", "◶", "◵"];
            FRAMES[frame % FRAMES.len()]
        }
        SessionStatus::WaitingInput => "?",
        SessionStatus::Success => "✓",
        SessionStatus::Failed => "✗",
        SessionStatus::Stopped => "■",
    }
}

fn bg_for_agent(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Claude => ANSI_BG_ORANGE,
        AgentKind::Codex => ANSI_BG_WHITE,
        AgentKind::Gemini => ANSI_BG_BLUE,
        AgentKind::Unknown => ANSI_BG_GRAY,
    }
}

fn color_for_status(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Running => ANSI_CYAN,
        SessionStatus::WaitingInput => ANSI_ORANGE,
        SessionStatus::Success => ANSI_GREEN,
        SessionStatus::Failed => ANSI_RED,
        SessionStatus::Stopped => ANSI_GRAY,
    }
}

fn truncate(input: &str, limit: usize) -> String {
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
    use crate::model::{AgentKind, SessionEvent, SessionStatus};

    use super::TerminalRenderer;

    #[test]
    fn renders_pending_action() {
        let renderer = TerminalRenderer::new();
        let event = SessionEvent {
            id: "1".to_string(),
            agent: AgentKind::Codex,
            title: "long title".to_string(),
            working_dir: "/tmp/repo".to_string(),
            user: "alice".to_string(),
            status: SessionStatus::WaitingInput,
            pending_action: Some("Click approve".to_string()),
            started_at_unix_ms: 1,
            updated_at_unix_ms: 2,
            last_lines: vec!["line 1".to_string()],
        };
        let output = renderer.render_session(&event);
        assert!(output.contains("Click approve"));
    }
}

