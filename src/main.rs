#![deny(warnings)]
#![warn(clippy::pedantic, clippy::nursery)]

use clap::Parser;
use colored::Colorize;
use serde_json::{Map, Value};
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

    let (kind, normalized) = classify(&args.query)?;
    let url = match kind {
        Kind::Domain => format!("https://rdap.org/domain/{normalized}"),
        Kind::IpAddr => format!("https://rdap.org/ip/{normalized}"),
        Kind::Asn => format!("https://rdap.org/autnum/{normalized}"),
    };

    let client = reqwest::Client::builder()
        .user_agent("rdapx/0.1 (+https://github.com/YOUR_GITHUB_USERNAME/rdapx)")
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

fn classify(q: &str) -> Result<(Kind, String), Box<dyn Error>> {
    let s = q.trim();
    if s.starts_with('A') || s.starts_with('a') {
        // Accept AS12345 or as12345
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
    // Work with an owned map to avoid temp borrows
    let obj: Map<String, Value> = json.as_object().cloned().unwrap_or_default();
    let kind = obj
        .get("objectClassName")
        .and_then(Value::as_str)
        .unwrap_or("-");

    let str_of = |k: &str| obj.get(k).and_then(Value::as_str).unwrap_or("-");
    let first_str = |arr_key: &str, sub: &str| {
        obj.get(arr_key)
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|o| o.get(sub))
            .and_then(Value::as_str)
            .unwrap_or("-")
            .to_string()
    };

    // common top line
    println!("{} {}", "Type:".blue().bold(), kind);
    println!("{} {}", "Handle:".blue().bold(), str_of("handle"));
    println!("{} {}", "Name:".blue().bold(), str_of("name"));

    match kind {
        "domain" => {
            if let Some(registrar) = find_entity_role(&obj, "registrar") {
                println!("{} {}", "Registrar:".blue().bold(), registrar);
            }
            if let Some(statuses) = obj.get("status").and_then(Value::as_array) {
                let s: Vec<&str> = statuses.iter().filter_map(Value::as_str).collect();
                if !s.is_empty() {
                    println!("{} {}", "Status:".blue().bold(), s.join(", "));
                }
            }
            if let Some(p43) = obj.get("port43").and_then(Value::as_str) {
                println!("{} {}", "WHOIS:".blue().bold(), p43);
            }
        }
        "ip network" => {
            println!("{} {}", "Country:".blue().bold(), str_of("country"));
            println!("{} {}", "Start:".blue().bold(), str_of("startAddress"));
            println!("{} {}", "End:".blue().bold(), str_of("endAddress"));
            let cidr = obj.get("cidr0_cidrs").map_or_else(
                || "-".to_string(),
                |_| {
                    format!(
                        "{}/{}",
                        first_str("cidr0_cidrs", "v4prefix"),
                        first_str("cidr0_cidrs", "length")
                    )
                },
            );
            println!("{} {}", "CIDR:".blue().bold(), cidr);

            if let Some(statuses) = obj.get("status").and_then(Value::as_array) {
                let s: Vec<&str> = statuses.iter().filter_map(Value::as_str).collect();
                if !s.is_empty() {
                    println!("{} {}", "Status:".blue().bold(), s.join(", "));
                }
            }
            if let Some(abuse) = find_entity_role(&obj, "abuse") {
                println!("{} {}", "Abuse:".blue().bold(), abuse);
            }
            if let Some(noc) = find_entity_role(&obj, "noc") {
                println!("{} {}", "NOC:".blue().bold(), noc);
            }
        }
        "autnum" => {
            let start = obj
                .get("startAutnum")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let end = obj
                .get("endAutnum")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            if start != 0 {
                println!("{} {start}–{end}", "Range:".blue().bold());
            }
            if let Some(statuses) = obj.get("status").and_then(Value::as_array) {
                let s: Vec<&str> = statuses.iter().filter_map(Value::as_str).collect();
                if !s.is_empty() {
                    println!("{} {}", "Status:".blue().bold(), s.join(", "));
                }
            }
            if let Some(registrant) = find_entity_role(&obj, "registrant") {
                println!("{} {}", "Registrant:".blue().bold(), registrant);
            }
            if let Some(abuse) = find_entity_role(&obj, "abuse") {
                println!("{} {}", "Abuse:".blue().bold(), abuse);
            }
        }
        _ => {}
    }

    if let Some(roles_line) = roles_summary(&obj) {
        println!("{} {}", "Roles:".blue().bold(), roles_line);
    }
}

fn roles_summary(obj: &Map<String, Value>) -> Option<String> {
    let entities = obj.get("entities")?.as_array()?;
    let mut roles: Vec<String> = Vec::new();
    for e in entities {
        if let Some(rs) = e.get("roles").and_then(Value::as_array) {
            for r in rs {
                if let Some(s) = r.as_str() {
                    roles.push(s.to_string());
                }
            }
        }
    }
    if roles.is_empty() {
        None
    } else {
        roles.sort();
        roles.dedup();
        Some(roles.join(", "))
    }
}

fn find_entity_role(obj: &Map<String, Value>, role: &str) -> Option<String> {
    let entities = obj.get("entities")?.as_array()?;
    for e in entities {
        let has_role = e
            .get("roles")
            .and_then(Value::as_array)
            .is_some_and(|rs| rs.iter().any(|r| r.as_str() == Some(role)));
        if has_role {
            // prefer vcard "fn" if present, else handle
            if let Some(vcard) = e.get("vcardArray").and_then(Value::as_array) {
                if vcard.len() == 2 {
                    if let Some(fields) = vcard.get(1).and_then(Value::as_array) {
                        for f in fields {
                            if f.get(0).and_then(Value::as_str) == Some("fn") {
                                if let Some(name) = f.get(3).and_then(Value::as_str) {
                                    return Some(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
            if let Some(handle) = e.get("handle").and_then(Value::as_str) {
                return Some(handle.to_string());
            }
        }
    }
    None
}
