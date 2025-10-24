# Changelog

All notable changes to **rdapx** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),  
and this project adheres to [Semantic Versioning](https://semver.org/).

---

## [0.1.0] — 2025-10-24

### Added
- Initial release of **rdapx**
- RDAP lookup for IPs, domains, and ASNs
- Support for JSON, NDJSON, and table output formats
- Bulk lookup with configurable concurrency
- Local caching with TTL expiry
- CLI built on Clap 4 with colored output
- Fully async using Tokio + Reqwest
- Clippy-clean, Rust 2021 edition

### Fixed
- All compiler warnings resolved
- Stable output handling and error reporting

---

## [Unreleased]

Planned:
- Configurable output templates
- Reverse lookup mode
- Smart offline cache indexing
- Export results to CSV/SQLite
- Optional integration with `jq` pipelines
