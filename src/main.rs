#![deny(warnings)]
#![warn(clippy::pedantic, clippy::nursery)]
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)] // keep CI happy while iterating
#![allow(clippy::module_name_repetitions)]

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{
    generate,
    shells::{Bash, Fish, Zsh},
};
use colored::Colorize;
use directories::BaseDirs;
use futures::stream::{self, StreamExt};
use serde_json::Value;
use std::error::Error;
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokio::time::sleep;
#[derive(ValueEnum, Clone, Copy, Debug)]
enum Format {
    Json,
    Pretty,
    Table,
}

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Global output format (replaces --pretty / --table)
    #[arg(long, value_enum, default_value_t = Format::Json)]
    format: Format,

    /// Disable ANSI colors (auto-disabled when stdout is not a TTY)
    #[arg(long)]
    no_color: bool,

    /// HTTP timeout in seconds
    #[arg(long, default_value_t = 20)]
    timeout: u64,

    /// Cache TTL in seconds
    #[arg(long, default_value_t = 3600)]
    cache_ttl: u64,

    /// Do not read/write cache
    #[arg(long)]
    no_cache: bool,

    /// Retry count for transient HTTP errors
    #[arg(long, default_value_t = 2)]
    retries: usize,

    /// Backoff delay between retries (ms)
    #[arg(long, default_value_t = 300)]
    retry_delay_ms: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Resolve a single query (domain, IP, or ASN)
    Get {
        /// Query: example.com | 1.1.1.1 | AS13335
        query: String,

        /// Emit shell completions for <bash|zsh|fish> to stdout
        #[arg(long, value_enum)]
        completions: Option<Shell>,
    },

    /// Resolve many queries from a file (one per line)
    Bulk {
        /// File containing queries
        file: PathBuf,

        /// Max concurrent requests
        #[arg(long, default_value_t = 8)]
        concurrency: usize,

        /// Emit Newline-Delimited JSON (one JSON per line)
        #[arg(long)]
        ndjson: bool,
    },

    /// Inspect or clear cache
    Cache {
        #[command(subcommand)]
        action: CacheCmd,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Shell {
    Bash,
    Zsh,
    Fish,
}

#[derive(Subcommand, Debug)]
enum CacheCmd {
    /// List cached JSON files
    List,
    /// Remove cached JSON files
    Clear,
}

/* ----------------------------- HTTP + RDAP ------------------------------ */

fn http_client(timeout_secs: u64) -> Result<reqwest::Client, Box<dyn Error>> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("rdapx/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(timeout_secs))
        .gzip(true)
        .brotli(true)
        .deflate(true)
        .build()?;
    Ok(client)
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Domain,
    Ip,
    Asn,
}

fn normalize(query: &str) -> (Kind, String) {
    // quick’n'tidy
    let s = query.trim();
    if s.starts_with(|c: char| ['A', 'a', 'S', 's'].contains(&c)) {
        // AS13335 -> 13335
        let num = s.trim_start_matches(|c: char| ['A', 'a', 'S', 's'].contains(&c));
        return (Kind::Asn, num.to_string());
    }
    if s.contains(':') || s.split('.').all(|p| p.parse::<u8>().is_ok()) {
        return (Kind::Ip, s.to_string());
    }
    (Kind::Domain, s.to_string())
}

fn classify_to_url(kind: Kind, normalized: &str) -> String {
    match kind {
        Kind::Domain => format!("https://rdap.verisign.com/com/v1/domain/{normalized}"),
        Kind::Ip => format!("https://rdap.apnic.net/ip/{normalized}"),
        Kind::Asn => format!("https://rdap.arin.net/registry/autnum/{normalized}"),
    }
}

/* ------------------------------ CACHING --------------------------------- */
fn cache_dir() -> io::Result<PathBuf> {
    let base = BaseDirs::new().ok_or_else(|| io::Error::other("no home"))?;
    let p = base.cache_dir().join("rdapx");
    if !p.exists() {
        fs::create_dir_all(&p)?;
    }
    Ok(p)
}

fn cache_key(normalized_url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    normalized_url.hash(&mut h);
    format!("{:016x}.json", h.finish())
}

fn cache_path(url: &str) -> io::Result<PathBuf> {
    Ok(cache_dir()?.join(cache_key(url)))
}

fn load_cache(url: &str, ttl: Duration) -> io::Result<Option<Value>> {
    let p = cache_path(url)?;
    let meta = fs::metadata(&p)?;
    let age_ok = meta
        .modified()
        .ok()
        .and_then(|m| SystemTime::now().duration_since(m).ok())
        .is_some_and(|age| age <= ttl);
    if age_ok {
        let raw = fs::read_to_string(p)?;
        let v: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        return Ok(Some(v));
    }
    Ok(None)
}

fn save_cache(url: &str, json: &Value) -> io::Result<()> {
    let p = cache_path(url)?;
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(p, serde_json::to_string(json)?)?;
    Ok(())
}

fn list_cache(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                out.push(p);
            }
        }
    }
    out
}

