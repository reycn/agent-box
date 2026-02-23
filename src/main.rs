use std::process;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;

use agent_box::cli::{detect_public_ip, parse_peer, validate_bind, CliArgs};
use agent_box::model::RuntimeStateStore;
use agent_box::security::generate_passkey_sha1;
use agent_box::sync::SyncClient;
use agent_box::{render_snapshot_with_frame, run_once, unix_ms_now};

fn main() -> Result<()> {
    let session_unix_ms = unix_ms_now();
    let args = CliArgs::parse();
    let listen_ip = if args.public {
        match detect_public_ip() {
            Ok(ip) => ip,
            Err(err) => {
                eprintln!("warning: --public failed to resolve public IP ({err}); using 0.0.0.0");
                "0.0.0.0".to_string()
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

    if let Some(peer) = args.peer.as_deref() {
        let parsed = parse_peer(peer, session_unix_ms)?;
        if parsed.generated_auth_key {
            println!(
                "No passkey supplied for peer '{}'; generated SHA-1 passkey: {}",
                parsed.host, parsed.auth_key
            );
        }
        session_key = Some(parsed.auth_key.clone());
        let client = SyncClient::new(&parsed.auth_key);
        client.handshake(&parsed.auth_key)?;
    } else if !args.no_expose {
        // No passkey supplied at all in CLI input: generate one for join instructions.
        session_key = Some(generate_passkey_sha1(
            &format!("{local_host}:{listen_ip}"),
            session_unix_ms,
            random_seed,
        ));
    }

    let tick_secs = args.interval.max(1);
    let mut store = RuntimeStateStore::default();
    let mut frame: usize = 0;

    loop {
        run_once(&mut store);
        // Clear screen and move cursor to top-left for live dashboard behavior.
        print!("\x1b[2J\x1b[H");
        println!("Agent-box live monitor (Ctrl+C to stop)");
        if let Some(key) = &session_key {
            println!("Join by: agent-box {}:{}\n", listen_ip, key);
        } else {
            println!("--- refresh @ {} ---\n", unix_ms_now());
        }
        println!("{}", render_snapshot_with_frame(&store, frame));
        frame = frame.wrapping_add(1);
        thread::sleep(Duration::from_secs(tick_secs));
    }
}

