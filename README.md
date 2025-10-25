# rdapx
[![CI](https://github.com/evozeus/rdapx/actions/workflows/ci.yml/badge.svg)](https://github.com/evozeus/rdapx/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/rdapx.svg)](https://crates.io/crates/rdapx)
⚡ A fast, modern RDAP client written in Rust — bulk lookups, caching, and clean JSON/NDJSON or table output.

---

## ✨ Features

- RDAP over HTTPS — modern replacement for `whois`
- Bulk lookups from files or pipelines
- Local caching with TTL expiry
- Formatted output (JSON, NDJSON, or table)
- Colorized terminal output with smart TTY detection
- Configurable concurrency for fast parallel lookups
- Clippy-clean, async-optimized, Rust 2021

---

## 🚀 Installation

**From source:**
1. Clone the repo  
   `git clone https://github.com/evozeus/rdapx.git`
2. Change directory  
   `cd rdapx`
3. Install  
   `cargo install --path .`

**From crates.io (after release):**  
`cargo install rdapx`

---

## 🧰 Usage

Single IP or domain:  
`rdapx get 1.1.1.1`  
`rdapx get example.com`

Custom format:  
`rdapx --format table get example.org`  
`rdapx --format json get 8.8.8.8`

Bulk mode (reads queries from file):  
`rdapx bulk --file targets.txt --concurrency 8`

Show help:  
`rdapx --help`

---

## ⚡ Example Output

**Table format**

Type:     ip network  
Handle:   1.1.1.0 - 1.1.1.255  
Name:     APNIC-LABS  
Country:  AU  
Status:   active  
Roles:    abuse, administrative, registrant, technical  

**JSON format**

{
  "type": "ip network",
  "handle": "1.1.1.0 - 1.1.1.255",
  "name": "APNIC-LABS",
  "country": "AU",
  "status": "active",
  "roles": ["abuse", "administrative", "registrant", "technical"]
}

---

## 🧩 Configuration

Default settings:  
- Cache directory: `~/.cache/rdapx`  
- Cache TTL: 24 hours  
- Timeout: 10 seconds  
- Max concurrency: 8  

Override via CLI flags or a config file at `~/.config/rdapx/config.toml`.

---

## 🧪 Development

`cargo fmt`  
`cargo clippy -- -D warnings`  
`cargo run -- --format table get 1.1.1.1`  
`cargo test`

---

## 🪪 License

MIT © 2025 Evozeus

---

## 🌐 Links

GitHub: https://github.com/evozeus/rdapx  
Crates.io: https://crates.io/crates/rdapx  
