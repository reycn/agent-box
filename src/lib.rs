pub mod cli;
pub mod collector;
pub mod model;
pub mod renderer;
pub mod security;
pub mod sync;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::collector::{Collector, LocalProcessCollector};
use crate::model::{RuntimeStateStore, SessionEvent};
use crate::renderer::TerminalRenderer;

pub fn run_once_with_collector<C: Collector>(collector: &C, store: &mut RuntimeStateStore) {
    let events = collector.collect();
    for event in events {
        store.upsert(event);
    }
}

pub fn run_once(store: &mut RuntimeStateStore) {
    let collector = LocalProcessCollector::new();
    run_once_with_collector(&collector, store);
}

pub fn render_snapshot(store: &RuntimeStateStore) -> String {
    render_snapshot_with_frame(store, 0)
}

pub fn render_snapshot_with_frame(store: &RuntimeStateStore, frame: usize) -> String {
    let rendered = TerminalRenderer::new().render_many_with_frame(store.all(), frame);
    if rendered.trim().is_empty() {
        "No active Claude/Codex/Gemini local sessions detected.".to_string()
    } else {
        rendered
    }
}

pub fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_millis(0))
        .as_millis() as u64
}

pub fn sample_event(id: &str) -> SessionEvent {
    SessionEvent::new_running(
        id.to_string(),
        "sample".to_string(),
        "/tmp/demo".to_string(),
        "user".to_string(),
        vec!["working".to_string()],
        unix_ms_now(),
    )
}

