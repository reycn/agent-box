use std::collections::{HashMap, HashSet};
use std::process;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;

use agent_box::cli::{detect_public_ip, parse_peer, validate_bind, CliArgs};
use agent_box::model::RuntimeStateStore;
use agent_box::security::generate_passkey_sha1;
use agent_box::sync::{discover_join_key, SyncClient, SyncServer, TransportProtocol};
use agent_box::{render_snapshot_with_frame, run_once, unix_ms_now};

fn main() -> Result<()> {
    let session_unix_ms = unix_ms_now();
    let args = CliArgs::parse();
    let prefer_public_ip = args.public || args.peer.is_some();
    let listen_ip = if prefer_public_ip {
        match detect_public_ip() {
            Ok(ip) => ip,
            Err(err) => {
                eprintln!(
                    "warning: public IP resolution failed ({err}); using configured bind IP {}",
                    args.ip
                );
                args.ip.clone()
            }
        }
    } else {
        args.ip.clone()
    };
    validate_bind(&listen_ip, args.port)?;
    let random_seed = session_unix_ms ^ (process::id() as u64);
    let local_host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "localhost".to_string());
    let mut session_key: Option<String> = None;
    let mut peer_host: Option<String> = None;

    if let Some(peer) = args.peer.as_deref() {
        let parsed = parse_peer(peer, session_unix_ms)?;
        let mut effective_key = if let Some(explicit) = args.key.as_deref() {
            explicit.to_string()
        } else {
            parsed.auth_key.clone()
        };
        if parsed.generated_auth_key && args.key.is_none() {
            match discover_join_key(&parsed.host, args.port, Duration::from_millis(500)) {
                Ok(discovered) => {
                    effective_key = discovered;
                    println!("Discovered peer passkey from '{}'.", parsed.host);
                }
                Err(err) => {
                    println!(
                        "No passkey supplied for peer '{}'; discovery failed ({err}), using generated fallback key.",
                        parsed.host
                    );
                }
            }
        }
        peer_host = Some(parsed.host.clone());
        session_key = Some(effective_key.clone());
        let client = SyncClient::new(&effective_key);
        client.handshake(&effective_key)?;
    } else if let Some(explicit_key) = args.key.as_deref() {
        // Explicit key also defines local session sharing key without a join target.
        session_key = Some(explicit_key.to_string());
    } else if !args.no_expose {
        // No passkey supplied at all in CLI input: generate one for join instructions.
        session_key = Some(generate_passkey_sha1(
            &format!("{local_host}:{listen_ip}"),
            session_unix_ms,
            random_seed,
        ));
    }

    let tick_secs = args.interval.max(1);
    let mut local_store = RuntimeStateStore::default();
    let mut combined_store = RuntimeStateStore::default();
    let mut frame: usize = 0;
    let mut remote_cache: HashMap<String, (agent_box::model::SessionEvent, u64)> = HashMap::new();
    let mut known_peers: HashSet<String> = HashSet::new();
    let protocol = transport_from_args(args.protocol);
    let bind_ip = if prefer_public_ip {
        "0.0.0.0".to_string()
    } else {
        listen_ip.clone()
    };

    let sync_server = if !args.no_expose {
        if let Some(key) = &session_key {
            match SyncServer::bind(&bind_ip, args.port, key) {
                Ok(server) => Some(server),
                Err(err) => {
                    eprintln!(
                        "warning: could not start sync server on {}:{} ({err})",
                        bind_ip, args.port
                    );
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    loop {
        let now_ms = unix_ms_now();
        local_store.clear();
        run_once(&mut local_store);
        let local_events = local_store.all();
        let local_events_snapshot = local_events.clone();

        if let (Some(server), Some(key)) = (&sync_server, session_key.as_deref()) {
            if let Ok(incoming) = server.serve_once(
                local_events.clone(),
                &listen_ip,
                now_ms,
                protocol,
            ) {
                for update in incoming {
                    known_peers.insert(update.peer.clone());
                    for mut event in update.payload {
                        event.id = format!("remote:{}:{}", update.peer, event.id);
                        event.user = format!("{}@{}", event.user, update.peer);
                        event.updated_at_unix_ms = now_ms;
                        remote_cache.insert(event.id.clone(), (event, now_ms));
                    }
                }
            }
            // Keep an explicit handshake check in loop for deterministic auth behavior.
            let _ = SyncClient::new(key).handshake(key);
        }

        let mut pull_targets = known_peers.clone();
        if let Some(host) = peer_host.as_ref() {
            pull_targets.insert(host.clone());
        }

        if let Some(key) = session_key.as_deref() {
            for target in pull_targets {
                if target == listen_ip {
                    continue;
                }
                let client = SyncClient::new(key);
                if let Ok(remote) = client.pull_once(
                    &target,
                    args.port,
                    key,
                    &listen_ip,
                    local_events_snapshot.clone(),
                    Duration::from_millis(350),
                ) {
                    let source_peer = if remote.peer.trim().is_empty() {
                        target.clone()
                    } else {
                        remote.peer.clone()
                    };
                    known_peers.insert(source_peer.clone());
                    for mut event in remote.payload {
                        // Namespace remote identity so local and remote sessions coexist.
                        event.id = format!("remote:{}:{}", source_peer, event.id);
                        event.user = format!("{}@{}", event.user, source_peer);
                        event.updated_at_unix_ms = now_ms;
                        remote_cache.insert(event.id.clone(), (event, now_ms));
                    }
                }
            }
        }

        // Keep remote cache stable to avoid flicker, but prune stale entries.
        let remote_ttl_ms = (tick_secs * 8 * 1000) as u64;
        remote_cache.retain(|_, (_, seen_at)| now_ms.saturating_sub(*seen_at) <= remote_ttl_ms);

        combined_store.clear();
        for event in local_events {
            let _ = combined_store.upsert(event);
        }
        for (event, _) in remote_cache.values() {
            let _ = combined_store.upsert(event.clone());
        }

        // Clear screen and move cursor to top-left for live dashboard behavior.
        print!("\x1b[2J\x1b[H");
        println!("Agent-box live monitor (Ctrl+C to stop)");
        if let Some(key) = &session_key {
            println!("Join by: agent-box {}:{}\n", listen_ip, key);
        } else {
            println!("--- refresh @ {} ---\n", now_ms);
        }
        println!("{}", render_snapshot_with_frame(&combined_store, frame));
        frame = frame.wrapping_add(1);
        thread::sleep(Duration::from_secs(tick_secs));
    }
}

fn transport_from_args(protocol: agent_box::cli::Protocol) -> TransportProtocol {
    match protocol {
        agent_box::cli::Protocol::Http => TransportProtocol::Http,
        agent_box::cli::Protocol::Https => TransportProtocol::Https,
        agent_box::cli::Protocol::Quic => TransportProtocol::Quic,
    }
}

