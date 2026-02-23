use agent_box::cli::validate_bind;
use agent_box::sync::{RetryPolicy, SyncClient};

#[test]
fn invalid_ip_is_rejected() {
    assert!(validate_bind("not_an_ip", 8346).is_err());
}

#[test]
fn invalid_auth_key_fails_handshake() {
    let client = SyncClient::new("expected-key");
    assert!(client.handshake("wrong-key").is_err());
}

#[test]
fn reconnect_backoff_caps_max_delay() {
    let policy = RetryPolicy::default();
    let last = policy.delay_for_attempt(policy.max_attempts);
    assert!(last.as_millis() <= policy.max_delay_ms as u128);
}

