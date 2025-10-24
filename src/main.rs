#![deny(warnings)]
#![warn(clippy::pedantic, clippy::nursery)]

use clap::Parser;
use colored::Colorize;
use directories::BaseDirs;
use futures::stream::{self, StreamExt};
use serde_json::{Map, Value};
use std::error::Error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// rdapx: RDAP-first lookup for domains, IPs, and ASNs.
/// Now with bulk mode and on-disk caching.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    /// Query: domain (example.com), IP (1.1.1.1), or ASN (AS13335 or 13335)
    /// If --file is provided, this is optional.
    query: Option<String>,

    /// Pretty-print JSON (ignored in --table)
    #[arg(long)]
    pretty: bool,

    /// Table (short summary) instead of JSON
    #[arg(long)]
    table: bool,

    /// Timeout in seconds (default 8)
    #[arg(long, default_value_t = 8)]
    timeout: u64,

    /// Bulk mode: read one query per line from a file (blank lines and lines starting with # are ignored)
    #[arg(long, value_name = "PATH")]
    file: Option<PathBuf>,

    /// How many lookups to run in parallel in --file mode
    #[arg(long, default_value_t = 8)]
    concurrency: usize,

    /// Cache TTL in seconds (default 86400 = 24h)
    #[arg(long, default_value_t = 86_400)]
    cache_ttl: u64,

    /// Disable cache (always fetch fresh)
    #[arg(long)]
    no_cache: bool,
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Domain,
    IpAddr,
    Asn,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    // Validate input presence
    if args.query.is_none() && args.file.is_none() {
        eprintln!(
            "{} Provide a QUERY or use {}.",
            "Error:".red().bold(),
            "--file <path>".bold()
        );
        std::process::exit(2);
    }

    let client = reqwest::Client::builder()
        .user_agent("rdapx/0.1 (+https://github.com/YOUR_GITHUB_USERNAME/rdapx)")
        .timeout(Duration::from_secs(args.timeout))
        .build()?;

    if let Some(list) = &args.file {
        // BULK mode
        let items = read_lines(list)?;
        if items.is_empty() {
            eprintln!(
                "{} no queries found in {}",
                "Note:".yellow().bold(),
                list.display()
            );
            return Ok(());
        }

        let ttl = Duration::from_secs(args.cache_ttl);
        let concurrency = args.concurrency.max(1);

        stream::iter(items.into_iter())
            .map(|q| {
                let client = &client;
                let args = &args;
                async move {
                    if let Err(e) = process_one(client, args, &q, ttl).await {
                        eprintln!("{} {q}: {e}", "Failed".red().bold());
                    }
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        return Ok(());
    }

    // SINGLE lookup
    let q = args.query.as_deref().unwrap();
    let ttl = Duration::from_secs(args.cache_ttl);
    process_one(&client, &args, q, ttl).await
}

async fn process_one(
    client: &reqwest::Client,
    args: &Args,
    q: &str,
    ttl: Duration,
) -> Result<(), Box<dyn Error>> {
    let (kind, normalized) = classify(q)?;
    let url = match kind {
        Kind::Domain => format!("https://rdap.org/domain/{normalized}"),
        Kind::IpAddr => format!("https://rdap.org/ip/{normalized}"),
        Kind::Asn => format!("https://rdap.org/autnum/{normalized}"),
    };

    let json = fetch_json(client, &url, ttl, args.no_cache).await?;

    if args.table {
        print_table(&json);
    } else if args.pretty {
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        println!("{}", serde_json::to_string(&json)?);
    }

    Ok(())
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

/* ----------------------------- caching layer ----------------------------- */

fn cache_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|b| b.cache_dir().join("rdapx"))
}

fn cache_key(url: &str) -> String {
    use blake3::Hasher;
    let mut h = Hasher::new();
    h.update(url.as_bytes());
    h.finalize().to_hex().to_string()
}

fn cache_path_for(url: &str) -> Option<PathBuf> {
    cache_dir().map(|d| d.join(cache_key(url) + ".json"))
}

fn is_fresh(path: &Path, ttl: Duration) -> bool {
    if let Ok(meta) = fs::metadata(path) {
        if let Ok(modt) = meta.modified() {
            if let Ok(age) = SystemTime::now().duration_since(modt) {
                return age <= ttl;
            }
        }
    }
    false
}

fn read_cache(path: &Path) -> Option<Value> {
    let mut s = String::new();
    let mut f = fs::File::open(path).ok()?;
    if f.read_to_string(&mut s).is_ok() {
        serde_json::from_str(&s).ok()
    } else {
        None
    }
}

fn write_cache(path: &Path, json: &Value) -> Result<(), Box<dyn Error>> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, serde_json::to_string(json)?)?;
    Ok(())
}

#[allow(clippy::missing_errors_doc)]
async fn fetch_json(
    client: &reqwest::Client,
    url: &str,
    ttl: Duration,
    no_cache: bool,
) -> Result<Value, Box<dyn Error>> {
    if !no_cache {
        if let Some(p) = cache_path_for(url) {
            if is_fresh(&p, ttl) {
                if let Some(v) = read_cache(&p) {
                    return Ok(v);
                }
            }
        }
    }

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        return Err(format!("RDAP error: {}", resp.status()).into());
    }
    let json: Value = resp.json().await?;

    if !no_cache {
        if let Some(p) = cache_path_for(url) {
            let _ = write_cache(&p, &json);
        }
    }

    Ok(json)
}

/* ------------------------------ table output ----------------------------- */

fn print_table(json: &Value) {
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

/* ------------------------------ helpers ---------------------------------- */

fn read_lines(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let raw = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        out.push(t.to_string());
    }
    Ok(out)
}
