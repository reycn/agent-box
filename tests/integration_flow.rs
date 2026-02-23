use agent_box::collector::MockCollector;
use agent_box::model::{RuntimeStateStore, SessionStatus};
use agent_box::run_once_with_collector;
use agent_box::{render_snapshot, sample_event};

#[test]
fn local_collect_store_render_flow() {
    let mut store = RuntimeStateStore::default();
    let collector = MockCollector::new();
    run_once_with_collector(&collector, &mut store);
    let output = render_snapshot(&store);
    assert!(output.contains("RUNNING") || output.contains("WAITING_INPUT"));
}

#[test]
fn store_blocks_invalid_terminal_regression() {
    let mut store = RuntimeStateStore::default();
    let mut first = sample_event("s-1");
    first.status = SessionStatus::Success;
    assert!(store.upsert(first.clone()));

    let mut invalid = first;
    invalid.status = SessionStatus::Running;
    invalid.updated_at_unix_ms += 1;
    assert!(!store.upsert(invalid));
}

