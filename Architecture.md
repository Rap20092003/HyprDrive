# HyprDrive — Virtual Distributed File System

## Architecture & Implementation Specification v4.0

> **The personal data OS.** Cross-platform, P2P-first, content-addressed,
> cryptographically sovereign, with WizTree-speed disk intelligence.

> [!NOTE]
> This document is written so that **anyone** — even a complete beginner — can
> read it and fully understand how HyprDrive works. Every concept is explained from
> first principles. No prior systems knowledge is assumed.

---

## Table of Contents

**Part I — What Is HyprDrive?**
1. [The Problem We're Solving](#1-the-problem-were-solving)
2. [What HyprDrive Actually Is](#2-what-HyprDrive-actually-is)
3. [Core Principles (The Rules We Never Break)](#3-core-principles)
4. [Locked Architectural Decisions](#4-locked-architectural-decisions)

**Part II — How The System Is Built**
5. [The Daemon (The Brain)](#5-the-daemon)
6. [Project Structure (Where Everything Lives)](#6-project-structure)
7. [How We Read Files Insanely Fast (Platform Indexing)](#7-platform-indexing)
8. [Content Addressing & Hashing](#8-content-addressing--hashing)
9. [The Database Layer](#9-the-database-layer)

**Part III — Intelligence & Operations**
10. [Disk Intelligence (The WizTree Engine)](#10-disk-intelligence)
11. [Operations Layer (CQRS — How Actions Work)](#11-operations-layer)
12. [File Watching (Real-Time Updates)](#12-file-watching)

**Part IV — Security & Networking**
13. [Cryptography (How We Keep Data Safe)](#13-cryptography)
14. [P2P Networking (How Devices Talk)](#14-p2p-networking)
15. [File Transfer (Blip Engine)](#15-file-transfer)
16. [Sync (Keeping Devices In Agreement)](#16-sync)
17. [Cloud & Cold Storage (OpenDAL)](#17-cloud--cold-storage)

**Part V — Search & Media**
18. [Unified Search](#18-unified-search)
19. [Media Pipeline](#19-media-pipeline)

**Part VI — Extensions & Integrations**
20. [WASM Extension System](#20-wasm-extension-system)
21. [Extension Apps](#21-extension-apps)
22. [External Integrations](#22-external-integrations)

**Part VII — User Interfaces**
23. [Interface Architecture](#23-interface-architecture)
24. [Desktop App (Tauri)](#24-desktop-app)
25. [Mobile App (React Native)](#25-mobile-app)
26. [Web App & Docker](#26-web-app--docker)

**Part VIII — Reference**
27. [Performance Targets](#27-performance-targets)
28. [Hardware & OS Requirements](#28-hardware--os-requirements)
29. [Glossary](#29-glossary)

---

# Part I — What Is HyprDrive?

---

## 1. The Problem We're Solving

### The World Today

Your files are **everywhere**:

- Photos on your phone (10,000+)
- Documents on your laptop
- Backups on an external drive
- PDFs in Google Drive
- Old projects on a second computer
- Music scattered across folders

And every tool you use only sees **one tiny piece**:

| Tool | What It Sees | What It Misses |
|------|-------------|---------------|
| Finder / Explorer | Files on this one computer | Your phone, cloud, other computers |
| Google Drive | Files in Google Drive | Your local files, Dropbox |
| Dropbox | Files in Dropbox | Your phone photos, local files |
| iCloud | Apple devices only | Windows, Linux, Android |
| WinDirStat / WizTree | Disk usage on this drive | Other drives, other computers |

**The result**: You can never answer simple questions like:
- "Where is that PDF I downloaded last month?"
- "Do I have a backup of my wedding photos?"
- "What's eating 200 GB on my laptop?"
- "How do I send this folder to my other computer?"

### What We Want Instead

One system that:
1. **Sees everything** — every file on every device, every cloud, every drive
2. **Is instant** — browsing 1 million files feels like browsing 100
3. **Is private** — your data never touches our servers
4. **Is smart** — finds duplicates, wasted space, organizes photos, extracts text
5. **Transfers fast** — send files between devices at wire speed, no cloud middleman
6. **Syncs automatically** — changes on one device appear on all others
7. **Is extensible** — plugins for photos, documents, finances, and more

That system is **HyprDrive**.

---

## 2. What HyprDrive Actually Is

### The One-Sentence Version

> HyprDrive is a **background service** (daemon) that indexes all your files, understands
> their content, and lets you browse, search, transfer, and sync them across every
> device you own — with end-to-end encryption throughout.

### The Analogy

Think of HyprDrive like a **personal librarian** who:

1. **Knows every book** in every room of your house (indexing)
2. **Remembers what's inside** each book (metadata, search)
3. **Can instantly find** any book you describe (search)
4. **Notices when you move** a book (file watching)
5. **Keeps a catalog** that's always up to date (database)
6. **Can send copies** to your friend's house instantly (transfer)
7. **Makes sure all rooms** have the same books (sync)
8. **Locks the vault** so only you have the key (encryption)

### What HyprDrive Is NOT

- ❌ Not a cloud storage service (your data stays on YOUR devices)
- ❌ Not a file sync tool like Dropbox (it does sync, but that's just one feature)
- ❌ Not a backup tool (it can help with backups, but it's not its primary job)
- ❌ Not a web app (it runs natively on your machine)

---

## 3. Core Principles

These are the **absolute rules** that every part of HyprDrive follows. They are never
violated, no matter what.

### Principle 1: Daemon-First

> **"The daemon IS the system. Everything else is a window into it."**

```
What this means:

  There is ONE background process (the "daemon") that does ALL the work:
  - Indexes files
  - Manages the database
  - Handles encryption
  - Runs P2P networking
  - Executes extensions

  Everything you SEE (the desktop app, the mobile app, the web app, the CLI)
  is just a "thin client" — a window that ASKS the daemon for information
  and SHOWS you the result.

  Think of it like a restaurant:
  - The daemon is the KITCHEN (where all the cooking happens)
  - The apps are WAITERS (they take your order and bring your food)
  - The waiters don't cook. They just communicate with the kitchen.
```

**Why this matters**: If the kitchen is great, every waiter serves great food.
Fix one bug in the daemon → fixed for ALL apps simultaneously.

### Principle 2: Local-First

> **"Works without internet. Always."**

Your files, your database, your search — everything works offline. The network
is a bonus for sync and transfer, never a requirement.

### Principle 3: Content-Addressed

> **"Files are identified by WHAT they contain, not WHERE they are."**

```
Traditional file systems:
  /Users/alice/photos/vacation.jpg  ← identified by PATH

HyprDrive:
  blake3:a7f3b2c9d1e4...  ← identified by CONTENT HASH

Why? Because the same photo might exist in:
  - /Users/alice/photos/vacation.jpg
  - /Users/alice/Desktop/copy_of_vacation.jpg
  - /Users/alice/Dropbox/backup/vacation.jpg

HyprDrive knows these are ALL THE SAME FILE because they have the same hash.
It stores the content ONCE and tracks all the locations separately.
```

### Principle 4: Privacy by Default

> **"Zero trust. End-to-end encryption. Your keys, your data."**

- All data transferred between devices is encrypted
- Only YOU hold the decryption keys
- No HyprDrive server ever sees your files
- Recovery via a 24-word phrase that only you know

### Principle 5: Test-Driven Development (TDD)

> **"No code exists without a test that proves it works."**

Every feature is built by:
1. Write a test that describes what the feature should do
2. Run the test — it fails (because the feature doesn't exist yet)
3. Write the code to make the test pass
4. Clean up the code
5. Repeat

---

## 4. Locked Architectural Decisions

These decisions have been made and will **not** be revisited. Each has a detailed
justification in the [Decisions Document](decisions.md).

### Decision 1: wasmtime (Not Wasmer)

**What**: The engine that runs extension plugins.

| | Wasmer | wasmtime (CHOSEN) |
|---|---|---|
| RAM per extension | ~100 MB | ~10 MB |
| Load time | ~200ms | ~5ms |
| Timeout handling | Manual | Automatic (epoch) |
| Backed by | Wasmer Inc. | Mozilla, Fastly, Intel, Red Hat |

**Why wasmtime**: Running 7 extensions at 100 MB each = 700 MB (Wasmer).
At 10 MB each = 70 MB (wasmtime). On mobile, this is the difference between
"runs" and "crashes."

### Decision 2: ChaCha20-Poly1305 Only (No AES-GCM)

**What**: The encryption algorithm used everywhere.

**Why one cipher**: One algorithm = one code path = one set of tests = fewer bugs.
ChaCha20 works equally fast on phones (ARM) and desktops (x86). AES-GCM only
shines with special hardware (AES-NI), which not all devices have.

**Who else uses it**: WireGuard, Signal, Cloudflare. If it secures billions of
connections, it's good enough for HyprDrive.

### Decision 3: fs-indexer (Not fs-watcher)

**What**: The crate that reads files from your disk.

**Why "indexer"**: It does THREE things:
1. **Scans** — reads millions of files from the disk (indexing)
2. **Watches** — detects new/changed/deleted files in real-time
3. **Deltas** — computes what changed since last scan

"Watcher" implies only #2. "Indexer" covers all three.

### Decision 4: Daemon Serves the API (No Separate Server)

**What**: The daemon itself runs an HTTP server. There is no separate API process.

```
The daemon listens on three ports:

  Port 7420 — WebSocket (for desktop/web/mobile apps, real-time)
  Port 7421 — HTTP REST API (for external tools, share links, health checks)
  Port 7422 — Prometheus metrics (for monitoring)
```

### Decision 5: Deferred Prototypes

The following are **removed** from the project until after launch:
- `apps/ios/` — redundant with `apps/mobile/` (React Native covers iOS)
- `apps/macos/` — redundant with `apps/tauri/` (Tauri covers macOS)
- `apps/gpui-photo-grid/` — GPUI is experimental and unstable

---

# Part II — How The System Is Built

---

## 5. The Daemon

### What Is a Daemon?

A **daemon** (pronounced "dee-mon") is a program that runs in the background,
like a service. You don't see a window for it — it just runs silently,
doing its job.

**Examples you already use**:
- Spotify's background process (plays music)
- Dropbox's sync agent (syncs files)
- Docker Desktop's engine (runs containers)

HyprDrive's daemon is called `hyprdrive-daemon`. It is **the** system.

### What the Daemon Owns

```
hyprdrive-daemon (THE system)
│
├── Database        — SQLite database with all file metadata
├── Indexer         — Scans your drives for files
├── File Watcher    — Detects changes in real-time (USN listener)
├── Dedup Engine    — Finds duplicate files (content + fuzzy + perceptual)
├── Hasher          — Computes content hashes (BLAKE3)
├── Crypto Engine   — Encrypts/decrypts data (ChaCha20)
├── P2P Node        — Connects to other devices (Iroh + QUIC)
├── Transfer Engine — Sends/receives files (Blip)
├── Sync Engine     — Keeps devices in agreement (CRDTs)
├── Extension Host  — Runs WASM plugins (wasmtime)
├── Media Worker    — Generates thumbnails, extracts metadata
├── Search Index    — Full-text + semantic search (Tantivy)
├── Task Queue      — Manages background jobs
├── Event Bus       — Notifies all components of changes
│
├── WebSocket :7420 — Real-time connection for apps
├── HTTP API :7421  — REST endpoints for external tools
└── Metrics :7422   — Prometheus monitoring
```

### Daemon Lifecycle

```
STARTUP:
  1. Read config file (or create defaults)
  2. Open SQLite database (create if first run)
  3. Start Iroh P2P node (begin discovering other devices)
  4. Start file watchers on all indexed locations
  5. Start WebSocket server on :7420
  6. Start HTTP server on :7421
  7. Start metrics server on :7422
  8. Load WASM extensions
  9. Signal "ready" to any waiting clients

RUNNING:
  - Accept client connections (Tauri, CLI, web, mobile)
  - Process file system events (new/changed/deleted files)
  - Handle sync messages from peer devices
  - Execute extension logic
  - Respond to search queries
  - Manage file transfers

SHUTDOWN:
  1. Flush all pending database writes
  2. Save sync checkpoints
  3. Close P2P connections gracefully
  4. Stop file watchers
  5. Close WebSocket/HTTP servers
  6. Exit
```

### How Clients Talk to the Daemon

```
Every app connects to the daemon the same way:

  ┌──────────┐     WebSocket     ┌──────────────┐
  │  Tauri   │ ◄───────────────► │              │
  │ Desktop  │                   │              │
  └──────────┘                   │              │
                                 │   HyprDrive-      │
  ┌──────────┐     WebSocket     │   daemon     │
  │   Web    │ ◄───────────────► │              │
  │   App    │                   │  (THE system)│
  └──────────┘                   │              │
                                 │              │
  ┌──────────┐   Unix Socket     │              │
  │   CLI    │ ◄───────────────► │              │
  └──────────┘                   └──────────────┘

  ┌──────────┐                   ┌──────────────┐
  │  Mobile  │  ← EXCEPTION →   │ Embedded     │
  │   App    │  runs core        │ hyprdrive-core    │
  └──────────┘  in-process       └──────────────┘

  Mobile is the ONE exception: phones can't run a background daemon
  reliably, so the mobile app embeds hyprdrive-core directly. It connects
  to desktop daemons as a PEER for syncing.
```

---

## 6. Project Structure

This is where every file in the HyprDrive codebase lives. Think of it like
a building floor plan.

```
HyprDrive/
│
├── apps/                          ← Applications (what users interact with)
│   ├── daemon/                    ← THE system — background service
│   │   └── src/main.rs            ← Entry point: starts everything
│   ├── cli/                       ← Command-line client (thin)
│   │   └── src/main.rs            ← Connects to daemon via socket
│   ├── tauri/                     ← Desktop app (thin React client)
│   ├── tauri-lite/                ← Lightweight desktop (egui, <14 MB)
│   ├── web/                       ← Web app (connects via WebSocket)
│   ├── mobile/                    ← React Native app (iOS + Android)
│   ├── server/                    ← Docker config for self-hosting
│   └── landing/                   ← Marketing website (Next.js)
│
├── core/                          ← The brain — pure Rust library
│   └── src/
│       ├── domain/                ← Data types (zero I/O, pure logic)
│       │   ├── id.rs              ← ObjectId, LocationId, VolumeId...
│       │   ├── enums.rs           ← FileCategory, ObjectKind, StorageTier
│       │   ├── filter.rs          ← FilterExpr (powers all search/queries)
│       │   └── ...
│       ├── ops/                   ← Operations (move, copy, delete, tag)
│       ├── infra/                 ← Database, events, jobs, sync
│       ├── service/               ← High-level business logic
│       ├── crypto/                ← Key management + encryption
│       └── ...
│
├── crates/                        ← Specialized libraries
│   ├── fs-indexer/                ← File scanning (MFT, getattrlistbulk, io_uring)
│   ├── disk-intelligence/         ← WizTree engine (treemap, usage analysis)
│   ├── file-transfer/             ← Blip transfer (QUIC, resume, routing)
│   ├── crypto/                    ← Cryptographic primitives
│   ├── search/                    ← Tantivy + HNSW + RRF fusion
│   ├── ffmpeg/                    ← Video/audio processing
│   ├── images/                    ← Image processing (HEIF, PDF, SVG)
│   ├── media-metadata/            ← EXIF/audio/video metadata
│   ├── sdk/                       ← Extension SDK for plugin authors
│   ├── sdk-macros/                ← Rust macros for extensions
│   ├── dedup-engine/              ← Duplicate detection (BLAKE3, Jaro-Winkler, blockhash)
│   ├── task-system/               ← Background job execution
│   ├── actors/                    ← Actor concurrency framework
│   └── utils/                     ← Shared utilities
│
├── helpers/                       ← Privileged helper processes
│   ├── hyprdrive-helper-windows/       ← Windows: MFT access (needs admin)
│   ├── hyprdrive-helper-macos/         ← macOS: Full Disk Access (XPC)
│   └── hyprdrive-helper-linux/         ← Linux: fanotify (needs root)
│
├── extensions/                    ← WASM extension plugins
│   ├── photos/                    ← Face detection, moments, GPS maps
│   ├── chronicle/                 ← Document intelligence, entity extraction
│   ├── atlas/                     ← Contact/CRM management
│   ├── studio/                    ← Video editing tools
│   ├── ledger/                    ← Receipt scanning, expense tracking
│   ├── guardian/                  ← Backup redundancy monitoring
│   └── cipher/                    ← Password vault, file encryption
│
├── packages/                      ← Shared frontend code
│   ├── ui/                        ← Component library (Radix UI + Tailwind)
│   ├── interface/                 ← Shared React components + state
│   ├── ts-client/                 ← Auto-generated TypeScript bindings
│   ├── swift-client/              ← Auto-generated Swift bindings
│   └── assets/                    ← Icons and images
│
├── docs/                          ← Documentation
│   └── architecture/              ← ADR decision records
│
└── skills/                        ← AI development skills
```

---

## 7. Platform Indexing

### The Problem

To know about your files, HyprDrive needs to **read** them from your disk.
The naive approach — scanning every file one-by-one — is painfully slow:

```
Naive approach (readdir + stat):
  For each directory:
    List files in directory          ← 1 system call
    For each file:
      Get file size                  ← 1 system call
      Get timestamps                 ← 1 system call
      Get file name                  ← 1 system call

  100,000 files × 3 calls each = 300,000 system calls
  Time: ~45 seconds 😱
```

HyprDrive uses **platform-specific fast paths** to do this 10–30× faster.

### Windows — Reading the MFT (Master File Table)

```
What is the MFT?

  NTFS (Windows' file system) keeps a special database called the
  "Master File Table." It's a SINGLE FILE that contains metadata
  for EVERY file on the drive:
  - File name
  - Size (logical)
  - Allocated size (actual disk space used)
  - Timestamps (created, modified, accessed)
  - Parent directory reference

  Instead of asking "what's in this folder?" 10,000 times,
  we read the MFT ONCE and get ALL files in a single pass.

How HyprDrive reads the MFT:

  1. Open the drive volume (requires admin privilege)
  2. Issue FSCTL_ENUM_USN_DATA (a Windows API call)
  3. Windows streams ALL file records back in one burst
  4. Parse each record → extract name, size, allocated_size, timestamps
  5. Build the file tree from parent references

  100,000 files = 1 API call stream
  Time: < 1.5 seconds ✨

How HyprDrive detects changes (USN Journal):

  Windows also keeps a "change journal" — a log of every file
  operation (create, rename, delete, modify) since last check.

  1. Read journal entries since our last checkpoint
  2. Each entry tells us: "file X was created/renamed/deleted"
  3. Apply changes to our database
  4. Save new checkpoint

  This means HyprDrive can "catch up" after being off for weeks
  in under 100 milliseconds — no matter how many changes happened.

Privilege model:

  Reading the MFT requires SeManageVolumePrivilege (admin).
  HyprDrive handles this by installing a tiny helper service:

  hyprdrive-helper.exe (runs as Windows Service, has admin rights)
      ↕ communicates via named pipe
  hyprdrive-daemon (runs as normal user, no admin rights)

  The helper does ONLY MFT reads. Everything else runs unprivileged.
  If the helper isn't installed, HyprDrive falls back to normal scanning
  (slower, but still works).
```

### macOS — getattrlistbulk

```
What is getattrlistbulk?

  A macOS system call that returns attributes for UP TO 1,024 files
  in a SINGLE call. Compare:

  Normal: stat("file1") + stat("file2") + ... = 1,024 calls
  Bulk:   getattrlistbulk(directory) = 1 call for 1,024 files

How HyprDrive uses it:

  1. Open a directory
  2. Call getattrlistbulk asking for: name, size, allocated_size, timestamps
  3. Receive up to 1,024 file records in one buffer
  4. Repeat until directory is fully read

  100,000 files ÷ 1,024 per call = ~98 calls
  Time: < 4 seconds ✨

How HyprDrive detects changes (FSEvents):

  macOS has a built-in file event system. HyprDrive subscribes to it:

  1. Register interest in watched directories
  2. macOS notifies us of any changes
  3. We process the change events

Privilege model:

  Full Disk Access is needed to read all directories.
  HyprDrive uses an XPC service (macOS's official helper mechanism).
```

### Linux — io_uring + fanotify

```
What is io_uring?

  A high-performance async I/O interface in the Linux kernel.
  Instead of making system calls one at a time, io_uring lets
  you submit BATCHES of operations and harvest results later.

How HyprDrive uses it:

  1. Submit 64 getdents64 calls simultaneously via io_uring
  2. Kernel processes them in parallel
  3. Harvest all results at once
  4. Submit next batch

  This saturates the disk's I/O bandwidth — no time wasted
  waiting between calls.

  100,000 files = ~1,562 batches of 64
  Time: < 2 seconds ✨

How HyprDrive detects changes (fanotify):

  fanotify is Linux's file notification system. We use it with
  FAN_REPORT_FID (reports file by ID, not path — more reliable).

  Requires a setuid helper for the initial setup.
  Falls back to inotify if fanotify isn't available.
```

### The Unified Indexer

All three platform implementations share a common interface:

```
TRAIT VolumeIndexer:
  full_scan()      → Stream of FileEntry (all files on volume)
  delta_scan()     → Stream of FsChange (only what changed since last scan)
  current_cursor() → IndexCursor (checkpoint to resume from)

The daemon calls VolumeIndexer without knowing which platform it's on.
At compile time, Rust selects the right implementation:
  - Windows → MFT reader
  - macOS   → getattrlistbulk
  - Linux   → io_uring

Priority scanning:
  Not all directories are equal. HyprDrive scans in priority order:
  1. Desktop, Documents, Downloads    ← you care about these NOW
  2. Home directory                   ← important files
  3. External volumes                 ← probably looking for something
  4. node_modules, .git, __pycache__  ← scan last (or skip entirely)
```

---

## 8. Content Addressing & Hashing

### Why Hash Files?

Every file in HyprDrive gets a **content hash** — a unique fingerprint computed from
the file's actual bytes. Think of it like a DNA test for files.

```
What is a hash?

  A hash function takes ANY input and produces a fixed-size output:

  Input: "Hello, World!"    → Hash: a7f3b2c9...
  Input: "Hello, World!!"   → Hash: 5e2d1f8a...  (completely different!)
  Input: (a 50 GB video)    → Hash: c9d4e7b1...  (still same fixed size)

  Key properties:
  1. Same input ALWAYS produces same output (deterministic)
  2. Even a tiny change → completely different hash (avalanche effect)
  3. Can't reverse-engineer the input from the hash (one-way)
  4. Extremely unlikely two different files produce the same hash
```

### BLAKE3 — The Hash We Use

HyprDrive uses **BLAKE3** because it's the fastest cryptographic hash available:

```
Speed comparison (hashing 1 GB of data):

  SHA-256:   ~400 MB/s   ← 2.5 seconds
  SHA-512:   ~600 MB/s   ← 1.7 seconds
  BLAKE3:    ~4,000 MB/s ← 0.25 seconds ← THIS ONE

BLAKE3 is fast because:
  - It automatically uses all CPU cores
  - It can use SIMD instructions (process multiple data in one CPU cycle)
  - It was designed for speed without sacrificing security
```

### How Hashing Works in HyprDrive

```
WHEN a new file is discovered during indexing:

  1. Check the INODE CACHE:
     Key = (volume_id, inode_number, last_modified_time)

     IF cache HIT (file hasn't changed since last hash):
       → Return cached ObjectId (skip hashing entirely)
       → This saves MASSIVE time on re-scans (95%+ cache hit rate)

     IF cache MISS (new file or file was modified):
       → Continue to step 2

  2. Hash the file:
     IF file < 512 MB:
       → Read file in streaming chunks, feed to BLAKE3
     IF file ≥ 512 MB:
       → Memory-map the file (let the OS handle I/O efficiently)
       → Feed mapped memory to BLAKE3

  3. Generate ObjectId:
     ObjectId = first 32 bytes of BLAKE3 hash

  4. Upsert to database:
     → Store Object (content identity) in "objects" table
     → Store Location (where the file lives) in "locations" table
     → Same ObjectId + different Locations = duplicate detection!

  5. Update inode cache:
     → Save (volume_id, inode, mtime) → ObjectId for next time

  6. Emit event:
     → EventBus.emit("ObjectIndexed", { object_id, location_id })
     → All listeners (UI, sync, search) get notified
```

---

## 9. The Database Layer

### Why SQLite?

```
HyprDrive stores all file metadata in SQLite because:

  1. Embedded — no separate database server to install or configure
  2. Single file — your entire library DB is ONE file you can backup/copy
  3. Fast — with proper indexes, queries over 100k files take < 5ms
  4. Reliable — used by Firefox, Chrome, iOS, Android, and literally
     billions of devices
  5. Local-first — works offline, no network needed
```

### Key Tables

```
OBJECTS table — one row per unique piece of content
  ┌────────────┬──────────┬──────────┬────────────┐
  │ id (hash)  │ size     │ kind     │ created_at │
  ├────────────┼──────────┼──────────┼────────────┤
  │ a7f3b2c9.. │ 4200000  │ File     │ 2024-01-15 │
  │ 5e2d1f8a.. │ 0        │ Dir      │ 2024-01-15 │
  └────────────┴──────────┴──────────┴────────────┘

LOCATIONS table — one row per place a file exists
  ┌────────────┬────────────┬──────────────────────────┬──────────┐
  │ id         │ object_id  │ path                     │ volume   │
  ├────────────┼────────────┼──────────────────────────┼──────────┤
  │ loc_001    │ a7f3b2c9.. │ /Users/alice/photo.jpg   │ vol_mac  │
  │ loc_002    │ a7f3b2c9.. │ /Users/alice/backup.jpg  │ vol_mac  │
  └────────────┴────────────┴──────────────────────────┴──────────┘
  ↑ Same object_id = same content = DUPLICATE DETECTED!

Other important tables:
  - tags, tag_relations      → user-defined tags + hierarchy
  - metadata                 → EXIF, audio tags, PDF info
  - virtual_folders          → saved searches / smart folders
  - sync_operations          → change log for multi-device sync
  - dir_sizes                → pre-computed directory sizes (for treemap)
  - file_types               → 200+ extensions with categories + colors
  - files_fts                → Full-Text Search index (FTS5)
  - temporal_index           → dates from EXIF for timeline view
  - backlinks                → wiki-style [[links]] between files
```

### Performance Tuning

```
SQLite is configured for maximum speed:

  WAL mode          → readers don't block writers (concurrent access)
  synchronous=NORMAL → fast writes (slight risk in power loss, acceptable)
  journal_size=64MB  → limits disk usage for the write-ahead log
  mmap_size=256MB    → memory-map the database for faster reads

The critical query — list_files_fast():

  PURPOSE: Load the file list for a directory (what you see in the file browser)
  TARGET: < 5ms at 100,000 files

  HOW: Uses a carefully designed index (idx_loc_sort) that lets SQLite
  jump directly to the right rows without scanning the whole table.
  Think of it like a book's index — instead of reading every page to
  find "quantum physics," you look in the index and jump to page 342.
```

### redb Caches

```
Some data is accessed SO frequently that even SQLite isn't fast enough.
For these, HyprDrive uses redb — a zero-copy embedded key-value store:

  INODE_CACHE:      (volume, inode, mtime) → ObjectId
                    "Has this file changed since last scan?"

  THUMB_MANIFEST:   ObjectId → ThumbnailRecord
                    "Where is the thumbnail for this file?"

  QUERY_CACHE:      query_hash → cached_results (500ms TTL)
                    "I just ran this exact query 200ms ago"

  XFER_CHECKPOINTS: transfer_id → RoaringBitmap
                    "Which chunks have been sent in this transfer?"

  DIR_SIZE_CACHE:   location_id → DirSizeRecord
                    "How big is this directory?"

  USN_CURSORS:      volume_key → UsnCursorRecord
                    "Where did we last read in the USN journal?"
                    Enables the USN listener to resume after daemon restart
                    without missing any filesystem changes.
```

---

# Part III — Intelligence & Operations

---

## 10. Disk Intelligence

### What Is This?

Think of it as **WizTree built in**. HyprDrive doesn't just list files — it
understands how your disk space is being used and gives you actionable insights.

### The Treemap

```
A treemap is a visualization where:
  - Every rectangle represents a file or folder
  - Bigger rectangles = more disk space used
  - Rectangles are nested (folders contain files)
  - Colors represent file types

  ┌──────────────────────────────────────────────┐
  │ Videos (45 GB)                               │
  │  ┌──────────────────┐ ┌──────────────────┐   │
  │  │ vacation.mp4     │ │ project.mov      │   │
  │  │ (12 GB)          │ │ (8 GB)           │   │
  │  └──────────────────┘ └──────────────────┘   │
  ├──────────────────────────────────────────────┤
  │ Photos (20 GB)       │ Documents (5 GB)      │
  │  ┌───────┐ ┌──────┐ │ ┌──────┐ ┌─────┐      │
  │  │IMG_001│ │IMG_002│ │ │thesis│ │notes│      │
  │  └───────┘ └──────┘ │ └──────┘ └─────┘      │
  └──────────────────────┴───────────────────────┘

HyprDrive uses the "squarified treemap" algorithm (Bruls et al. 2000):
  - Rectangles are close to SQUARE shaped (easy to compare)
  - No gaps or overlaps (every pixel represents real data)
  - No rectangle thinner than 1:10 ratio (still readable)
```

### How Aggregation Works

```
Building the treemap requires knowing how big each directory is.
HyprDrive computes this bottom-up:

  /Users/alice/
  ├── photos/           ← cumulative = 20 GB
  │   ├── IMG_001.jpg   ← 5 MB
  │   ├── IMG_002.jpg   ← 8 MB
  │   └── wedding/      ← cumulative = 19.987 GB
  │       └── ...
  └── videos/           ← cumulative = 45 GB
      └── ...

  The "cumulative_allocated" field counts ALL bytes used by a directory
  and ALL its subdirectories. This uses the ALLOCATED size (actual disk
  space), not the logical size — so compressed/sparse files are accurate.

Live updates:

  When a file changes, HyprDrive "bubbles up" the size change:

  1. File added (5 MB) in /photos/wedding/
  2. Update /photos/wedding/ cumulative: +5 MB
  3. Update /photos/ cumulative: +5 MB
  4. Update /Users/alice/ cumulative: +5 MB
  
  This propagation takes < 1 millisecond.
  The treemap updates in real-time without full re-scanning.
```

### Disk Insights

```
HyprDrive automatically detects:

  LARGEST FILES:      Top 100 files by size
  LARGEST DIRECTORS:  Top 100 directories by cumulative size
  STALE FILES:        Files not accessed in 2+ years
  BUILD ARTIFACTS:    node_modules/, target/, __pycache__/, .gradle/
  DUPLICATES:         Multi-strategy duplicate detection (see Dedup Engine below)
  WASTED SPACE:       allocated_size − logical_size (filesystem overhead)
  TYPE BREAKDOWN:     Pie chart of space by file type (Video, Photo, etc.)
```

### Dedup Engine (Duplicate Detection)

```
HyprDrive includes a dedicated dedup-engine crate (inspired by dupeguru)
that goes far beyond simple hash matching. It uses THREE complementary
strategies to find duplicates:

STRATEGY 1: Content Hashing (BLAKE3 Progressive)
  The gold standard — finds exact byte-for-byte duplicates.

  Pipeline (eliminates non-duplicates cheaply at each stage):

    INPUT_FILES
         │
         ▼
    ┌─────────────────┐
    │ SIZE BUCKETING   │  Group by file size. Different size = different content.
    │ (free — no I/O)  │  Skip files with unique sizes immediately.
    └────────┬────────┘
             ▼
    ┌─────────────────┐
    │ PARTIAL HASH     │  Hash first 4KB of each file (BLAKE3).
    │ (cheap — 4KB)    │  Most non-duplicates eliminated here.
    └────────┬────────┘
             ▼
    ┌─────────────────┐
    │ FULL HASH        │  Hash entire file in 64KB streaming chunks.
    │ (expensive)      │  Files > 512MB use memory-mapped I/O.
    └────────┬────────┘
             ▼
    CONFIRMED DUPLICATES (same full BLAKE3 hash)

  All hashing stages use rayon for CPU-parallel computation.

STRATEGY 2: Fuzzy Filename Matching (Jaro-Winkler)
  Finds renamed copies like "report (1).pdf" or "Copy of report.pdf".

  Step 1 — Normalize names:
    "Copy of Budget.xlsx"    → "budget"
    "Report (1).pdf"         → "report"
    "photo - Copy.jpg"       → "photo"

  Step 2 — Group by extension (only compare .pdf with .pdf)
  Step 3 — Pairwise Jaro-Winkler similarity (threshold: 0.85)

  Note: Fuzzy matches suggest POTENTIAL duplicates. The user decides.

STRATEGY 3: Perceptual Image Matching (blockhash)
  Finds visually similar images even when resized, recompressed, or
  slightly modified — like finding that a 2MB JPEG and a 5MB PNG
  are actually the same photo.

  Uses the image_hasher crate with 16×16 blockhash:
    1. Load image → resize to 16×16 grid
    2. Compute perceptual hash (captures visual structure, not pixels)
    3. Compare hashes via Hamming distance (threshold: 10)

  Behind the "perceptual" feature flag (optional dependency).
  Supports: jpg, jpeg, png, webp, bmp, gif, tiff.

GROUPING (Union-Find):
  Matches are grouped transitively: if A=B and B=C, then {A,B,C} is one group.

  For each group, a REFERENCE file is selected (the likely "original"):
    - Shallowest path depth (fewer directories = more likely root copy)
    - Oldest modification time
    - No "copy" pattern in filename

  DupeReport output:
    - Duplicate groups with reference + duplicates
    - Total wasted bytes (sum of duplicate sizes, excluding reference)
    - Scan duration, files scanned, files skipped

  Example:
    ┌──────────────────────────────────────────────────────┐
    │ Group 1 (Content match, BLAKE3)                      │
    │ Reference: /photos/wedding/IMG_4521.jpg   (5.2 MB)   │
    │ Duplicate: /backup/old/IMG_4521.jpg       (5.2 MB)   │
    │ Duplicate: /photos/Copy of IMG_4521.jpg   (5.2 MB)   │
    │ Wasted:    10.4 MB                                    │
    └──────────────────────────────────────────────────────┘

Crate: crates/dedup-engine/
  src/hasher.rs      — Progressive BLAKE3 (partial + full + mmap)
  src/scanner.rs     — DuplicateScanner orchestrator + size bucketing
  src/fuzzy.rs       — Jaro-Winkler fuzzy matching + name normalization
  src/perceptual.rs  — Blockhash image matching (feature-gated)
  src/grouping.rs    — Union-find + reference selection + DupeGroup
  src/lib.rs         — FileEntry type + DupeReport + public API
```

---

## 11. Operations Layer

### What Is CQRS?

```
CQRS = Command Query Responsibility Segregation

In simple terms:
  - QUERIES read data (don't change anything)
  - COMMANDS change data (and are tracked for undo)

Every action in HyprDrive goes through this pipeline:

  ┌─────────┐    ┌──────────┐    ┌────────┐    ┌────────┐    ┌──────┐
  │ Preview │ →  │ Validate │ →  │ Commit │ →  │ Verify │ →  │ Undo │
  └─────────┘    └──────────┘    └────────┘    └────────┘    └──────┘

  PREVIEW:  Show the user what will happen BEFORE doing it
            "This will move 50 files from /photos to /archive"

  VALIDATE: Check that the operation is legal
            "Do all source files exist? Is the destination writable?"

  COMMIT:   Execute the operation and record it
            Move the files, update the database, emit events

  VERIFY:   Confirm the operation succeeded
            "Are all 50 files now in /archive? Are they intact?"

  UNDO:     Create the INVERSE operation and push to UndoStack
            "Undo = move all 50 files back to /photos"
```

### Supported Operations

```
Every operation listed here has: preview, commit, verify, and undo.

  MoveFiles     — Move files to a new location
  CopyFiles     — Duplicate files to a new location
  DeleteFiles   — Move files to trash (soft delete)
  RenameFile    — Change a file's name
  CreateDir     — Create a new directory
  BulkTag       — Add/remove tags from multiple files
  EmptyTrash    — Permanently delete trashed files
  SmartRename   — Rename using templates: {year}/{month}/{camera}
                  Example: IMG_001.jpg → 2024/January/Canon/IMG_001.jpg
```

### The Undo Stack

```
HyprDrive keeps the last 50 operations in an UndoStack.
Pressing Cmd+Z (Mac) or Ctrl+Z (Windows) replays the inverse:

  UndoStack (max 50):
  ┌───────────────────────────────────────────────────┐
  │ 1. MoveFiles: /photos → /archive (inverse: move back) │
  │ 2. BulkTag: added "vacation" to 20 files (inverse: remove) │
  │ 3. DeleteFiles: 3 files to trash (inverse: restore) │
  └───────────────────────────────────────────────────┘

  When stack reaches 50, the oldest entry is evicted.
```

---

## 12. File Watching

```
HyprDrive needs to know INSTANTLY when a file changes on disk.

How it works:

  1. Platform watcher detects a change:
     - Windows: USN journal polling (UsnListener — continuous background monitor)
     - macOS: FSEvents notification
     - Linux: fanotify event

  2. Debounce:
     If 1,000 files change in 100ms (e.g., npm install),
     combine them into ONE batch event instead of 1,000 individual events.

  3. Event pipeline:
     Change detected → Debounce (100ms window) → Hash new content
     → Update database → Emit "ObjectIndexed" event → UI re-renders

  4. UI invalidation:
     The daemon sends WebSocket messages to all connected clients.
     The React frontend uses TanStack Query, which automatically
     re-fetches data when it receives an invalidation signal.

  End-to-end target: file change on disk → UI updates in < 50ms
```

### Windows USN Listener (Real-Time)

```
On Windows, HyprDrive uses a continuous background USN journal listener
to detect filesystem changes in real-time — the same approach used by
Everything (Voidtools) for instant file search updates.

Architecture:

  ┌───────────────────────────────────────┐
  │         UsnListener                    │
  │  ┌─────────────────────────────────┐  │
  │  │  Volume Thread C:\              │  │
  │  │  loop {                         │  │
  │  │    poll_changes(cursor)         │──┼──→ mpsc::Sender<FsChange>
  │  │    persist_cursor(redb)         │  │         │
  │  │    sleep(100ms)                 │  │         ▼
  │  │  }                              │  │   mpsc::Receiver<FsChange>
  │  └─────────────────────────────────┘  │   (consumer: daemon pipeline)
  │  ┌─────────────────────────────────┐  │
  │  │  Volume Thread D:\              │  │
  │  │  (same loop)                    │  │
  │  └─────────────────────────────────┘  │
  │  CancellationToken → graceful stop    │
  └───────────────────────────────────────┘

How it works:

  1. UsnListener::start() spawns one tokio::spawn_blocking thread per volume
  2. Each thread calls poll_changes() in a loop (default: every 100ms)
  3. Changes arrive as FsChange events: Created, Deleted, Moved, Modified
  4. Cursor position is persisted to redb (USN_CURSORS table) after each batch
  5. On daemon restart, cursor is loaded from redb — no missed changes
  6. If the USN journal wraps or is recreated, emits FsChange::FullRescanNeeded

Key design:

  CursorStore trait    — Abstracts cursor persistence (redb, file, or no-op)
  NoCursorStore        — No-op implementation for testing
  ListenerConfig       — Builder pattern: poll_interval, channel_capacity, volumes
  CancellationToken    — Graceful shutdown from tokio-util
  Multi-volume         — Each drive (C:\, D:\, etc.) monitored independently

Event latency target: < 200ms from file change to FsChange event
```

---

# Part IV — Security & Networking

---

## 13. Cryptography

### Key Hierarchy (How Keys Are Organized)

```
Think of it like a tree of keys:

  Master Key
  (derived from your password via Argon2id)
  │
  ├── Device Key (Ed25519 keypair)
  │   Used to identify THIS device and sign messages
  │
  ├── Envelope Keys (one per file, derived via HKDF)
  │   Used to encrypt individual files
  │
  └── Capability Tokens (short-lived, per-transfer)
      Used to authorize specific actions between devices

Recovery:
  Your Master Key can be backed up as a 24-word BIP39 mnemonic phrase:
  "abandon ability able about above absent absorb abstract absurd abuse..."
  Write it down and store it safely. It's the ONLY way to recover
  if you forget your password.
```

### Streaming Encryption (ChaCha20-Poly1305)

```
Files are encrypted in CHUNKS, not all at once:

  ┌──────────────────────────────────────────────────────┐
  │ Original file (any size, could be 50 GB)             │
  └──────────────────────────────────────────────────────┘
                          ↓ split into chunks
  ┌──────────┬──────────┬──────────┬──────────┬──────────┐
  │ Chunk 0  │ Chunk 1  │ Chunk 2  │ Chunk 3  │ Chunk 4  │
  │ (64 KB)  │ (64 KB)  │ (64 KB)  │ (64 KB)  │ (remaining)
  └──────────┴──────────┴──────────┴──────────┴──────────┘
                          ↓ encrypt each chunk separately
  ┌──────────┬──────────┬──────────┬──────────┬──────────┐
  │ 🔒 Enc 0 │ 🔒 Enc 1 │ 🔒 Enc 2 │ 🔒 Enc 3 │ 🔒 Enc 4 │
  └──────────┴──────────┴──────────┴──────────┴──────────┘

  WHY chunks?
  1. You can decrypt chunk 5 without decrypting chunks 0-4
     → Enables "range requests" (play video from the middle)
  2. A corrupted chunk doesn't destroy the whole file
  3. Memory-efficient: only one chunk in RAM at a time

  Each chunk has its own nonce (random number) and authentication tag,
  so tampering with ANY byte of ANY chunk is detected.
```

### Capability Tokens

```
When Device A wants Device B to do something, it creates a
Capability Token — a signed permission slip:

  CapabilityToken {
    action:  "read_file"
    target:  "object_id:a7f3b2c9..."
    device:  "device_id:B"
    expires: "2024-01-15T12:00:00Z"
    nonce:   "unique_random_value"
    signature: Ed25519_sign(all_above_fields, device_A_private_key)
  }

  Device B verifies the signature and checks:
  - Is it signed by a paired device? ✅
  - Has it expired? ❌ (still valid)
  - Has the nonce been revoked? ❌ (not revoked)
  → Permission granted.

  If a device is lost/stolen, you add its device_id to the
  RevocationList. All tokens from that device become invalid instantly.
```

---

## 14. P2P Networking

### What Is Peer-to-Peer (P2P)?

```
Traditional (client-server):
  Your Phone → [Internet] → [Company Server] → [Internet] → Your Laptop
  Slow. Your data passes through someone else's server.

P2P (peer-to-peer):
  Your Phone ←→ Your Laptop
  Fast. Direct connection. No middleman. No data leaves your network.
```

### How HyprDrive Connects Devices

HyprDrive uses **Iroh** (by n0.computer) for P2P networking:

```
Iroh provides:
  - QUIC transport    → fast, encrypted, reliable (like TCP but better)
  - Hole-punching     → connects devices even behind firewalls/NAT
  - mDNS discovery    → finds devices on your local network automatically
  - Relay fallback    → if direct connection fails, relay via Iroh servers

Connection flow:

  1. DISCOVERY — find other HyprDrive devices
     The daemon broadcasts on the local network: "I'm HyprDrive device X"
     Other HyprDrive daemons hear this and respond: "I'm HyprDrive device Y"

  2. PAIRING — establish trust (one-time setup)
     Device A shows a QR code containing its public key
     Device B scans the QR code
     Both devices exchange Ed25519 keys → mutually authenticated
     Capability tokens are exchanged → authorized

  3. CONNECTION — ongoing communication
     After pairing, devices automatically connect whenever they're
     on the same network. If the network changes, the connection
     manager auto-reconnects.
```

---

## 15. File Transfer

### The Blip Transfer Engine

```
Goal: Transfer files at WIRE SPEED. No artificial bottlenecks.
Target: > 900 Mbps on a 1 Gbps LAN connection.

How it works:

  1. ROUTING — choose the fastest path
     ┌────────────────────────────────────────────────┐
     │ RoutingOracle decision tree (< 500ms):         │
     │                                                │
     │ Are devices on same subnet?                    │
     │   YES → Direct LAN transfer (fastest)          │
     │   NO  → Try hole-punching (usually works)      │
     │         Failed? → Use relay server (slowest)    │
     └────────────────────────────────────────────────┘

  2. CHUNKING — split file into pieces
     LAN:   16 MB chunks (big = fewer round trips)
     WAN:   4 MB chunks  (medium = balance speed + reliability)
     Relay: 512 KB chunks (small = works over constrained connections)

  3. STREAMING — send chunks in parallel QUIC streams
     The BandwidthSaturator tunes concurrency every 10 chunks:
     "Am I using 90%+ of available bandwidth? If not, add more streams."

  4. RESUMING — survive disconnections
     Each transfer has a RoaringBitmap tracking which chunks are sent.
     If the connection drops at 50%, only the remaining 50% is re-sent.

  5. VERIFICATION — confirm integrity
     After transfer, receiver hashes the complete file (BLAKE3)
     and compares with the sender's hash. Mismatch → re-transfer.

  For FOLDERS:
     1. Build manifest (list all files + relative paths)
     2. Create directory structure on receiver
     3. Stream files preserving paths
     4. Verify each file's hash
```

---

## 16. Sync

### What Is Sync?

```
Sync ensures that when you change something on Device A,
Device B eventually has the same change. And vice versa.

The hard part: what if BOTH devices change the SAME file?
This is called a CONFLICT.
```

### How HyprDrive Syncs (CRDTs + Vector Clocks)

```
CRDT = Conflict-free Replicated Data Type

Every change in HyprDrive is recorded as a SyncOperation:

  SyncOperation {
    id:        ULID (time-sortable unique ID)
    device:    "device_A"
    action:    "rename_file"
    target:    "object_id:a7f3b2c9..."
    data:      { old_name: "photo.jpg", new_name: "vacation.jpg" }
    clock:     VectorClock { device_A: 42, device_B: 37 }
  }

Vector Clocks track "who has seen what":

  Device A's clock: { A: 42, B: 37 }  ← "I've done 42 ops, last I saw B was at 37"
  Device B's clock: { A: 40, B: 39 }  ← "I've done 39 ops, last I saw A was at 40"

  When they sync: compare clocks, exchange only the MISSING operations.
  A sends ops 41-42 to B. B sends ops 38-39 to A.

Conflict resolution:

  IF both devices changed the SAME file:
    Metadata (rename, tag)  → Last-Writer-Wins (most recent timestamp wins)
    File content            → Show conflict panel to user:
                              "Keep Mine | Keep Theirs | Keep Both"

Sync strategies (auto-selected based on how behind a device is):

  < 1,000 missing ops:  → OpLog replay (send each operation, fast for small gaps)
  ≥ 1,000 missing ops:  → MerkleDiff (compare hash trees, efficient for large gaps)

Bandwidth management:

  WiFi:     Sync uses max 25% of bandwidth (don't hog the network)
  Cellular: Sync is PAUSED by default (don't burn data plan)
  User can override both settings.
```

---

## 17. Cloud & Cold Storage

### What Is OpenDAL?

```
OpenDAL = Open Data Access Layer

It's a Rust library that lets you talk to ANY cloud storage
using the SAME code:

  SUPPORTED BACKENDS:
  ┌─────────────────┬──────────────────────────────────┐
  │ Backend         │ Authentication                   │
  ├─────────────────┼──────────────────────────────────┤
  │ Amazon S3       │ API key + secret                 │
  │ Google Drive    │ OAuth2 + refresh token           │
  │ Dropbox         │ OAuth2 + refresh token           │
  │ OneDrive        │ OAuth2 + refresh token           │
  │ Azure Blob      │ API key                          │
  │ Google Cloud    │ API key                          │
  │ Backblaze B2    │ API key                          │
  └─────────────────┴──────────────────────────────────┘

  Same operations for ALL backends:
    list(path)         → list files in a directory
    read(path)         → download a file
    write(path, data)  → upload a file
    stat(path)         → get file metadata
    delete(path)       → delete a file
```

### Storage Tiering

```
HyprDrive automatically manages where your files live based on usage:

  HOT  (local SSD)    — Files you use daily. Instant access.
  WARM (local HDD)    — Files you use monthly. Fast access.
  COLD (cloud)        — Files you rarely use. Slow access, cheap storage.

  TieringPolicy {
    warm_after_days: 90     ← No access for 90 days → move to warm
    cold_after_days: 365    ← No access for 365 days → move to cold
  }

  When you access a cold file:
  1. Daemon downloads it from cloud (transparently)
  2. Caches it locally
  3. Opens it as if it were always there
  4. You never see a "downloading..." spinner (ideally)

  All uploads are encrypted with ChaCha20 BEFORE leaving your device.
  Cloud providers see only encrypted blobs — they can't read your files.
```

---

# Part V — Search & Media

---

## 18. Unified Search

### The Problem with Normal Search

```
Normal file search (Finder / Explorer) only searches FILE NAMES.

  Search: "vacation"
  Results: vacation.jpg, vacation-plans.pdf

  But what about:
  - A PDF that CONTAINS the word "vacation" but is named "2024-plans.pdf"?
  - A photo TAKEN on vacation but named "IMG_4521.jpg"?
  - A video where someone SAYS "vacation" in the audio?
  - A note that LINKS to vacation.jpg?

  Normal search misses ALL of these. HyprDrive finds them all.
```

### Four Search Engines Working Together

```
HyprDrive runs FOUR search engines simultaneously and merges the results:

  1. FULL-TEXT SEARCH (Tantivy)
     What: Searches inside files — filenames, PDF text, transcripts, notes
     How:  Inverted index (like Google builds for the web, but local)
     Speed: < 30ms across 1 million files

  2. SEMANTIC SEARCH (HNSW + CLIP)
     What: "Find photos similar to this one" or "find sunset pictures"
     How:  AI model (CLIP) converts images/text to vectors.
           HNSW index finds nearest vectors (= most similar items).
     Speed: < 5ms per query

  3. TAG SEARCH
     What: Files tagged with "vacation" AND "2024"
     How:  SQL query on tags table with closure for hierarchical tags

  4. TEMPORAL SEARCH
     What: "Photos from January 2024" or "files modified last week"
     How:  temporal_index table populated from EXIF DateTimeOriginal

How results are merged (Reciprocal Rank Fusion):

  Each engine returns its top results RANKED by relevance.
  RRF combines ranks without needing to calibrate scores:

  RRF_score(file) = Σ  1 / (k + rank_in_engine_i)
                    for each engine that found the file

  k = 60 (constant that prevents any single engine from dominating)

  This means: a file ranked #1 in two engines beats a file ranked #1
  in one engine but not found by others. Results feel intuitive.
```

### Command Palette & Query Language

```
Press Cmd/Ctrl+K to open the command palette:

  ┌──────────────────────────────────────────────┐
  │ 🔍  vacation photos 2024                     │
  │──────────────────────────────────────────────│
  │ 📸 IMG_4521.jpg     (tag: vacation, 2024)   │
  │ 📸 beach.heic       (EXIF: Jan 2024, Maui)  │
  │ 📄 travel-plans.pdf (contains "vacation")    │
  │ 🎬 vlog.mp4         (transcript: "vacation") │
  └──────────────────────────────────────────────┘

Power users can use the query language:

  type:image size:>5MB date:2024-01 tag:vacation
  extension:pdf modified:last-week
  kind:video duration:>10min
  duplicate:true
  stale:>2years
```

### Knowledge Graph (Obsidian-Style)

```
HyprDrive parses [[wikilinks]] in markdown files and builds a
backlink graph — just like Obsidian:

  notes/project-ideas.md contains: "See [[vacation-plans]]"
  notes/vacation-plans.md contains: "Budget in [[finances-2024]]"

  This creates a GRAPH:
    project-ideas → vacation-plans → finances-2024

  The graph is visualized in 3D using Three.js/R3F with UMAP clustering
  (files that are linked together appear close together in 3D space).
```

---

## 19. Media Pipeline

### Overview

```
When HyprDrive indexes a file, it doesn't just record the name and size.
For media files, it extracts RICH information:

  Photos:  thumbnail, EXIF (camera, GPS, date), blurhash, face detection
  Videos:  thumbnail, duration, codec, audio track, transcript (Whisper)
  Audio:   waveform, tags (artist, album), duration, transcript
  PDFs:    text extraction, page count, thumbnail of first page
  HEIF:    decoded to standard format (Apple's photo format)
```

### Process Isolation

```
Media processing runs in a SEPARATE PROCESS — not inside the daemon.

Why? Because:
  1. FFmpeg can crash on corrupt files → don't crash the daemon
  2. Image decoders have had security bugs → sandboxed process limits damage
  3. CPU-heavy work doesn't block the daemon's event loop

  ┌────────────────┐    msgpack IPC    ┌──────────────────┐
  │  hyprdrive-daemon   │ ◄──────────────► │  media-worker    │
  │  (main process)│                  │  (subprocess)    │
  └────────────────┘                  └──────────────────┘

  If the media worker crashes:
  1. Daemon detects the crash
  2. Restarts the worker
  3. Retries the failed job
  4. After 3 failures, marks file as "unprocessable" and moves on
```

### Thumbnails

```
Every image/video gets TWO thumbnails:

  320px  — for grid view (tiny, loads fast)
  1080px — for preview panel (detailed, loads on hover)

  Format: WebP (smaller than JPEG at same quality)
  Storage: keyed by ObjectId in the THUMB_MANIFEST cache

  Before the thumbnail loads, the UI shows a BLURHASH:
  a 4×3 grid of blurred colors computed during indexing.
  It's only ~30 bytes and gives an instant "preview of the preview."
```

### Whisper (Speech-to-Text)

```
HyprDrive can transcribe audio and video using OpenAI's Whisper model:

  Models available (user chooses based on disk/RAM):
  ┌──────────────┬──────────┬───────────┬──────────────────────┐
  │ Model        │ Size     │ RAM       │ Quality              │
  ├──────────────┼──────────┼───────────┼──────────────────────┤
  │ tiny.en      │ 75 MB    │ 250 MB    │ Good (English only)  │
  │ small        │ 466 MB   │ 1 GB      │ Better (multilingual)│
  │ large-v3     │ 1.5 GB   │ 4 GB      │ Best (multilingual)  │
  └──────────────┴──────────┴───────────┴──────────────────────┘

  Transcripts are indexed into Tantivy → searchable by spoken words.
  Hardware acceleration: Metal (Mac), CUDA (NVIDIA), CPU fallback.
```

---

# Part VI — Extensions & Integrations

---

## 20. WASM Extension System

### What Is WASM?

```
WASM = WebAssembly

It's a way to run code in a SANDBOX — a secure container that
prevents the code from accessing anything it shouldn't.

Think of it like an apartment building:
  - Each extension lives in its own apartment (sandbox)
  - The building manager (daemon) controls what keys each
    tenant has (capabilities)
  - A tenant can't enter another tenant's apartment
  - A misbehaving tenant can be evicted (terminated)
```

### How Extensions Work in HyprDrive

```
Extension lifecycle:

  1. AUTHOR writes extension in Rust
  2. COMPILE to WASM (wasm-pack build)
  3. AOT COMPILE to native code (wasmtime compile → .cwasm file)
     This pre-compilation means extensions load in < 5ms
  4. SIGN with Ed25519 (author's key)
  5. PUBLISH to extension marketplace

  When user installs an extension:
  1. Verify Ed25519 signature (is this really from the author?)
  2. Check transparency log (has this version been audited?)
  3. Read permission manifest (what does it want access to?)
  4. User approves permissions
  5. Load .cwasm file into wasmtime engine

Runtime sandbox:

  Each extension gets:
  ┌─────────────────────────────────────────────┐
  │ Extension Sandbox                           │
  │                                             │
  │ Memory:    256 MB maximum                   │
  │ CPU:       Epoch-based timeout              │
  │            (if extension runs too long,      │
  │             it's automatically killed)       │
  │ Disk:      10 MB state in redb              │
  │ Network:   NONE (extensions can't phone home)│
  │                                             │
  │ Host functions (the ONLY way to interact):  │
  │   db_query()       ← read from database     │
  │   file_read()      ← read a file's content  │
  │   metadata_write() ← write metadata         │
  │   emit_event()     ← notify the UI          │
  │                                             │
  │ Each call checks capability token first.    │
  │ Extension can only access what was approved.│
  └─────────────────────────────────────────────┘
```

---

## 21. Extension Apps

HyprDrive ships with 7 built-in extension apps:

```
┌──────────────┬──────────────────────────────────────────────────┐
│ Extension    │ What It Does                                     │
├──────────────┼──────────────────────────────────────────────────┤
│ 📸 Photos    │ Face detection, CLIP similarity search,          │
│              │ moment clustering, GPS map view, timeline        │
├──────────────┼──────────────────────────────────────────────────┤
│ 📖 Chronicle │ Document intelligence: NER entity extraction,    │
│              │ relationship graphs, AI summaries, timeline      │
├──────────────┼──────────────────────────────────────────────────┤
│ 👤 Atlas     │ Contact/CRM: extract people from emails/docs,    │
│              │ deal pipeline, relationship tracking              │
├──────────────┼──────────────────────────────────────────────────┤
│ 🎬 Studio   │ Video tools: scene detection, proxy generation,   │
│              │ Whisper transcript overlay, waveform view         │
├──────────────┼──────────────────────────────────────────────────┤
│ 💰 Ledger    │ Finance: OCR receipt scanning, expense categories,│
│              │ CSV/QBO export, tax preparation reports           │
├──────────────┼──────────────────────────────────────────────────┤
│ 🛡️ Guardian │ Backup health: redundancy score per file,         │
│              │ zero-redundancy alerts, backup suggestions        │
├──────────────┼──────────────────────────────────────────────────┤
│ 🔐 Cipher    │ Password vault (Argon2 + ChaCha20), HIBP breach  │
│              │ check, file-level encryption UI                   │
└──────────────┴──────────────────────────────────────────────────┘

Each extension:
  - Is a standalone WASM module (< 10 MB RAM)
  - Has its own UI panel in the desktop/web app
  - Can be enabled/disabled independently
  - Follows the same capability-based security model
```

---

## 22. External Integrations

```
Integrations connect EXTERNAL data sources into HyprDrive:

┌─────────────────┬──────────────────┬───────────────────────────┐
│ Integration     │ Feeds Into       │ How It Works              │
├─────────────────┼──────────────────┼───────────────────────────┤
│ Email Archive   │ Chronicle, Atlas │ Gmail API / Outlook Graph │
│ (Gmail/Outlook) │ Ledger           │ → OAuth → index messages  │
├─────────────────┼──────────────────┼───────────────────────────┤
│ Chrome History  │ Chronicle        │ JSON export → temporal    │
│                 │                  │ index (browsing timeline) │
├─────────────────┼──────────────────┼───────────────────────────┤
│ Spotify Archive │ Chronicle        │ Spotify API → listening   │
│                 │                  │ history + analytics       │
├─────────────────┼──────────────────┼───────────────────────────┤
│ GPS Tracker     │ Photos           │ GPX file import + live    │
│                 │                  │ mobile location sync      │
├─────────────────┼──────────────────┼───────────────────────────┤
│ GitHub Tracker  │ Chronicle        │ GitHub API + webhooks →   │
│                 │                  │ repo activity index       │
├─────────────────┼──────────────────┼───────────────────────────┤
│ Obsidian Vault  │ Knowledge Graph  │ Two-way .md sync,         │
│                 │                  │ wikilink parsing          │
└─────────────────┴──────────────────┴───────────────────────────┘

All credentials stored in redb, encrypted with master key.
```

---

# Part VII — User Interfaces

---

## 23. Interface Architecture

```
HyprDrive has a "one core, many faces" architecture:

  ┌─────────────────────────────────────────────────────────┐
  │                    hyprdrive-daemon (Rust)                    │
  │                   WebSocket :7420                        │
  └───────────┬──────────────┬──────────────┬───────────────┘
              │              │              │
     ┌────────▼───────┐ ┌───▼────────┐ ┌───▼────────────┐
     │ Tauri Desktop  │ │  Web App   │ │ Tauri Lite     │
     │ React + rspc   │ │ React      │ │ egui (native)  │
     │ Full features  │ │ Full feat. │ │ < 14 MB, 40 MB │
     └────────────────┘ └────────────┘ └────────────────┘

  Mobile is different (see Section 25).

Shared packages:
  packages/ui/         → Radix UI + Tailwind CSS component library
  packages/interface/  → Shared React components, Zustand state, Framer Motion
  packages/ts-client/  → Auto-generated TypeScript types from Rust (Specta)
```

---

## 24. Desktop App

```
Built with Tauri 2 (Rust backend + React frontend).

KEY RULE: Tauri is a THIN CLIENT.
  - Zero core code linked into the Tauri binary
  - All data comes from the daemon via WebSocket (rspc)
  - If the daemon isn't running, the app shows "Connecting..."

Components:

  FILE LIST (TanStack Virtual)
    - Renders 1 MILLION rows at 60fps using virtualization
      (only renders the ~50 rows visible on screen)
    - Grid / List / Columns view toggle
    - Multi-column sort, filter, resize (TanStack Table)
    - Breadcrumb navigation + path bar
    - Blurhash → thumbnail crossfade
    - Keyboard: hjkl navigation, Enter to open, Backspace for parent
    - /  to search

  PANELS
    - Contextual sidebar: EXIF, word count, duration, treemap
    - Tag panel: create, hierarchical, bulk assign, autocomplete
    - Virtual folders sidebar (saved FilterExpr queries)
    - Activity feed (real-time file changes)
    - Version history

  DISK INTELLIGENCE VIEW (Ctrl/Cmd+D)
    - Treemap SVG: hover details, click to drill down, animated transitions
    - "Reveal in treemap" from any file
    - Type breakdown donut chart + ranked table
    - Top 100 largest files/directories
    - Wasted space summary
    - Disk Insights: stale, artifacts, duplicates → [Clean] / [Review]

  OPERATIONS
    - Context menu: cut/copy/paste/rename/delete/tag/send/analyze
    - Drag-and-drop (dnd-kit library)
    - Undo/Redo (Cmd+Z / Ctrl+Z) — last 50 operations
    - Bulk select + batch operations
    - Search bar with query language
    - Debug overlay: Cmd/Ctrl+Shift+D

Frontend tech stack:
  React 19, Vite, TypeScript, TanStack (Query, Virtual, Table),
  Zustand (state), Radix UI (accessible components), Tailwind CSS,
  Framer Motion (animations), React Hook Form + Zod (validation),
  dnd-kit (drag-and-drop), Three.js/R3F (3D graph), rspc (typed RPC)
```

---

## 25. Mobile App

```
Built with React Native + Expo.

KEY DIFFERENCE: Mobile embeds hyprdrive-core IN-PROCESS (no daemon).

  Why? Phones can't reliably run background daemons.
  Instead, hyprdrive-core is compiled as a static library:
    - iOS:     aarch64-apple-ios
    - Android: aarch64-linux-android

  Communication: C ABI FFI layer (JSON over C strings)
    iOS:     ObjC JSI bridge
    Android: JNI bridge

  The mobile app connects to desktop daemons as a PEER for syncing.

Features:
  - Photo library virtual volumes (PHPhotoLibrary / MediaStore)
  - Background sync (BGProcessingTask / WorkManager)
  - iOS Share Sheet extension ("Save to HyprDrive")
  - File browser + transfer UI + disk panel (responsive layout)

Mobile tech stack:
  React Native, Expo Router, NativeWind (Tailwind for RN),
  Reanimated (animations), React Navigation
```

---

## 26. Web App & Docker

```
WEB APP (apps/web/):
  - Vite + React, same shared packages as desktop
  - Connects to daemon via WebSocket
  - For accessing your files from any browser on the network

DOCKER (apps/server/):
  - Dockerfile runs hyprdrive-daemon
  - Caddy reverse proxy for HTTPS
  - Volume mounts for library persistence
  - docker-compose.yml for one-command deploy

  Perfect for: running HyprDrive on a home server / NAS
```

---

# Part VIII — Reference

---

## 27. Performance Targets

These targets are enforced by CI — if a commit makes any metric worse,
the build fails.

```
┌──────────────────────────────┬────────────┬──────────────────┐
│ What                         │ Target     │ First Verified   │
├──────────────────────────────┼────────────┼──────────────────┤
│ list_files_fast(100k)        │ < 5ms      │ Phase 2          │
│ MFT scan 100k (Windows)     │ < 1.5s     │ Phase 3          │
│ USN change → FsChange event │ < 200ms    │ Phase 3.5        │
│ Partial hash (4KB)          │ < 10µs     │ Phase 3.6        │
│ Full hash 1 MB              │ < 5ms      │ Phase 3.6        │
│ Size bucket 100k files      │ < 50ms     │ Phase 3.6        │
│ Fuzzy match 1k names        │ < 100ms    │ Phase 3.6        │
│ getattrlistbulk 100k (Mac)  │ < 4s       │ Phase 4          │
│ io_uring scan 100k (Linux)  │ < 2s       │ Phase 5          │
│ BLAKE3 hash 1 GB            │ < 1s       │ Phase 7          │
│ Treemap build 100k          │ < 1.5s     │ Phase 8          │
│ Aggregation 100k            │ < 200ms    │ Phase 8          │
│ Filename search 100k        │ < 30ms     │ Phase 11         │
│ Encrypt 1 GB stream         │ < 2s       │ Phase 12         │
│ LAN transfer 1 GbE          │ > 900 Mbps │ Phase 14         │
│ Sync 10k delta              │ < 5s       │ Phase 15         │
│ Cloud list 10k files        │ < 3s       │ Phase 15.5       │
│ WASM extension load         │ < 5ms      │ Phase 18         │
│ FTS query at 1M files       │ < 30ms     │ Phase 19         │
│ HNSW ANN query              │ < 5ms      │ Phase 19         │
│ File change → UI update     │ < 50ms     │ Phase 10         │
│ Virtual list 1M rows        │ 60fps      │ Phase 11         │
│ Tauri-lite RAM              │ < 40 MB    │ Phase 21         │
│ Tauri-lite binary           │ < 14 MB    │ Phase 21         │
└──────────────────────────────┴────────────┴──────────────────┘
```

---

## 28. Hardware & OS Requirements

```
MINIMUM HARDWARE:
  CPU:          x64 or ARM64, 2015 or newer
  RAM:          512 MB free
  Disk (app):   50 MB
  Disk (DB):    ~1 MB per 10,000 files
  Disk (thumbs): ~1 MB per 100 photos
  Network:      None required (offline-first)

RECOMMENDED:
  CPU:          4+ cores (indexing uses parallelism)
  RAM:          4 GB free
  Disk (app):   100 MB
  Network:      WiFi for P2P sync and transfer

OPERATING SYSTEMS:
  Windows:  10+ (NTFS MFT requires 1903+)
  macOS:    12+ Monterey (getattrlistbulk requires APFS)
  Linux:    5.15+ kernel (io_uring requires 5.6+, fanotify requires 5.1+)
  iOS:      16+
  Android:  12+ (API 31+, Scoped Storage)
```

---

## 29. Glossary

For anyone new to these concepts:

```
BLAKE3          Fast cryptographic hash function (like SHA-256 but 10× faster)
BIP39           Standard for turning encryption keys into 24-word phrases
Blurhash        Tiny (~30 byte) color placeholder shown before thumbnails load
Capability Token  Signed permission slip authorizing a specific action
ChaCha20-Poly1305  Encryption algorithm (encrypts + verifies integrity)
CLI             Command Line Interface (text-based app in terminal)
CLIP            AI model that understands both images and text
CRDT            Data structure that syncs without conflicts
Daemon          Background process with no visible window
Dedup Engine    Multi-strategy duplicate detection (content hash, fuzzy, perceptual)
Ed25519         Digital signature algorithm (proves identity)
EventBus        Internal message system — components notify each other
FTS5            SQLite's Full-Text Search engine
getattrlistbulk macOS syscall that reads 1024 file attributes at once
HKDF            Key derivation function (creates sub-keys from master key)
HNSW            Hierarchical graph index for finding similar vectors fast
image_hasher    Perceptual hashing library for detecting visually similar images
io_uring        Linux async I/O interface for high-throughput disk reads
Iroh            P2P networking library by n0.computer
Jaro-Winkler    String similarity metric (0.0–1.0) for fuzzy filename matching
MFT             Master File Table — NTFS's index of all files on a drive
mDNS            Protocol for discovering devices on a local network
OpenDAL         Rust library for unified cloud storage access
P2P             Peer-to-peer — devices connect directly, no server
QUIC            Modern transport protocol (like TCP, but faster + encrypted)
RRF             Reciprocal Rank Fusion — merges search results from multiple engines
rspc            Type-safe RPC library for Rust ↔ TypeScript communication
redb            Embedded key-value store (like a fast Dictionary/HashMap on disk)
Specta           Auto-generates TypeScript/Swift types from Rust structs
Squarified Treemap  Layout algorithm for disk usage visualization
SQLite          Embedded database engine (runs inside your app, no server)
TanStack        React libraries for tables, virtual scrolling, and data fetching
Tantivy         Rust full-text search engine (like Elasticsearch, but embedded)
Tauri           Framework for building desktop apps with Rust + web frontend
TDD             Test-Driven Development — write tests before code
ULID            Unique Lexicographic ID — like UUID but sortable by time
Union-Find      Data structure for grouping transitive matches (if A=B, B=C → {A,B,C})
USN             Update Sequence Number — Windows change journal entry
UsnListener     Background monitor that polls USN journal for real-time file changes
Vector Clock    Data structure tracking "who has seen what" across devices
WAL             Write-Ahead Logging — SQLite mode for concurrent access
WASM            WebAssembly — portable bytecode format for sandboxed execution
wasmtime        WASM runtime engine (runs extension plugins)
X25519          Key exchange algorithm (two devices agree on a shared secret)
```

---

## Technology Stack Summary

```
CORE (Rust):
  Runtime:     Tokio (async)
  Database:    SQLite + SeaORM
  Caching:     redb
  Hashing:     BLAKE3
  Dedup:       BLAKE3 progressive + Jaro-Winkler + blockhash (image_hasher)
  Crypto:      ChaCha20-Poly1305, Ed25519, X25519, Argon2, BIP39
  P2P:         Iroh (QUIC + hole-punching + mDNS)
  Transfer:    Custom Blip engine (QUIC streams)
  Cloud:       OpenDAL (S3, GDrive, Dropbox, OneDrive, Azure, GCS, B2)
  Extensions:  wasmtime AOT
  Search:      Tantivy (FTS) + HNSW (semantic)
  HTTP:        Axum
  Type Export: Specta (→ TypeScript, Swift)

FRONTEND (TypeScript):
  Framework:   React 19
  Bundler:     Vite
  State:       Zustand
  Data:        TanStack Query
  Virtualize:  TanStack Virtual
  Tables:      TanStack Table
  Components:  Radix UI
  Styling:     Tailwind CSS
  Animation:   Framer Motion
  Forms:       React Hook Form + Zod
  DnD:         dnd-kit
  3D:          Three.js / React Three Fiber
  RPC:         rspc

DESKTOP:       Tauri 2
MOBILE:        React Native + Expo
LITE CLIENT:   egui + wgpu
BUILD:         Cargo + Turborepo

MEDIA:
  Video/Audio: FFmpeg
  Images:      libheif (HEIF/HEIC)
  PDFs:        Pdfium
  Speech:      Whisper (via candle)
  AI Models:   CLIP (via candle), MobileNet (faces)
```

---

> **End of Architecture Specification v4.0**
>
> For the detailed implementation plan with step-by-step instructions,
> see the [Implementation Plan](implementation_plan.md).
