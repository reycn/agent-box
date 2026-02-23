use std::io::{Read, Write};
use std::net::IpAddr;
use std::net::TcpStream;
use std::process;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use clap::{Parser, ValueEnum};

use crate::security::generate_passkey_sha1;

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
pub enum Protocol {
    Http,
    Https,
    Quic,
}

#[derive(Debug, Parser)]
#[command(name = "agent-box")]
#[command(about = "Terminal monitor for local and remote agent sessions")]
pub struct CliArgs {
    #[arg(help = "Optional address as HOST:AUTH_KEY")]
    pub peer: Option<String>,

    #[arg(long, help = "Local mode only, no network listener")]
    pub no_expose: bool,

    #[arg(short = 'i', long, default_value = "127.0.0.1")]
    pub ip: String,

    #[arg(long, help = "Use detected public IP as bind/join IP")]
    pub public: bool,

    #[arg(short = 'p', long, default_value_t = 8346)]
    pub port: u16,

    #[arg(short = 't', long, default_value_t = 3)]
    pub interval: u64,

    #[arg(short = 'r', long = "protocol", value_enum, default_value_t = Protocol::Http)]
    pub protocol: Protocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPeer {
    pub host: String,
    pub auth_key: String,
    pub generated_auth_key: bool,
}

pub fn parse_args_from<I, T>(args: I) -> CliArgs
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    CliArgs::parse_from(args)
}

pub fn parse_peer(peer: &str, session_unix_ms: u64) -> Result<ParsedPeer> {
    if let Some((host, auth_key)) = peer.split_once(':') {
        if host.trim().is_empty() || auth_key.trim().is_empty() {
            return Err(anyhow!("peer host and auth key must be non-empty"));
        }
        return Ok(ParsedPeer {
            host: host.to_string(),
            auth_key: auth_key.to_string(),
            generated_auth_key: false,
        });
    }

    let host = peer.trim();
    if host.is_empty() {
        return Err(anyhow!("peer host must be non-empty"));
    }

    let local_host = detect_hostname();
    let random_seed = runtime_random_seed();
    let generated = generate_passkey_sha1(
        &format!("{local_host}:{host}"),
        session_unix_ms,
        random_seed,
    );

    Ok(ParsedPeer {
        host: host.to_string(),
        auth_key: generated,
        generated_auth_key: true,
    })
}

fn detect_hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "localhost".to_string())
}

fn runtime_random_seed() -> u64 {
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    ns ^ (process::id() as u64)
}

pub fn validate_bind(ip: &str, port: u16) -> Result<()> {
    let _ = IpAddr::from_str(ip).map_err(|_| anyhow!("invalid IP address: {ip}"))?;
    if port == 0 {
        return Err(anyhow!("port 0 is not allowed"));
    }
    Ok(())
}

pub fn detect_public_ip() -> Result<String> {
    let mut stream = TcpStream::connect("api.ipify.org:80")
        .map_err(|e| anyhow!("failed to contact public IP service: {e}"))?;
    let request = "GET / HTTP/1.1\r\nHost: api.ipify.org\r\nConnection: close\r\n\r\n";
    stream
        .write_all(request.as_bytes())
        .map_err(|e| anyhow!("failed to request public IP: {e}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| anyhow!("failed to read public IP response: {e}"))?;
    let body = response
        .split("\r\n\r\n")
        .last()
        .ok_or_else(|| anyhow!("invalid public IP response"))?
        .trim();
    let _ = IpAddr::from_str(body).map_err(|_| anyhow!("invalid public IP: {body}"))?;
    Ok(body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let args = parse_args_from(["agent-box"]);
        assert_eq!(args.ip, "127.0.0.1");
        assert!(!args.public);
        assert_eq!(args.port, 8346);
        assert_eq!(args.interval, 3);
        assert_eq!(args.protocol, Protocol::Http);
    }

    #[test]
    fn parses_public_flag() {
        let args = parse_args_from(["agent-box", "--public"]);
        assert!(args.public);
    }

    #[test]
    fn peer_requires_separator() {
        let parsed = parse_peer("127.0.0.1:key", 100).expect("valid peer");
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.auth_key, "key");
        assert!(!parsed.generated_auth_key);
    }

    #[test]
    fn peer_without_key_generates_one() {
        let parsed = parse_peer("127.0.0.1", 100).expect("valid peer");
        assert_eq!(parsed.host, "127.0.0.1");
        assert_eq!(parsed.auth_key.len(), 40);
        assert!(parsed.generated_auth_key);
    }
}