fn clear_cache(dir: &Path) -> usize {
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") && fs::remove_file(p).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}

/* ------------------------------ OUTPUT ---------------------------------- */
fn output(json: &Value, fmt: Format) {
    match fmt {
        Format::Json => {
            // compact JSON
            println!("{json}");
        }
        Format::Pretty => {
            // pretty JSON
            println!("{}", serde_json::to_string_pretty(json).unwrap());
        }
        Format::Table => {
            use colored::Colorize;
            use serde_json::Value;
            use std::collections::BTreeSet;
            use std::io::IsTerminal;

            let use_color = std::io::stdout().is_terminal();

            // Helper to pull a string field from the top-level object
            let field = |k: &str| -> String {
                json.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string()
            };

            let kind = field("objectClassName"); // RDAP's type name
            let handle = field("handle");
            let name = field("name");
            let country = field("country");

            let status = json.get("status").and_then(Value::as_array).map_or_else(
                || "-".to_string(),
                |a| {
                    if a.is_empty() {
                        "-".to_string()
                    } else {
                        a.iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(",")
                    }
                },
            );

            if use_color {
                println!("{} {}", "Type:".blue().bold(), kind);
                println!("{} {}", "Handle:".blue().bold(), handle);
                println!("{} {}", "Name:".blue().bold(), name);
                println!("{} {}", "Country:".blue().bold(), country);
                println!("{} {}", "Status:".blue().bold(), status);
            } else {
                println!("Type: {kind}");
                println!("Handle: {handle}");
                println!("Name: {name}");
                println!("Country: {country}");
                println!("Status: {status}");
            }

            // Derive roles from entities (sorted, unique)
            if let Some(entities) = json.get("entities").and_then(Value::as_array) {
                let mut roles = BTreeSet::new();
                for e in entities {
                    if let Some(rs) = e.get("roles").and_then(Value::as_array) {
                        for r in rs {
                            if let Some(s) = r.as_str() {
                                roles.insert(s.to_string());
                            }
                        }
                    }
                }
                if !roles.is_empty() {
                    let joined = roles.into_iter().collect::<Vec<_>>().join(", ");
                    if use_color {
                        println!("{} {}", "Roles:".yellow().bold(), joined);
                    } else {
                        println!("Roles: {joined}");
                    }
                }
            }
        }
    }
}
/* ------------------------------ IO utils -------------------------------- */

fn read_lines(path: &Path) -> io::Result<Vec<String>> {
    let mut buf = String::new();
    fs::File::open(path)?.read_to_string(&mut buf)?;
    Ok(buf
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/* ------------------------------ Fetch ----------------------------------- */

async fn fetch_for_query(
    client: &reqwest::Client,
    q: &str,
    ttl: Duration,
    no_cache: bool,
    retries: usize,
    retry_delay_ms: u64,
) -> Result<Value, Box<dyn Error>> {
    let (kind, norm) = normalize(q);
    let url = classify_to_url(kind, &norm);

    if !no_cache {
        if let Ok(Some(v)) = load_cache(&url, ttl) {
            return Ok(v);
        }
    }

    // retry loop
    let mut last_err: Option<reqwest::Error> = None;
    for attempt in 0..=retries {
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let v: Value = resp.json().await?;
                if !no_cache {
                    let _ = save_cache(&url, &v);
                }
                return Ok(v);
            }
            Ok(resp) => {
                let code = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("HTTP {code}: {body}").into());
            }
            Err(e) => {
                last_err = Some(e);
                if attempt < retries {
                    sleep(Duration::from_millis(retry_delay_ms)).await;
                }
            }
        }
    }

    Err(format!("network error for {url}: {}", last_err.unwrap()).into())
}

