# HyprDrive

**The personal data OS.** A cross-platform, P2P-first, content-addressed filesystem indexer with WizTree-speed disk intelligence — built in Rust.

HyprDrive is a background daemon that indexes all your files across every device, drive, and cloud account. It identifies files by content (BLAKE3 hash), not by path — so duplicates are detected instantly, syncs are conflict-free, and moves/renames are tracked without re-reading file data.

> **Status:** Active development. Core indexing pipeline, deferred hashing, real-time file watching, and disk intelligence are implemented and tested. P2P networking, encryption, extensions, and UI are planned.

---

## Features

**Implemented:**
- **Platform-native scanning** — NTFS MFT on Windows, io_uring on Linux, getattrlistbulk on macOS
- **Deferred content hashing** — 10x faster first scan by generating synthetic IDs from metadata, then hashing file content in the background
- **Real-time file watching** — USN journal (Windows) / inotify (Linux) with event coalescing and debouncing
- **Content-addressed storage** — BLAKE3 hashing with inode cache (95%+ cache hit rate on re-scans)
- **Disk intelligence** — volume summary, top-N by size, wasted space analysis, duplicate detection
- **Duplicate detection** — three strategies: content hash (exact), fuzzy filename (Jaro-Winkler), perceptual image matching (blockhash)
- **SQLite metadata store** — WAL mode, 13 migrations, bulk load mode for 10-20x import speedup
- **redb inode cache** — zero-copy KV store for sub-microsecond cache lookups

**Planned:**
- P2P networking (Iroh/QUIC), file transfer (Blip engine), sync (CRDTs)
- End-to-end encryption (ChaCha20-Poly1305), capability tokens
- WASM extension system (wasmtime), 7 built-in extensions
- Desktop (Tauri), web, mobile (React Native) interfaces
- Full-text + semantic search (Tantivy + HNSW)

## Architecture

```
hyprdrive-daemon
|
+-- Indexer            Scans drives via platform-native APIs
+-- Object Pipeline    Batch hashing + DB upsert (deferred or real)
+-- Background Hasher  Upgrades synthetic -> real BLAKE3 hashes
+-- File Watcher       Real-time USN/inotify with event coalescing
+-- Change Processor   Dispatches create/delete/move/modify events
+-- Dedup Engine       Content + fuzzy + perceptual duplicate detection
+-- Disk Intelligence  Volume stats, treemap, wasted space analysis
|
+-- SQLite (sqlx)      Metadata store (WAL, 13 migrations)
+-- redb               Inode cache, cursor store, thumbnails
```

See [Architecture.md](Architecture.md) for the full specification (v4.1).

## Project Structure

```
HyprDrive/
+-- apps/
|   +-- daemon/              Background service (the system)
|   +-- cli/                 Command-line client
|   +-- tauri/               Desktop app (scaffold)
|   +-- web/                 Web app (scaffold)
|
+-- core/                    Database, migrations, domain types
|   +-- src/db/              SQLite pool, queries, types, cache
|   +-- src/domain/          ObjectId, enums, filters, tags, sync
|   +-- migrations/          001-013 SQL migration files
|
+-- crates/
|   +-- fs-indexer/          MFT/io_uring/getattrlistbulk scanning + USN/inotify listeners
|   +-- object-pipeline/     Hashing, pipeline, background hasher, change processor
|   +-- dedup-engine/        BLAKE3 progressive + Jaro-Winkler + blockhash
|   +-- disk-intelligence/   Volume stats, treemap, wasted space
|   +-- crypto/              ChaCha20, Ed25519, key management (scaffold)
|   +-- search/              Tantivy + HNSW (scaffold)
|   +-- ...                  Additional crates (see Cargo.toml)
|
+-- helpers/
|   +-- hyprdrive-helper-windows/   MFT access (admin privileges)
|   +-- hyprdrive-helper-macos/     Full Disk Access (XPC)
|   +-- hyprdrive-helper-linux/     fanotify (root)
|
+-- docs/
    +-- architecture/        ADR decision records
```

## Getting Started

### Prerequisites

- **Rust** 1.80+ (edition 2021)
- **Windows 10+**, **macOS 12+**, or **Linux 5.15+**
- Admin/root for MFT/fanotify scanning (optional — falls back to normal scanning)

### Build

```bash
# Debug build
cargo build --workspace

# Release build (optimized, stripped)
cargo build --workspace --release

# Run the daemon
cargo run -p hyprdrive-daemon --release
```

### Test

```bash
# Run all tests
cargo test --workspace

# Run with specific package
cargo test -p hyprdrive-object-pipeline
cargo test -p hyprdrive-dedup-engine
cargo test -p hyprdrive-fs-indexer

# Run benchmarks
cargo bench -p hyprdrive-object-pipeline
```

### Lint

```bash
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

### Cross-Platform Testing (Windows + WSL)

```bash
# Windows (native)
cargo test --workspace

# Linux (via WSL)
wsl -d Ubuntu -e bash -c "source ~/.cargo/env && cd /mnt/d/HyprDrive && cargo test --workspace"
```

## Key Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Database | SQLite + sqlx | Embedded, single-file, billions of deployments |
| Hashing | BLAKE3 | 4 GB/s, auto-parallelized, SIMD-accelerated |
| Cache | redb | Zero-copy, embedded KV, sub-microsecond reads |
| Encryption | ChaCha20-Poly1305 | Fast on ARM + x86, one cipher = fewer bugs |
| Extensions | wasmtime | 10 MB/extension, 5ms load, epoch-based timeout |
| P2P | Iroh (QUIC) | Hole-punching, mDNS, relay fallback |

See [docs/architecture/](docs/architecture/) for detailed ADRs.

## Performance Targets

| Metric | Target |
|--------|--------|
| MFT scan 100K files (Windows) | < 1.5s |
| USN change detection | < 200ms |
| BLAKE3 hash 1 GB | < 1s |
| list_files_fast (100K) | < 5ms |
| Inode cache hit | < 1us |

## License

MIT — see [Cargo.toml](Cargo.toml) for details.
