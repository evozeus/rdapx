#![deny(warnings)]
#![warn(clippy::pedantic, clippy::nursery)]

use clap::Parser;
use colored::Colorize;
use serde_json::Value;
use std::error::Error;

/// rdapx: RDAP-first lookup for domains, IPs, and ASNs.
/// Prints JSON by default, with a simple table mode for quick looks.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Query: domain (example.com), IP (1.1.1.1), or ASN (AS13335 or 13335)
    query: String,

    /// Pretty-print JSON
    #[arg(long)]
    pretty: bool,

    /// Table (short summary) instead of JSON
    #[arg(long)]
    table: bool,

    /// Timeout in seconds
    #[arg(long, default_value_t = 8)]
    timeout: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Very simple routing via rdap.org proxy for v0.1
    let (kind, normalized) = classify(&args.query)?;
    let url = match kind {
        Kind::Domain => format!("https://rdap.org/domain/{normalized}"),
        Kind::IpAddr => format!("https://rdap.org/ip/{normalized}"),
        Kind::Asn => format!("https://rdap.org/autnum/{normalized}"),
    };

    let client = reqwest::Client::builder()
        .user_agent("rdapx/0.1 (+https://github.com/yourname/rdapx)")
        .timeout(std::time::Duration::from_secs(args.timeout))
        .build()?;

    let resp = client.get(&url).send().await?;

    if !resp.status().is_success() {
        eprintln!("{} {}", "RDAP error:".red().bold(), resp.status());
        std::process::exit(1);
    }

    let json: Value = resp.json().await?;

    if args.table {
        print_table(&json);
    } else if args.pretty {
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("{}", serde_json::to_string(&json)?);
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Domain,
    IpAddr,
    Asn,
}
fn classify(q: &str) -> Result<(Kind, String), Box<dyn std::error::Error>> {
    let s = q.trim();
    if s.starts_with('A') || s.starts_with('a') {
        // still accept “AS…”
        // ↓ was: trim_start_matches(|c| c=='A'||c=='a'||c=='S'||c=='s')
        let num = s.trim_start_matches(&['A', 'a', 'S', 's'][..]);
        Ok((Kind::Asn, num.to_string()))
    } else if s.parse::<std::net::IpAddr>().is_ok() {
        Ok((Kind::IpAddr, s.to_string()))
    } else if s.contains('.') {
        Ok((Kind::Domain, s.to_string()))
    } else if let Ok(_n) = s.parse::<u32>() {
        Ok((Kind::Asn, s.to_string()))
    } else {
        Err("cannot classify query (expect domain, IP, or ASN)".into())
    }
}
fn print_table(json: &Value) {
    // Take an *owned* Map so there are no dangling references
    let obj = json.as_object().cloned().unwrap_or_default();

    let handle = obj.get("handle").and_then(|v| v.as_str()).unwrap_or("-");
    let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("-");
    let type_ = obj
        .get("objectClassName")
        .and_then(|v| v.as_str())
        .unwrap_or("-");

    println!("{} {}", "Type:".blue().bold(), type_);
    println!("{} {}", "Handle:".blue().bold(), handle);
    println!("{} {}", "Name:".blue().bold(), name);

    if let Some(entities) = obj.get("entities").and_then(|v| v.as_array()) {
        let mut roles: Vec<String> = Vec::new();
        for e in entities {
            if let Some(rs) = e.get("roles").and_then(|v| v.as_array()) {
                for r in rs {
                    if let Some(s) = r.as_str() {
                        roles.push(s.to_string());
                    }
                }
            }
        }
        if !roles.is_empty() {
            roles.sort();
            roles.dedup();
            println!("{} {}", "Roles:".blue().bold(), roles.join(", "));
        }
    }
}
