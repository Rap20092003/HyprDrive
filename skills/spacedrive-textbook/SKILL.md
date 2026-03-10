---
name: spacedrive-textbook
description: Comprehensive reference guide extracted from Spacedrive's ~183k-line codebase. Use as a textbook when building HyprDrive — patterns, structures, and lessons learned.
---

# Spacedrive Textbook

> **Source**: [github.com/spacedriveapp/spacedrive](https://github.com/spacedriveapp/spacedrive)
> **Purpose**: Reference guide for building HyprDrive from scratch using patterns
> proven in Spacedrive's ~183k-line Rust codebase.

> [!IMPORTANT]
> This skill is a **textbook**, not a copy-paste source. Study the patterns,
> understand the WHY, then implement your own version adapted for HyprDrive's
> decisions (wasmtime, ChaCha20-only, Vector Clocks, MFT indexing, daemon-first).

---

## Chapter 1: Application Bootstrap Sequence

### How Spacedrive Starts Up

The `Core::new_with_config()` function in `lib.rs` follows a strict
initialization order. Each component depends on the ones before it.

```
BOOTSTRAP ORDER (must be sequential — each step depends on the previous):

  1. Load AppConfig (or create defaults)
  2. Create KeyManager (crypto keys)
  3. Create DeviceManager (device identity, needs KeyManager)
  4. Set global device ID + slug (convenience accessors)
  5. Create EventBus (pub/sub for all components)
  6. Create LogBus (separate from events — avoids log spam in event stream)
  7. Create VolumeManager (detect drives, needs device_id + events)
  8. Initialize volume detection (async scan of mounted drives)
  9. Create CoreContext (god struct holding Arc refs to everything above)
  10. Create LibraryManager (user's libraries, needs events + volumes + device)
  11. Inject LibraryManager into CoreContext (circular dep resolution)
  12. Create Services container (networking, sync, watcher, sidecar)
  13. Inject SidecarManager into CoreContext
  14. Inject FsWatcher into CoreContext
  15. Scan for existing .sdlibrary directories
  16. Load all found libraries
  17. Initialize WASM plugin manager (if feature enabled)
  18. Create ActionManager (CQRS command dispatcher)
  19. Create ApiDispatcher (HTTP/WebSocket API layer)
  20. Start networking service (Iroh P2P)
  21. Return Core struct

HyprDrive adaptation:
  - Replace steps 17 with wasmtime engine initialization
  - Add MFT/getattrlistbulk/io_uring helper process launch at step 8.5
  - Add Axum HTTP server start at step 19.5
  - Add Prometheus metrics server at step 19.6
```

### The Core Struct

```
Core {
    config:          Arc<RwLock<AppConfig>>       // Hot-reloadable config
    device:          Arc<DeviceManager>           // This device's identity
    libraries:       Arc<LibraryManager>          // User's libraries
    volumes:         Arc<VolumeManager>           // Mounted drives
    events:          Arc<EventBus>                // Pub/sub for all events
    logs:            Arc<LogBus>                  // Separate log stream
    services:        Services                     // Network, sync, watcher
    plugin_manager:  Option<Arc<RwLock<PM>>>      // WASM extensions
    context:         Arc<CoreContext>             // Shared context for ops
    api_dispatcher:  ApiDispatcher                // HTTP/WS API layer
}
```

### Lesson: Circular Dependency Resolution

Spacedrive uses `Arc<RwLock<Option<T>>>` for components that depend on
each other. The pattern:

```
1. Create CoreContext with library_manager = None
2. Create LibraryManager (needs CoreContext)
3. Inject LibraryManager back: context.set_libraries(libraries)

This is used for: LibraryManager, ActionManager, NetworkingService,
SidecarManager, FsWatcher, PluginManager.

HyprDrive lesson:
  Accept this pattern. It's ugly but necessary for cyclic deps in
  async Rust. The alternative (channels) adds complexity without clarity.
```

---

## Chapter 2: Domain Modeling

### The Object/Location Split

Spacedrive's MOST IMPORTANT architectural insight:

```
WRONG approach (traditional file systems):
  One table: files(path, size, hash, tags...)
  Problem: same file at 2 paths = 2 rows = no dedup awareness

RIGHT approach (Spacedrive / HyprDrive):
  Two tables:
    ContentIdentity(uuid, content_hash, mime, size)  ← WHAT the content IS
    Entry(id, sd_path, content_id, name, timestamps) ← WHERE the content LIVES

  Same content at 2 paths = 1 ContentIdentity + 2 Entries
  → Instant duplicate detection
  → "Find all copies of this file" = one query

HyprDrive mapping:
  ContentIdentity → Object (in our schema)
  Entry → Location (in our schema)
  Same concept, different names.
```

### SdPath — Universal File Addressing

Spacedrive's SdPath is a 4-variant enum that can address ANY file anywhere:

```
SdPath::Physical { device_slug, path }
  → "jamies-macbook:/Users/jamie/photo.jpg"
  → Points to a specific file on a specific device

SdPath::Cloud { service, identifier, path }
  → "s3:my-bucket:/photos/photo.jpg"
  → Points to a cloud-stored file

SdPath::Content { content_id }
  → "content:a7f3b2c9-..."
  → Points to content by identity (any device that has it)

SdPath::Sidecar { content_id, kind, variant, format }
  → "sidecar:a7f3b2c9/thumb/grid@2x/webp"
  → Points to derived data (thumbnail, transcript, embedding)

HyprDrive adaptation:
  Adopt this pattern exactly. It enables:
  - "Copy this file" without knowing which device has it
  - Transparent cloud fetch
  - Sidecar management (thumbnails, transcripts)
```

### File — The Aggregated Domain Model

```
File is a COMPUTED struct, not a database table. It aggregates:
  - Entry data (path, name, size, timestamps)
  - ContentIdentity (hash, mime, kind)
  - Tags (with relationships and composition rules)
  - Sidecars (thumbnails, OCR text, embeddings)
  - MediaData (EXIF for images, codec for video, tags for audio)
  - Alternate paths (other locations with same content)

Key insight: File is assembled from pre-fetched data, NOT by
querying the DB on-demand for each field. This is critical for
performance when listing 100k files.

HyprDrive adaptation:
  Same pattern. Pre-fetch all related data in the list query using
  JOINs, then assemble the domain File struct in Rust.
```

### Tag System Architecture

```
Spacedrive has a rich tag system:

Tag {
    canonical_name  — machine-readable ("quarterly-report")
    display_name    — human-readable ("Quarterly Report")
    formal_name     — full name ("Q4 2024 Quarterly Financial Report")
    abbreviation    — short form ("Q4-QR")
    aliases         — alternatives (["quarterly", "report-q4"])
    namespace       — grouping ("finance")
    tag_type        — Standard | Organizational | Privacy | System
    privacy_level   — Normal | Archive | Hidden
    search_weight   — affects ranking in search results
    attributes      — arbitrary key-value metadata
    composition_rules — rules for combining tags
}

TagRelationship {
    related_tag_id
    relationship_type — ParentChild | Synonym | Related
    strength          — 0.0 to 1.0 (how strongly related)
}

This enables:
  - "vacation" tag auto-includes "travel" (ParentChild)
  - Searching "quarterly" also finds "Q4-QR" (Synonym)
  - Privacy tags hide files from normal search (Hidden level)

HyprDrive adaptation:
  Adopt this model. Add: hierarchical closure table for
  ancestor/descendant queries (Spacedrive doesn't have this).
```

---

## Chapter 3: CQRS Pattern

### Spacedrive's CQRS Implementation

```
SEPARATION:
  Queries  — read data, never mutate state
  Actions  — mutate state, create audit trail

THREE SCOPE LEVELS:

  Query         — simple, no session context needed
  CoreQuery     — daemon-level, needs SessionContext
  LibraryQuery  — library-scoped, needs SessionContext + library_id

  The same pattern exists for Actions:
  CoreAction    — daemon-level mutations
  LibraryAction — library-scoped mutations

DISPATCH FLOW:

  Client request
    → ApiDispatcher (deserialize, route)
      → SessionContext (auth, permissions, audit)
        → QueryManager.dispatch() or ActionManager.dispatch()
          → Operation.execute(context, session)
            → Return result
```

### The Action Pipeline

```
Every mutation follows preview-commit-verify:

  ActionBuilder::new(MoveFilesAction { ... })
    .preview()    // → "Will move 50 files, free 0 bytes"
    .commit()     // → Execute the operation
    .verify()     // → "All 50 files verified at destination"

This is implemented via the Action trait:
  trait CoreAction {
      type Input;
      type Output;

      fn from_input(input: Self::Input) -> Result<Self>;
      fn execute(self, context, session) -> Result<Self::Output>;
  }

Operations are REGISTERED at compile time using the `inventory` crate:
  inventory::submit! { ActionRegistration::new::<MoveFilesAction>() }

This means all available operations are known at compile time —
no runtime reflection, no string-based dispatch.

HyprDrive adaptation:
  Use the same pattern. Add:
  - UndoStack (Spacedrive doesn't have explicit undo)
  - Operation recording for sync (Spacedrive uses HLC logs)
```

### SessionContext

```
Every operation receives a SessionContext containing:
  - Device identity (who is making this request)
  - Authentication state
  - Permissions
  - Audit trail metadata
  - Library context (if library-scoped)

This enables:
  - "Who moved this file?" → audit log
  - "Can this device delete files?" → permission check
  - "Which library should this affect?" → scoping

HyprDrive adaptation:
  Add CapabilityToken validation to SessionContext.
  Spacedrive uses simpler device-based auth.
```

---

## Chapter 4: Infrastructure Layer

### Database (SeaORM + SQLite)

```
Spacedrive's database structure:

  core/src/infra/db/
    ├── entities/          ← SeaORM entity definitions (auto-generated)
    ├── migration/         ← Schema migrations
    └── mod.rs             ← Connection setup, WAL mode, pragmas

Key pragmas (same as HyprDrive plan):
  journal_mode = WAL
  synchronous = NORMAL
  foreign_keys = ON
  busy_timeout = 5000

HyprDrive adaptation:
  Same setup. Add:
  - mmap_size = 256MB (Spacedrive doesn't set this)
  - Custom indexes for list_files_fast()
  - dir_sizes table (Spacedrive doesn't have aggregation)
```

### EventBus

```
core/src/infra/event/

EventBus is a broadcast channel that ALL components listen to:

  events.emit(Event::FileCreated { path, object_id })
  events.emit(Event::SyncCompleted { device_id })
  events.emit(Event::JobProgress { job_id, percent })

Subscribers:
  - UI (via WebSocket → React)
  - Sync engine (records operations for replication)
  - Search indexer (updates FTS on file changes)
  - Statistics listener (updates counts/sizes)

Spacedrive also has a SEPARATE LogBus for detailed logging:
  logs.emit(LogEntry { level, message, context })

This prevents log messages from flooding the event stream.

HyprDrive adaptation:
  Same pattern. Separate EventBus and LogBus.
```

### Job System (task-system crate)

```
crates/task-system/

Spacedrive's job system handles long-running operations:

Key concepts:
  - Jobs are DURABLE — they survive daemon restarts
  - Jobs use MessagePack serialization for checkpoints
  - Jobs have progress reporting (percent, items processed)
  - Jobs can be paused/resumed/cancelled
  - Per-job file logging (each job gets its own log file)

Job lifecycle:
  Created → Queued → Running → (Paused) → Completed/Failed

The job-derive crate provides procedural macros:
  #[derive(Job)]
  struct IndexLocationJob { location_id: Uuid }

HyprDrive adaptation:
  Study this crate carefully. Replicate:
  - Durable checkpointing (for resume after crash)
  - Progress reporting (for UI progress bars)
  - Per-job logging (for debugging)
  Add:
  - Priority scheduling (index Desktop before node_modules)
```

### API Layer

```
core/src/infra/api/

ApiDispatcher routes client requests to operations:

  Client → WebSocket → ApiDispatcher → SessionContext → Operation → Result

Spacedrive uses rspc for type-safe RPC:
  - Rust operations → Specta generates TypeScript types
  - React frontend uses auto-generated hooks
  - Zero manual serialization code

HyprDrive adaptation:
  Same rspc + Specta pattern. Add:
  - Axum HTTP routes on :7421 (Spacedrive doesn't expose HTTP API)
  - Prometheus metrics on :7422
```

---

## Chapter 5: Services Layer

### Networking (Iroh P2P)

```
core/src/service/network/

Components:
  NetworkingService      — main service, manages connections
  PairingProtocolHandler — QR code + key exchange for device pairing
  NetworkLogger          — structured logging for network events

Protocols:
  protocol/pairing/      — Ed25519 key exchange, device verification
  protocol/              — custom Iroh protocols for SD messages

Connection management:
  - Auto-discover devices on LAN (mDNS)
  - Maintain persistent connections to paired devices
  - Reconnect on network changes
  - Fallback to relay servers

HyprDrive adaptation:
  Same Iroh setup. Add:
  - CapabilityToken exchange during pairing
  - RoutingOracle for transfer path selection
  - BandwidthSaturator for transfer speed optimization
```

### File Sync

```
core/src/service/sync/     — sync engine
core/src/service/file_sync/ — file content sync

Spacedrive uses HLC (Hybrid Logical Clocks):
  - Each operation gets an HLC timestamp
  - Operations are ordered by HLC for conflict resolution
  - Leaderless: no primary device, all peers are equal

Sync domains (separate sync rules for different data types):
  - Device-local data (filesystem index) → state replication
  - Shared metadata (tags, ratings) → HLC-ordered operation log

HyprDrive adaptation:
  Replace HLC with Vector Clocks for better causality tracking.
  Keep the domain separation concept (local vs shared data).
  Add: CRDT-based conflict resolution for metadata.
```

### File Watcher

```
core/src/service/watcher/     — current implementation
core/src/service/watcher_old/ — previous version (kept for reference)

The watcher service:
  1. Registers filesystem watchers on indexed locations
  2. Debounces rapid changes (batch events)
  3. Triggers re-indexing for changed paths
  4. Emits events for UI invalidation

Spacedrive has TWO versions — they iterated on this.
The old version likely used notify crate directly.
The new version may use platform-specific APIs.

HyprDrive adaptation:
  Our fs-indexer crate goes beyond what Spacedrive has:
  - MFT reader (Windows) — they don't have this
  - getattrlistbulk (macOS) — they don't have this
  - io_uring (Linux) — they don't have this
  Study their debouncing and event pipeline, but write
  the actual platform indexers from scratch.
```

### Sidecar Management

```
core/src/service/sidecar_sync/
core/src/service/sidecar_manager.rs

"Sidecars" are derived files:
  - Thumbnails (320px grid + 1080px preview)
  - OCR text (extracted from PDFs/images)
  - Embeddings (CLIP vectors for similarity search)
  - Media metadata (EXIF, FFmpeg probes)

They are stored alongside their source content,
addressed by: content_id + kind + variant + format

Example: content:abc123 → sidecar:abc123/thumb/grid@2x/webp

This is a clean separation:
  - Source content is immutable after indexing
  - Sidecars are re-generatable (delete and recreate anytime)
  - Different quality levels (grid vs preview thumbnails)

HyprDrive adaptation:
  Adopt this sidecar pattern. It maps perfectly to our
  media pipeline (Phase 17) and search pipeline (Phase 19).
```

---

## Chapter 6: Volume & Location Management

```
core/src/volume/         — volume (drive) detection + fingerprinting
core/src/location/       — indexed locations within volumes
core/src/service/volume_monitor.rs — live volume mount/unmount detection

Volume = a physical or virtual drive (C:\, /dev/sda1, S3 bucket)
Location = a specific directory being indexed within a volume

Volume fingerprinting:
  Each volume is identified by a unique fingerprint (not just mount path).
  This means if you plug in the same USB drive to a different port,
  it's still recognized as the SAME volume.

HyprDrive adaptation:
  Same concept. Volume = drive, Location = indexed path.
  Add: CloudVolume variant for OpenDAL backends.
```

---

## Chapter 7: WASM Extensions

```
crates/sdk/             — extension SDK
crates/sdk-macros/      — procedural macros for extensions
core/src/infra/extension/ — host-side extension management

Spacedrive uses Wasmer (HyprDrive will use wasmtime).

Current state: "under active development" — SDK exists but
is not production-ready. Only test-extension exists.

Host functions exposed to extensions:
  db_query()        — read from database (read-only)
  file_read()       — read file content
  metadata_write()  — write metadata back
  emit_event()      — send events to UI

Extension lifecycle:
  Install → Verify signature → Load WASM → Execute → Sandbox

HyprDrive adaptation:
  - Use wasmtime instead of Wasmer (AOT compilation)
  - Add epoch-based interruption (kill runaway extensions)
  - Add capability tokens per extension (fine-grained permissions)
  - Build 7 real extensions (Spacedrive has 0 production extensions)
```

---

## Chapter 8: Frontend Integration

### rspc + Specta Type Generation

```
The TypeScript frontend NEVER writes type definitions manually.

Flow:
  1. Rust structs derive `specta::Type`
  2. Specta exports TypeScript interfaces at build time
  3. rspc generates typed React hooks
  4. Frontend imports: import { useQuery } from '@sd/ts-client'

Example:
  // Rust
  #[derive(Type)]
  pub struct File { pub name: String, pub size: u64 }

  // Auto-generated TypeScript
  interface File { name: string; size: number }

  // Auto-generated React hook
  const { data } = useQuery(['files.list', { path: '/photos' }])
  // data is fully typed as File[]

HyprDrive adaptation:
  Same pattern. Add Swift type generation for iOS Share Sheet.
```

### Tauri Integration

```
apps/tauri/

Tauri is a THIN SHELL:
  - No core logic in the Tauri binary
  - All data comes from daemon via WebSocket
  - Tauri's Rust backend just manages the window + system tray

Startup:
  1. Tauri starts
  2. Checks if daemon is running (try connect to :7420)
  3. If not running, start daemon as subprocess
  4. Connect via WebSocket
  5. Render React frontend

HyprDrive adaptation:
  Same pattern. The Tauri app is literally just a browser
  window connected to the daemon. Zero business logic.
```

### Mobile Integration

```
apps/mobile/ + sd-mobile-core

React Native app embeds Rust core via FFI:
  - iOS: aarch64-apple-ios → static lib → ObjC JSI bridge
  - Android: aarch64-linux-android → JNI bridge

Communication: JSON over C strings through the FFI boundary
  Rust function → serialize to JSON → C string → JS bridge → React

The mobile app is the ONE exception to daemon-first:
  - Phones can't run reliable background daemons
  - So mobile embeds vdfs-core directly (in-process)
  - Connects to desktop daemons as a PEER for sync

HyprDrive adaptation:
  Same pattern. Study sd-mobile-core's C ABI layer carefully.
```

---

## Chapter 9: Lessons Learned (Anti-Patterns to Avoid)

### 1. The God Struct Problem

```
CoreContext has 14 fields, many wrapped in Arc<RwLock<Option<T>>>.
This is functional but hard to reason about.

HyprDrive mitigation:
  Consider breaking CoreContext into focused sub-contexts:
  - StorageContext (db, cache, volumes)
  - NetworkContext (iroh, sync, transfer)
  - MediaContext (ffmpeg, thumbnails, sidecars)
  - ExtensionContext (wasmtime, permissions)
  Each passed only to components that need it.
```

### 2. Deferred Prototypes

```
Spacedrive has apps/ios/, apps/macos/, apps/gpui-photo-grid/ —
all prototypes that add maintenance burden without shipping value.

HyprDrive decision: DEFERRED. Don't start prototypes until
the core product ships. Focus is hard.
```

### 3. Dual Cipher Complexity

```
Spacedrive supports BOTH ChaCha20-Poly1305 AND AES-GCM.
This means double the test surface, algorithm negotiation logic,
and potential for misconfiguration.

HyprDrive decision: ChaCha20-Poly1305 ONLY.
One cipher = one code path = fewer bugs.
```

### 4. Basic Indexing

```
Spacedrive uses fs-watcher (likely notify crate + walkdir).
This works but is 10-30x slower than platform-native APIs.

HyprDrive advantage: MFT (Windows), getattrlistbulk (macOS),
io_uring (Linux). This is our biggest performance differentiator.
```

### 5. Missing Intelligence Features

```
Spacedrive indexes files but doesn't ANALYZE them:
  - No disk usage treemap (WizTree)
  - No stale file detection
  - No build artifact detection
  - No storage tiering
  - No query language for power users
  - No knowledge graph

These are HyprDrive's value-adds.
```

---

## Chapter 10: File-by-File Reference Map

When implementing a HyprDrive phase, consult these Spacedrive files:

```
PHASE 0 (Project Setup):
  Study: Cargo.toml workspace, turbo.json, package.json

PHASE 1 (Domain Types):
  Study: core/src/domain/mod.rs
         core/src/domain/content_identity.rs  → ContentIdentity
         core/src/domain/addressing.rs        → SdPath enum
         core/src/domain/file.rs              → File aggregate
         core/src/domain/tag.rs               → Tag system
         core/src/domain/volume.rs            → Volume types
         core/src/domain/location.rs          → Location types
         core/src/domain/device.rs            → Device identity

PHASE 2 (Database):
  Study: core/src/infra/db/

PHASE 6 (CQRS):
  Study: core/src/cqrs.rs                    → Query/Action traits
         core/src/ops/                        → All operations
         core/src/infra/action/               → ActionManager
         core/src/infra/query/                → QueryManager

PHASE 7 (Hashing):
  Study: core/src/domain/content_identity.rs  → BLAKE3 hashing

PHASE 9 (Events + Jobs):
  Study: core/src/infra/event/                → EventBus
         core/src/infra/job/                  → Job infrastructure
         crates/task-system/                  → Durable jobs

PHASE 10 (File Watching):
  Study: core/src/service/watcher/            → Current watcher
         crates/fs-watcher/                   → Watcher crate

PHASE 11 (Desktop UI):
  Study: apps/tauri/                          → Tauri shell
         packages/interface/                  → Shared React components
         packages/ui/                         → Component library
         packages/ts-client/                  → rspc + Specta bindings

PHASE 12 (Crypto):
  Study: core/src/crypto/                     → Key management
         crates/crypto/                       → Crypto primitives

PHASE 13 (Daemon):
  Study: core/src/lib.rs                      → Core bootstrap
         core/src/context.rs                  → CoreContext
         core/src/infra/daemon/               → Daemon infrastructure
         apps/cli/                            → CLI + daemon entry

PHASE 14-15 (P2P + Sync):
  Study: core/src/service/network/            → Iroh networking
         core/src/service/sync/               → Sync engine
         core/src/infra/sync/                 → Sync infrastructure

PHASE 16 (Mobile):
  Study: apps/mobile/                         → React Native app

PHASE 17 (Media):
  Study: crates/ffmpeg/                       → FFmpeg bindings
         crates/images/                       → Image processing
         crates/media-metadata/               → EXIF/metadata
         core/src/service/sidecar_manager.rs  → Sidecar management

PHASE 18 (Extensions):
  Study: crates/sdk/                          → Extension SDK
         crates/sdk-macros/                   → Proc macros
         core/src/infra/extension/            → Extension host

NOT IN SPACEDRIVE (build from scratch):
  Phase 3-5:   MFT reader, getattrlistbulk, io_uring
  Phase 8:     WizTree engine (treemap, aggregation)
  Phase 15.5:  Cloud/OpenDAL tiering
  Phase 19:    4-engine RRF search
  Phase 20:    7 extension apps
  Phase 20.5:  External integrations
  Phase 21:    egui lightweight client
```