/* --------------------------------- MAIN ---------------------------------- */

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut cli = Cli::parse();

    // Completions (only for `get --completions <shell>`)
    if let Command::Get {
        completions: Some(sh),
        ..
    } = &cli.command
    {
        let mut cmd = Cli::command();
        match sh {
            Shell::Bash => generate(Bash, &mut cmd, "rdapx", &mut io::stdout()),
            Shell::Zsh => generate(Zsh, &mut cmd, "rdapx", &mut io::stdout()),
            Shell::Fish => generate(Fish, &mut cmd, "rdapx", &mut io::stdout()),
        }
        return Ok(());
    }

    // Auto-disable color if piped
    if !io::stdout().is_terminal() {
        cli.no_color = true;
        colored::control::set_override(false);
    } else if cli.no_color {
        colored::control::set_override(false);
    }

    match &cli.command {
        Command::Get { query, .. } => {
            let client = http_client(cli.timeout)?;
            let ttl = Duration::from_secs(cli.cache_ttl);
            let json = fetch_for_query(
                &client,
                query,
                ttl,
                cli.no_cache,
                cli.retries,
                cli.retry_delay_ms,
            )
            .await?;
            output(&json, cli.format);
        }

        Command::Bulk {
            file,
            concurrency,
            ndjson,
        } => {
            let client = http_client(cli.timeout)?;
            let ttl = Duration::from_secs(cli.cache_ttl);
            let items = read_lines(file)?;
            if items.is_empty() {
                eprintln!("{} no queries found in file", "Note:".yellow().bold());
                return Ok(());
            }

            // Prefer NDJSON for JSON formats
            let ndjson_mode: bool = matches!(cli.format, Format::Json | Format::Pretty) && *ndjson;

            // Copy Format once for the async closures
            let fmt: Format = cli.format;

            let conc: usize = (*concurrency).max(1);

            stream::iter(items.into_iter())
                .map(|q: String| {
                    let client = &client;
                    async move {
                        match fetch_for_query(
                            client,
                            &q,
                            ttl,
                            cli.no_cache,
                            cli.retries,
                            cli.retry_delay_ms,
                        )
                        .await
                        {
                            Ok(json) => Ok((q, json)),
                            Err(e) => Err((q, e)),
                        }
                    }
                })
                .buffer_unordered(conc)
                .for_each(|res| async {
                    match res {
                        Ok((_q, json)) => {
                            if ndjson_mode {
                                println!("{}", serde_json::to_string(&json).unwrap());
                            } else {
                                output(&json, fmt);
                            }
                        }
                        Err((q, e)) => eprintln!("{} {q}: {e}", "Failed".red().bold()),
                    }
                })
                .await;
        }

        Command::Cache { action } => match action {
            CacheCmd::List => {
                let dir = cache_dir().unwrap_or_else(|_| PathBuf::from("./.cache/rdapx"));
                let entries = list_cache(&dir);
                if entries.is_empty() {
                    println!("(empty)");
                } else {
                    for p in entries {
                        println!("{}", p.display());
                    }
                }
            }
            CacheCmd::Clear => {
                let dir = cache_dir().unwrap_or_else(|_| PathBuf::from("./.cache/rdapx"));
                let n = clear_cache(&dir);
                println!("Cleared {n} cached files");
            }
        },
    }

    Ok(())
}
