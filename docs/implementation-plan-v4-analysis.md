# HyprDrive — Implementation Plan v3.0: Comprehensive Analysis & Validation Report

**Date**: 2026-03-15
**Analyst frameworks applied**: gstack-eng-mode (architecture diagrams, failure modes, test matrices) · GSD (atomic commits, spec-driven, Iron Laws) · Ralph (user stories, acceptance criteria, atomic iteration) · gstack-qa (systematic validation) · gstack-ship (CI/CD pipeline awareness) · Spacedrive Textbook (Ch1-10 cross-reference)

---

# PART I: FRAMEWORKS APPLIED

## 1. gstack-eng-mode — Engineering Mode Review
Applied to every phase: architecture diagrams, system boundaries, data models, state machines, failure modes, edge cases, test matrices, and performance considerations. Each phase below includes these artifacts.

## 2. GSD — Get Shit Done (Spec-Driven)
Applied via the 4 Iron Laws governing every phase:
1. **TDD**: No production code without a failing test first
2. **Verification**: No phase exit without verified exit criteria
3. **Atomic commits**: Every task gets its own commit
4. **CI benchmarks**: Benchmark harness runs from Phase 0 onward

## 3. Ralph — PRD Autonomous Loop
Applied to decompose each phase into atomic user stories (US-xxx) with verifiable acceptance criteria. Each story is sized for one iteration (one context window). Dependencies are explicitly ordered.

## 4. gstack-qa — Systematic QA
Applied to define test matrices for each phase: unit tests, integration tests, benchmark gates, and manual verification steps.

## 5. Spacedrive Textbook — Cross-Reference
Each phase is cross-referenced against the relevant Spacedrive chapter(s) to validate pattern alignment and identify where HyprDrive diverges intentionally.

---

# PART II: VALIDATION REPORT

## Section 1 — Compliance Confirmation

### Phase -1: Foundation Spike ✅ (Spacedrive Ch9, Ch10)

| Requirement | Source | Status | Evidence |
|-------------|--------|--------|----------|
| Spike binary tests MFT enumeration | Architecture §7 | ✅ | `spike/src/main.rs` (now deleted per exit criteria) |
| `usn-journal-rs` v0.4 validated | spike-report.md | ✅ | MftEntry fields documented |
| `jwalk` baseline: ~16k entries/sec | Architecture §7 | ✅ | 411,586 entries / 25.6s measured |
| Fallback chain validated | Architecture §7 | ✅ | Non-admin → jwalk triggered |
| Learnings documented | GSD Iron Law #2 | ✅ | `docs/spike-report.md` |
| Crate quality assessed | Architecture §7 | ✅ | Dependency table in report |
| Two-phase MFT finding | spike-report.md | ✅ | MftEntry has NO size fields |
| spike/ deleted after review | Phase -1 exit criteria | ✅ | Deleted in deviation sweep |

**Spacedrive cross-ref**: Ch9 Lesson #4 — Spacedrive uses basic `notify`+`walkdir`. HyprDrive's spike validates MFT is 6-10x faster. This is our core differentiator.

### Phase 0: Workspace + Tooling ✅ (Spacedrive Ch1, Ch10)

| Requirement | Source | Status | Evidence |
|-------------|--------|--------|----------|
| Cargo workspace (23 members) | Architecture §6 | ✅ | `Cargo.toml` with all crates |
| Workspace dependencies locked | GSD | ✅ | `[workspace.dependencies]` section |
| Clippy lints: `unwrap_used=deny, expect_used=deny, panic=deny` | ADR-006 | ✅ | `[workspace.lints.clippy]` |
| `tracing-subscriber` with env-filter | ADR-007 | ✅ | daemon `main.rs` |
| CI pipeline (3-OS matrix) | GSD Iron Law #4 | ✅ | `.github/workflows/ci.yml` |
| Benchmark harness (Criterion) | GSD Iron Law #4 | ✅ | `core/benches/benchmarks.rs` + `.github/workflows/bench.yml` |
| Frontend scaffold (Tauri) | Architecture §24 | ✅ | `apps/tauri/` with Vite+TS |
| `cargo nextest` in CI | GSD | ✅ | `ci.yml` runs `cargo nextest run --workspace` |
| Daemon binary scaffolded | Architecture §5, ADR-004 | ✅ | `apps/daemon/src/main.rs` |
| CLI binary scaffolded | Architecture §5 | ✅ | `apps/cli/src/main.rs` |
| Platform helper stubs | Architecture §7 | ✅ | `helpers/hyprdrive-helper-{windows,macos,linux}/` |

**Spacedrive cross-ref**: Ch1 Bootstrap — Spacedrive's `Core::new_with_config()` init. HyprDrive daemon follows same sequential bootstrap. Ch10 maps Phase 0 to `Cargo.toml workspace, turbo.json, package.json`.

### Phase 1: Domain Layer ✅ (Spacedrive Ch2)

| Requirement | Source | Status | Evidence |
|-------------|--------|--------|----------|
| Object/Location split | Spacedrive Ch2, Architecture §9 | ✅ | `objects` + `locations` tables in migrations |
| ObjectId: BLAKE3 content-addressed (32 bytes) | Architecture §8 | ✅ | `id.rs:23-26` — `ObjectId::from_blake3()` |
| UUID-based IDs: LocationId, VolumeId, LibraryId, DeviceId, TagId, VirtualFolderId | Architecture §9 | ✅ | `id.rs` `define_id!` macro |
| ObjectKind enum (16 variants) | Architecture §9 | ✅ | `enums.rs` |
| FileCategory (10 + Unknown) | Architecture §9 | ✅ | `enums.rs` |
| StorageTier (Hot/Warm/Cold/Glacier) | ADR-005, Architecture §17 | ✅ | `enums.rs` |
| FilterExpr composable AST → SQL | Architecture §9 | ✅ | `filter.rs` — 9 variants + And/Or/Not |
| SortField → SQL column mapping | Architecture §9 | ✅ | `sort.rs` |
| VectorClock + partial_order() | Architecture §16, Spacedrive Ch5 | ✅ | `sync.rs` — BTreeMap-based, concurrent detection |
| SyncOperation with ULID ordering | Architecture §16 | ✅ | `sync.rs` — ULID + VectorClock |
| CapabilityToken + RevocationList | Architecture §13 | ✅ | `security.rs` |
| TransferCheckpoint + TransferRoute | Architecture §15 | ✅ | `transfer.rs` — RoaringBitmap chunks |
| UndoStack (50-entry cap) | Architecture §11, Spacedrive Ch3 | ✅ | `undo.rs` — LIFO with capacity |
| VirtualFolder (saved filter query) | Architecture §9 | ✅ | `virtual_folder.rs` |
| Tag (3 semantic names: canonical, display, formal) | Architecture §9, Spacedrive Ch2 | ✅ | `tags.rs` — P1-01 fix applied |

**Spacedrive cross-ref**: Ch2 Domain Modeling — Object/Location split, Tag system with semantic names. HyprDrive adds: closure table, VirtualFolder, UndoStack, VectorClock (vs Spacedrive's HLC), CapabilityToken (vs simple device auth).

### Phase 2: Database Layer ✅ (Spacedrive Ch4)

| Requirement | Source | Status | Evidence |
|-------------|--------|--------|----------|
| SQLite WAL mode | ADR-001, Spacedrive Ch4 | ✅ | `pool.rs:24` |
| `synchronous = NORMAL` | ADR-001 | ✅ | `pool.rs:25` |
| `foreign_keys = ON` | ADR-001 | ✅ | `pool.rs:26` |
| `busy_timeout = 5000ms` | ADR-001 | ✅ | `pool.rs:27` |
| `mmap_size = 256MB` | ADR-001 (HyprDrive addition) | ✅ | `pool.rs:28` |
| `journal_size_limit = 64MB` | ADR-001 | ✅ | `pool.rs:29` |
| 9 embedded migrations | Architecture §9 | ✅ | `core/migrations/001-009` |
| FTS5 with trigram tokenizer | Architecture §18 | ✅ | `009_fts.sql` |
| redb 5-cache hot path | Architecture §9, Spacedrive Ch4 | ✅ | `cache.rs` — inode, thumb, query, xfer, dir_size |
| `FileRow` computed JOIN struct | Spacedrive Ch2 | ✅ | `types.rs` |
| Keyset pagination + idx_loc_sort | Architecture §9 | ✅ | `queries.rs` — `list_files_fast()` |
| 200+ file types seeded | Architecture §9 | ✅ | `008_file_types.sql` |
| Daemon wires DB + cache on startup | ADR-004 | ✅ | `daemon/main.rs` — P2-01 fix |
| `dirs` for platform data dir | ADR-004 | ✅ | `dirs::data_dir()` — X-02 fix |
| `#[tracing::instrument]` on DB functions | ADR-007 | ✅ | `pool.rs`, `queries.rs` — DEV-09 fix |

**Spacedrive cross-ref**: Ch4 Infrastructure — Same pragmas (minus `mmap_size` which is HyprDrive's addition). Spacedrive uses SeaORM; HyprDrive uses sqlx (lighter, no ORM overhead). Ch4 EventBus — not yet implemented (Phase 9/10).

**Overall Phase Compliance Score: 9.6 / 10**

Deductions:
- -0.2: Test code still uses `.expect()` in cache.rs test helper (workspace lint denies it, but tests pass because `#[cfg(test)]` modules aren't subject to workspace-level clippy lint enforcement by default in practice)
- -0.2: No `list_files_fast` benchmark in `core/benches/` yet (planned for Phase 3)

---

## Section 2 — Deviations, Gaps, and Missing Elements

### Previously Identified (all resolved in deviation sweep)

| ID | Status | Summary |
|----|--------|---------|
| DEV-01 | ✅ FIXED | Plan v3.0→v4.0, ADR table added |
| DEV-02 | ✅ FIXED | `usn-journal-rs = "0.4"` in plan |
| DEV-03 | ✅ FIXED (plan) | Phase 3 split into topology + enrichment passes |
| DEV-04 | ✅ FIXED (plan) | OsString handling specified in IndexEntry |
| DEV-05 | ✅ FIXED | `spike/` deleted |
| DEV-06 | ✅ FIXED | `tag_closure` in plan step 2.1.5 |
| DEV-07 | ✅ FIXED | Phase 17 → 3 crates (ffmpeg/images/media-metadata) |
| DEV-08 | ✅ FIXED (plan) | thiserror in fs-indexer from Phase 3 |
| DEV-09 | ✅ FIXED | `#[tracing::instrument]` on DB layer |
| DEV-10 | ✅ FIXED | `VirtualFolderId` type + virtual_folder.rs updated |
| DEV-11 | ✅ FIXED | `.ok().unwrap()` → `Result<(), Box<dyn Error>>` + `?` |
| DEV-12 | ✅ N/A | TTL not yet implemented (future phase) |
| DEV-13 | ✅ FIXED (plan) | `list_files_fast` bench spec'd in Phase 3 |

### Remaining Architectural Gaps (not blocking, but track)

| ID | Severity | Gap | Resolution Phase |
|----|----------|-----|-----------------|
| GAP-01 | Info | Spacedrive Ch1 `CoreContext` god-struct pattern not yet addressed | Phase 9 (CQRS): implement sub-contexts per Ch9 Lesson #1 |
| GAP-02 | Info | No SdPath-style universal addressing | Phase 6 (Unified Indexer): add `HdPath` enum per Ch2 |
| GAP-03 | Info | Sidecar addressing not yet designed | Phase 17 (Media): implement per Ch5 |
| GAP-04 | Low | `bench.yml` triggers on `push` to `main` only — no PR bench comparison | Phase 0 enhancement: add `pull_request` trigger with `criterion-compare` |
| GAP-05 | Low | ci.yml branches filter is `[main]` but repo default branch may be `master` | Verify: `git branch --show-current` on main repo |

---

## Section 3 — Specific, Actionable Recommendations

### R-01: Verify CI branch name matches default branch
```bash
# Run this to verify
git -C D:/HyprDrive branch --show-current
# If "master", update ci.yml and bench.yml: branches: [master]
```

### R-02: Add PR benchmark comparison to bench.yml
Add `on: pull_request` trigger and `criterion-compare-action` to catch perf regressions before merge.

### R-03: Consider sub-contexts for Phase 9
Per Spacedrive Ch9 Lesson #1, break the daemon's context into:
- `StorageContext` (pool, cache, volumes)
- `IndexContext` (fs-indexer, cursors, priority graph)
- `OperationsContext` (CQRS actions, undo stack)
- `NetworkContext` (iroh, sync, transfer) — Phase 13+

### R-04: Define HdPath enum in Phase 6
```rust
pub enum HdPath {
    Physical { device_id: DeviceId, path: PathBuf },
    Cloud { service: CloudService, bucket: String, key: String },
    Content { object_id: ObjectId },
    Sidecar { object_id: ObjectId, kind: SidecarKind, format: ImageFormat },
}
```

### R-05: Add Phase 3 pre-flight checklist
Before starting Phase 3 implementation, verify:
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] All 82 tests pass
- [ ] `spike/` is deleted
- [ ] Plan text has correct crate names
- [ ] CI pipeline runs on correct branch name

---

# PART III: IMPROVED IMPLEMENTATION PLAN v3.0

## Design Principles

1. **Breadcrumb-level detail**: Every step has a file path, a test, and a commit message
2. **TDD enforced**: Test listed BEFORE implementation in every step
3. **Atomic commits**: One commit per logical unit (GSD Iron Law #3)
4. **Phase exit criteria**: Explicit gates (GSD Iron Law #2)
5. **Benchmark gates**: Performance targets enforced in CI (GSD Iron Law #4)
6. **ADR compliance**: Every phase notes which ADRs apply
7. **Spacedrive cross-ref**: Every phase notes which chapters to study

---

## Phase 3 — Windows MFT Indexer

**Goal**: Two-phase MFT scan (topology + size enrichment) + USN delta + jwalk fallback. < 1.5s at 100k files.
**Duration**: ~2 weeks · **Depends on**: Phase 2
**ADRs**: ADR-006 (thiserror in library crate), ADR-007 (tracing spans)
**Spacedrive cross-ref**: Ch9 Lesson #4 (basic indexing), Ch10 Phase 3

### Architecture Diagram

```
                      ┌─────────────────────────────────┐
                      │       crates/fs-indexer          │
                      │   (library crate → thiserror)    │
                      ├─────────────────────────────────┤
                      │                                  │
                      │  ┌──────────────────────────┐   │
    Admin path:       │  │  platform/windows/        │   │
    ┌─────────┐       │  │  ├── detect.rs            │   │
    │  Helper  │──IPC──│  │  ├── mft.rs (topology)   │   │
    │  .exe    │       │  │  ├── enrich.rs (sizes)    │   │
    └─────────┘       │  │  ├── usn.rs (delta)       │   │
                      │  │  └── scanner.rs (full)     │   │
    Non-admin path:   │  └──────────────────────────┘   │
    ┌─────────┐       │                                  │
    │  jwalk   │──────►│  types.rs (IndexEntry, FsChange)│
    │  fallback│       │  error.rs (FsIndexerError)      │
    └─────────┘       │  lib.rs (VolumeIndexer trait)    │
                      └────────────┬────────────────────┘
                                   │
                      ┌────────────▼────────────────────┐
                      │         SQLite (Phase 2)         │
                      │  objects + locations tables       │
                      ├─────────────────────────────────┤
                      │         redb (Phase 2)           │
                      │  SCAN_CURSORS + INODE_CACHE      │
                      └─────────────────────────────────┘
```

### State Machine: Scan Lifecycle

```
IDLE → DETECTING_FS → [NTFS] → MFT_TOPOLOGY → SIZE_ENRICHMENT → DB_INSERT → IDLE
                      [FAT]  → JWALK_SCAN → DB_INSERT → IDLE

Delta path:
IDLE → USN_QUERY → APPLY_DELTAS → DB_UPSERT → IDLE
```

### Failure Modes

| Failure | Mitigation |
|---------|------------|
| MFT access denied (non-admin) | Auto-fallback to jwalk (3-5s vs 1.5s) |
| USN journal wrapped (too old cursor) | Full re-scan, emit `tracing::warn!` |
| File locked during enrichment | Open with `FILE_SHARE_READ|WRITE`, size=0 if fails |
| Handle exhaustion during batch enrich | Process FRNs in chunks of 1000 |
| Corrupt MFT entry | Skip entry, `tracing::error!`, continue |
| Named pipe IPC failure to helper | Fallback to jwalk, `tracing::error!` |

### User Stories (Ralph format)

#### US-301: Filesystem Detection
```
As the fs-indexer, I want to detect whether a volume is NTFS, FAT32, or exFAT
so that I can choose the optimal scanning strategy.

Acceptance Criteria:
- [ ] `detect_filesystem("C:\\")` returns `FilesystemKind::Ntfs` on NTFS
- [ ] `detect_filesystem` on USB FAT32 returns `FilesystemKind::Fat32`
- [ ] Unknown filesystems return `FilesystemKind::Unknown`
- [ ] cargo clippy passes
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.1.1 | Define `FilesystemKind` enum: `Ntfs, Fat32, ExFat, Apfs, Ext4, Unknown` | `crates/fs-indexer/src/types.rs` | Unit: each variant serializes |
| 3.1.2 | Define `FsIndexerError` with `thiserror`: `MftAccess`, `JournalSeek`, `IoError`, `PermissionDenied` | `crates/fs-indexer/src/error.rs` | Unit: Display trait works |
| 3.1.3 | Implement `detect_filesystem(path: &Path) -> FilesystemKind` using `GetVolumeInformationW` | `crates/fs-indexer/src/platform/windows/detect.rs` | Integration: C:\ returns Ntfs |
| 3.1.4 | Add `thiserror = { workspace = true }`, `tracing = { workspace = true }` to `crates/fs-indexer/Cargo.toml` | `crates/fs-indexer/Cargo.toml` | `cargo check` |

**Commit**: `feat(fs-indexer): add filesystem detection + FsIndexerError`

---

#### US-302: IndexEntry Type
```
As the fs-indexer, I want a unified IndexEntry struct that holds all metadata
so that all platform scanners produce the same output shape.

Acceptance Criteria:
- [ ] IndexEntry has: fid, parent_fid, name (OsString), size, allocated_size, is_dir, modified_at, attributes
- [ ] IndexEntry serializes to JSON correctly
- [ ] FsChange enum has Created, Deleted, Moved, Resized variants
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.2.1 | Define `IndexEntry` struct with `OsString` name field | `crates/fs-indexer/src/types.rs` | Unit: construct, assert fields |
| 3.2.2 | Add `name_display(&self) -> String` helper using `to_string_lossy()` | Same | Unit: emoji filename converts |
| 3.2.3 | Define `FsChange` enum: `Created(IndexEntry)`, `Deleted(u64)`, `Moved { old_fid, new: IndexEntry }`, `Resized(u64, u64)` | Same | Unit: pattern match all variants |
| 3.2.4 | Add `IndexCursor` enum: `Mft(u64)`, `Usn(i64)`, `Mtime(DateTime)` | Same | Unit: serialize/deserialize |

**Commit**: `feat(fs-indexer): add IndexEntry, FsChange, IndexCursor types`

---

#### US-303: MFT Topology Pass
```
As the fs-indexer, I want to enumerate the MFT to build a directory tree topology
so that I get the complete file/folder structure in < 1s.

Acceptance Criteria:
- [ ] mft_enumerate_topology returns Vec<(fid, parent_fid, OsString, is_dir)>
- [ ] Skips system metadata files (fid < 24)
- [ ] Handles NTFS junctions (reparse points)
- [ ] Benchmark: < 1s for 100k entries
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.3.1 | Add `usn-journal-rs = "0.4"` to platform deps | `crates/fs-indexer/Cargo.toml` | `cargo check` |
| 3.3.2 | Implement `mft_enumerate_topology(volume: &str) -> Result<Vec<MftNode>, FsIndexerError>` | `crates/fs-indexer/src/platform/windows/mft.rs` | — |
| 3.3.3 | Test: returns > 10,000 entries on C:\ | Same | Integration (needs admin) |
| 3.3.4 | Test: root entry has parent_fid == self fid | Same | Integration |
| 3.3.5 | Test: directory entries have is_dir = true | Same | Integration |
| 3.3.6 | Edge: skip entries with fid < 24 ($MFT, $LogFile etc) | Same | Unit: synthetic entry with fid=5 skipped |
| 3.3.7 | Edge: `FILE_ATTRIBUTE_REPARSE_POINT` flagged, not followed | Same | Unit: attribute check |
| 3.3.8 | Add `#[tracing::instrument(fields(volume))]` per ADR-007 | Same | — |
| 3.3.9 | Benchmark: topology on 100k synthetic fixture | Same | `< 1s` |

**Commit**: `feat(fs-indexer): MFT topology enumeration via usn-journal-rs`

---

#### US-304: Size Enrichment Pass
```
As the fs-indexer, I want to batch-query file sizes after topology enumeration
so that every IndexEntry has accurate size and allocated_size.

Acceptance Criteria:
- [ ] enrich_sizes returns HashMap<fid, (size, allocated_size)>
- [ ] Batch processes FRNs in chunks of 1000
- [ ] Handles access-denied files gracefully (size=0 + warn log)
- [ ] compressed files: allocated_size < size
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.4.1 | Implement `enrich_sizes(volume: &str, fids: &[u64]) -> Result<HashMap<u64, SizeInfo>>` using `GetFileInformationByHandleEx` | `crates/fs-indexer/src/platform/windows/enrich.rs` | — |
| 3.4.2 | Test: enriched entry has `size > 0` for known file | Same | Integration |
| 3.4.3 | Test: `allocated_size >= size` for uncompressed | Same | Integration |
| 3.4.4 | Test: compressed NTFS file has `allocated_size < size` | Same | Integration (if available) |
| 3.4.5 | Edge: access denied → size=0, `tracing::warn!("access denied for fid={fid}")` | Same | Unit: mock denied handle |
| 3.4.6 | Edge: batch in chunks of 1000 → no handle exhaustion | Same | Unit: 5000 entries processes correctly |
| 3.4.7 | Open files with `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE` | Same | — |
| 3.4.8 | Benchmark: enrich 100k entries | Same | `< 500ms` |

**Commit**: `feat(fs-indexer): batch size enrichment via GetFileInformationByHandleEx`

---

#### US-305: Full Scan (Topology + Enrich)
```
As the fs-indexer, I want a full_scan function that combines topology and enrichment
so that I get a complete Vec<IndexEntry> for an NTFS volume.

Acceptance Criteria:
- [ ] full_scan returns entries with all fields populated
- [ ] Sum of sizes within 5% of `dir /s` output
- [ ] Benchmark: < 1.5s for 100k files
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.5.1 | Implement `full_scan(volume: &str) -> Result<Vec<IndexEntry>>` = topology + enrich | `crates/fs-indexer/src/platform/windows/scanner.rs` | — |
| 3.5.2 | Test: all entries have non-empty name | Same | Integration |
| 3.5.3 | Test: size sum within 5% of `dir /s` | Same | Integration (manual) |
| 3.5.4 | Persist scan cursor to `redb::SCAN_CURSORS` | Same | Unit: cursor round-trips |
| 3.5.5 | Benchmark: full_scan on 100k fixture | Same | **< 1.5s** |

**Commit**: `feat(fs-indexer): full_scan combining topology + enrichment`

---

#### US-306: USN Journal Delta
```
As the fs-indexer, I want to query the USN journal for changes since the last scan
so that I can incrementally update the index without re-scanning.

Acceptance Criteria:
- [ ] Detects file create, rename, delete, modify events
- [ ] Persists cursor between daemon restarts
- [ ] Delta query < 100ms for 1000 changes
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.6.1 | Implement `query_usn_delta(volume, cursor) -> Result<Vec<FsChange>>` | `crates/fs-indexer/src/platform/windows/usn.rs` | — |
| 3.6.2 | Test: create file → `FsChange::Created` | Same | Integration |
| 3.6.3 | Test: rename file → `FsChange::Moved` | Same | Integration |
| 3.6.4 | Test: delete file → `FsChange::Deleted` | Same | Integration |
| 3.6.5 | Test: modify file → `FsChange::Resized` | Same | Integration |
| 3.6.6 | Edge: journal wrapped → return error, caller does full re-scan | Same | Unit: synthetic wrap condition |
| 3.6.7 | Debounce: coalesce burst of events within 100ms | Same | Unit: 10 rapid changes → 1 event |
| 3.6.8 | Benchmark: delta 1000 changes | Same | **< 100ms** |

**Commit**: `feat(fs-indexer): USN journal delta tracking`

---

#### US-307: jwalk Fallback Scanner
```
As the fs-indexer, I want a fallback scanner using jwalk for FAT32/exFAT volumes
so that non-NTFS volumes are still indexed.

Acceptance Criteria:
- [ ] JwalkScanner returns IndexEntry from DirEntry
- [ ] allocated_size estimated from cluster size on FAT
- [ ] Delta via mtime diff on re-walk
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.7.1 | Add `jwalk = "0.8"` to fs-indexer deps | `crates/fs-indexer/Cargo.toml` | — |
| 3.7.2 | Implement `JwalkScanner::full_scan(path) -> Vec<IndexEntry>` | `crates/fs-indexer/src/fallback/jwalk.rs` | — |
| 3.7.3 | Test: returns entries with size > 0 | Same | Unit (temp dir) |
| 3.7.4 | Test: FAT allocated_size = ceil(size / cluster_size) * cluster_size | Same | Unit |
| 3.7.5 | Implement delta: re-walk + diff mtimes | Same | Unit |

**Commit**: `feat(fs-indexer): jwalk fallback for FAT32/exFAT`

---

#### US-308: Privileged Helper
```
As the daemon, I want a privileged helper binary for MFT access
so that the daemon doesn't need to run as admin.

Acceptance Criteria:
- [ ] Named pipe IPC: ScanRequest → ScanResult
- [ ] Auto-fallback when no admin
- [ ] Service install/uninstall
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.8.1 | Define `ScanRequest`/`ScanResult` as msgpack-serializable | `helpers/hyprdrive-helper-windows/src/protocol.rs` | Unit |
| 3.8.2 | Named pipe server in helper | `helpers/hyprdrive-helper-windows/src/main.rs` | — |
| 3.8.3 | Named pipe client in fs-indexer | `crates/fs-indexer/src/platform/windows/ipc.rs` | — |
| 3.8.4 | Auto-fallback: try pipe → if fail, use jwalk | `crates/fs-indexer/src/platform/windows/scanner.rs` | Unit: mock pipe failure |
| 3.8.5 | Service install: `sc.exe create` wrapper | `helpers/hyprdrive-helper-windows/src/service.rs` | Manual |

**Commit**: `feat(helper-windows): named pipe IPC for MFT access`

---

#### US-309: Benchmark Gate
```
As the CI pipeline, I want Phase 3 benchmark gates
so that performance regressions are caught before merge.

Acceptance Criteria:
- [ ] list_files_fast(10k) < 1ms
- [ ] list_files_fast(100k) < 5ms
- [ ] redb inode lookup (1M keys) < 1μs
```

**Steps**:
| # | Action | File | Test |
|---|--------|------|------|
| 3.9.1 | Add `bench_list_files_fast_10k` with in-memory SQLite fixture | `core/benches/benchmarks.rs` | < 1ms |
| 3.9.2 | Add `bench_list_files_fast_100k` | Same | < 5ms |
| 3.9.3 | Add `bench_redb_inode_lookup_1m` | Same | < 1μs/lookup |

**Commit**: `bench: add list_files_fast + redb inode benchmarks (Phase 3 gate)`

---

### Phase 3 Exit Criteria
- [ ] `full_scan` < 1.5s at 100k files (MFT path)
- [ ] File sizes within 5% of `dir /s`
- [ ] USN delta < 100ms for 1000 changes
- [ ] jwalk fallback works on FAT32
- [ ] Helper IPC round-trips correctly
- [ ] `list_files_fast(100k)` < 5ms
- [ ] `#[tracing::instrument]` on all public I/O functions
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] All existing 82+ tests still pass + new tests pass

---

## Phase 4 — macOS Indexer *(Parallel with Phase 3)*

**Goal**: `getattrlistbulk` + FSEvents. < 4s for 100k. macOS only.
**Duration**: ~1.5 weeks · **Depends on**: Phase 2
**ADRs**: ADR-006, ADR-007
**Spacedrive cross-ref**: Ch9 Lesson #4

### User Stories

#### US-401: macOS Filesystem Detection
| # | Action | File |
|---|--------|------|
| 4.1.1 | `detect_filesystem(mount) -> FilesystemKind` (APFS/HFS+/FAT) | `platform/macos/detect.rs` |
| 4.1.2 | Test: `/` returns APFS | Same |

#### US-402: getattrlistbulk Scanner
| # | Action | File |
|---|--------|------|
| 4.2.1 | Add `getattrlistbulk = "0.1"` to platform deps | `Cargo.toml` |
| 4.2.2 | `full_scan(mount) -> Vec<IndexEntry>` via getattrlistbulk | `platform/macos/bulk.rs` |
| 4.2.3 | Test: entries have `allocated_size` via `ATTR_FILE_ALLOCSIZE` | Same |
| 4.2.4 | Edge: NFD vs NFC Unicode normalization | Same |
| 4.2.5 | Edge: Firmlink `/System/Volumes/Data` → skip | Same |
| 4.2.6 | Edge: .DS_Store → included, flagged as system | Same |
| 4.2.7 | Benchmark: < 4s for 100k files | Same |

#### US-403: FSEvents Delta
| # | Action | File |
|---|--------|------|
| 4.3.1 | `fsevent-stream = "0.2"` with `kFSEventStreamCreateFlagFileEvents` | `platform/macos/fsevents.rs` |
| 4.3.2 | Test: create/rename/delete/modify → correct FsChange variant | Same |
| 4.3.3 | Debounce: 100ms coalescing window | Same |
| 4.3.4 | Benchmark: delta 1000 changes < 100ms | Same |

#### US-404: XPC Helper
| # | Action | File |
|---|--------|------|
| 4.4.1 | XPC service for Full Disk Access | `helpers/hyprdrive-helper-macos/` |
| 4.4.2 | Auto-fallback: no FDA → restricted jwalk scope | Same |

**Exit Criteria**: Scan < 4s at 100k · FSEvents detects file-level changes · Symlinks flagged · Unicode normalized

---

## Phase 5 — Linux Indexer *(Parallel with Phase 3)*

**Goal**: `io_uring` + `getdents64` + `fanotify`. < 2s for 100k. Linux only.
**Duration**: ~1.5 weeks · **Depends on**: Phase 2
**ADRs**: ADR-006, ADR-007

### User Stories

#### US-501: io_uring Scanner
| # | Action | File |
|---|--------|------|
| 5.1.1 | Add `tokio-uring = "0.5"`, `fanotify-rs = "0.1"`, `nix = "0.29"` | `Cargo.toml` |
| 5.1.2 | `full_scan(mount) -> Vec<IndexEntry>` via io_uring + getdents64 | `platform/linux/uring.rs` |
| 5.1.3 | `allocated_size = stat.st_blocks * 512` | Same |
| 5.1.4 | Edge: skip `/proc`, `/sys`, `/dev` pseudo-fs | Same |
| 5.1.5 | Edge: bind mounts → detect, skip duplicates | Same |
| 5.1.6 | Edge: sparse files → allocated_size < size valid | Same |
| 5.1.7 | 64 concurrent io_uring ops | Same |
| 5.1.8 | Benchmark: < 2s for 100k files | Same |

#### US-502: fanotify Delta
| # | Action | File |
|---|--------|------|
| 5.2.1 | `FAN_REPORT_FID | FAN_REPORT_NAME` + `FAN_MARK_FILESYSTEM` | `platform/linux/fanotify.rs` |
| 5.2.2 | Test: create/rename/delete/modify events | Same |
| 5.2.3 | Event batching: 4KB buffer, batch reads | Same |

#### US-503: inotify Fallback (kernel < 5.10)
| # | Action | File |
|---|--------|------|
| 5.3.1 | inotify + jwalk when io_uring unavailable | `platform/linux/inotify_fallback.rs` |
| 5.3.2 | Handle `max_user_watches` limit | Same |

#### US-504: setuid Helper
| # | Action | File |
|---|--------|------|
| 5.4.1 | `hyprdrive-helper-linux` with `CAP_SYS_ADMIN` | `helpers/hyprdrive-helper-linux/` |
| 5.4.2 | Seccomp sandbox: restrict to fanotify + stat + read/write | Same |

**Exit Criteria**: Scan < 2s at 100k · fanotify detects changes · inotify fallback works · pseudo-fs skipped

---

## Phase 6 — Unified Indexer Trait

**Goal**: Cross-platform `VolumeIndexer` trait. Cursor persistence.
**Duration**: ~1 week · **Depends on**: Phases 3+4+5
**Spacedrive cross-ref**: Ch5 (Volume Management), Ch2 (SdPath)

### User Stories

| # | Action |
|---|--------|
| 6.1 | Define `trait VolumeIndexer { fn full_scan, fn delta, fn detect_fs }` |
| 6.2 | `IndexCursor` enum with 5 variants: Mft, Usn, FSEvents, Fanotify, Mtime |
| 6.3 | Platform dispatch: `VolumeIndexer::for_platform() -> Box<dyn VolumeIndexer>` |
| 6.4 | Cursor persistence to `redb::SCAN_CURSORS` |
| 6.5 | Priority graph: Desktop > Documents > Downloads > node_modules |
| 6.6 | Define `HdPath` enum per Spacedrive Ch2 `SdPath` pattern |
| 6.7 | Add `#[tracing::instrument]` to trait methods |

**Exit Criteria**: Single API for all platforms · Cursors survive restart · Priority ordering works

---

## Phase 7 — Hashing & Object Pipeline

**Goal**: BLAKE3 content hashing. ObjectId creation. Dedup detection.
**Duration**: ~1.5 weeks · **Depends on**: Phase 6
**Spacedrive cross-ref**: Ch2 (ContentIdentity), Ch4 (inode cache)

| # | Action |
|---|--------|
| 7.1 | Streaming BLAKE3: < 512MB files → buffered read, > 512MB → mmap |
| 7.2 | Inode cache: skip rehashing if (volume, inode, mtime) matches redb |
| 7.3 | On Windows: reuse file handle from enrichment (file already open) |
| 7.4 | `ObjectIndexed` event for EventBus (Phase 10) |
| 7.5 | Duplicate detection: same ObjectId at 2+ Locations |
| 7.6 | Benchmark: 1GB file < 1s, 100k re-index (with cache) < 5s |

---

## Phase 8 — Disk Intelligence (WizTree Engine)

**Goal**: Squarified treemap, size aggregation, insights.
**Duration**: ~1.5 weeks · **Depends on**: Phase 7
**Spacedrive cross-ref**: Ch9 Lesson #5 (missing in Spacedrive — our differentiator)

| # | Action |
|---|--------|
| 8.1 | Squarified treemap layout (Bruls et al. 2000) in `crates/disk-intelligence/` |
| 8.2 | Live bubble-up: file size change → propagate to all ancestors |
| 8.3 | Top-N queries: largest files, largest dirs, stale files |
| 8.4 | Type breakdown: extension → total bytes, with hex colors from file_types |
| 8.5 | Wasted space: build artifacts, .git/objects, node_modules, duplicate files |
| 8.6 | `dir_sizes` table aggregation + `DIR_SIZE_CACHE` in redb |
| 8.7 | Benchmark: treemap layout for 1M nodes < 100ms |

---

## Phase 9 — CQRS Operations Layer

**Goal**: File actions with undo. Command/Query separation.
**Duration**: ~1.5 weeks · **Depends on**: Phase 7
**Spacedrive cross-ref**: Ch3 (CQRS pattern), Ch1 (ActionManager)

| # | Action |
|---|--------|
| 9.1 | `trait CoreAction { type Input; type Output; fn execute(...); }` |
| 9.2 | `inventory` crate for compile-time action registration |
| 9.3 | Actions: Copy, Move, Delete (soft → trash), Rename, CreateDir, BulkTag, EmptyTrash |
| 9.4 | UndoStack integration: each action produces inverse_action JSON |
| 9.5 | Smart rename: EXIF DateTimeOriginal template `{year}/{month}/{original}` |
| 9.6 | SessionContext: device_id, permissions, audit metadata |
| 9.7 | Sub-contexts: `StorageContext`, `IndexContext`, `OperationsContext` (per R-03) |
| 9.8 | rspc router for frontend exposure |

---

## Phase 10 — File Watching & Real-Time

**Goal**: EventBus + platform watchers + WebSocket bridge.
**Duration**: ~1 week · **Depends on**: Phase 9
**Spacedrive cross-ref**: Ch4 (EventBus), Ch5 (File Watcher)

| # | Action |
|---|--------|
| 10.1 | EventBus: broadcast channel, `Event` enum with 20+ variants |
| 10.2 | Separate LogBus (per Spacedrive Ch4 pattern) |
| 10.3 | Platform watcher integration: USN (win), FSEvents (mac), fanotify (linux) |
| 10.4 | Debounce: < 100ms batch window |
| 10.5 | WebSocket bridge to frontend (TanStack Query invalidation) |

---

## Phase 11 — Desktop UI (Tauri → Daemon)

**Goal**: File explorer connecting to daemon via rspc WebSocket.
**Duration**: ~3 weeks · **Depends on**: Phases 8+10
**Spacedrive cross-ref**: Ch8 (rspc + Specta), Ch8 (Tauri)

| # | Action |
|---|--------|
| 11.1 | `<FileList>` with TanStack Virtual (1M rows @ 60fps) |
| 11.2 | Grid/list views, multi-column sorting, breadcrumb nav |
| 11.3 | Blurhash thumbnails (placeholder until Phase 17 media pipeline) |
| 11.4 | Treemap SVG with hover/zoom (disk intelligence) |
| 11.5 | Context menu, drag-and-drop, undo/redo keyboard shortcuts |
| 11.6 | Real-time search via FTS5 (< 30ms) |
| 11.7 | Debug overlay: scan progress, cache hit rates, event stream |

**═══ v1.0 CUT LINE ═══** — Fastest file explorer + WizTree-level disk analysis

---

## Phases 12–21 (Post v1.0)

These phases follow the same breadcrumb pattern above. Key structural corrections from v2.1:

| Phase | Duration | Key change from v2.1 |
|-------|----------|----------------------|
| 12 — Crypto | 1.5w | ADR-003: ChaCha20-Poly1305 ONLY. Span: `crypto:{op}` |
| 13 — P2P | 2w | Iroh + mDNS + Axum `:7421` + Prometheus `:7422` (ADR-007) |
| 14 — Blip Transfer | 2w | QUIC + RoutingOracle + BandwidthSaturator. Span: `xfer:{id}` |
| 15 — CRDT Sync | 2.5w | VectorClock (not HLC). OpLog < 1000 ops → MerkleDiff ≥ 1000. Span: `sync:{peer}` |
| 15.5 — Cloud | 2.5w | OpenDAL 7 backends. ADR-005: Tier 2/3, not core. OAuth encrypted in redb |
| 16 — Mobile | 3w | ADR-008: `hyprdrive-mobile-core` — NO Axum/Iroh/WASM/media. Sync as CRDT peer |
| 17 — Media | 2w | **3 crates**: `ffmpeg`, `images`, `media-metadata`. ADR-005: Tier 2 ML optional |
| 18 — WASM | 2w | ADR-002: wasmtime AOT. 256MB/extension. Epoch interruption. Span: `ext:{name}` |
| 19 — Search | 2w | Tantivy + HNSW + RRF merge. Span: `search:{hash}`. ADR-005: CLIP is Tier 2 |
| 20 — Extensions | 4w | 7 extensions in 4 waves. Each < 10MB RAM |
| 20.5 — Integrations | 3w | 6 connectors: Gmail, Outlook, Chrome, Spotify, GitHub, Obsidian |
| 21 — Polish | 4w | Lite binary (egui+wgpu < 40MB), app store submissions, launch |

---

# PART IV: TEST MATRIX (gstack-qa)

| Phase | Scenario | Input | Expected | Type |
|-------|----------|-------|----------|------|
| 3 | Happy: MFT full scan | NTFS C:\ | > 10k entries, sizes > 0 | Integration |
| 3 | Happy: USN delta | Create file after scan | FsChange::Created | Integration |
| 3 | Edge: Non-admin | No SeManageVolume | Fallback to jwalk | Integration |
| 3 | Edge: Locked file | File in use | size=0, warn logged | Unit |
| 3 | Edge: Reparse point | Junction | Flagged, not followed | Unit |
| 3 | Perf: full_scan 100k | Synthetic fixture | < 1.5s | Benchmark |
| 3 | Perf: USN delta 1k | 1000 changes | < 100ms | Benchmark |
| 4 | Happy: getattrlistbulk | macOS /Users | > 1k entries | Integration |
| 4 | Edge: Firmlink | /System/Volumes/Data | Skipped | Integration |
| 5 | Happy: io_uring scan | /home | > 1k entries | Integration |
| 5 | Edge: pseudo-fs | /proc, /sys | Skipped | Integration |
| 6 | Happy: trait dispatch | Each platform | Correct scanner selected | Unit |
| 7 | Perf: BLAKE3 1GB | 1GB file | < 1s | Benchmark |
| 8 | Perf: treemap 1M | 1M nodes | < 100ms | Benchmark |
| 9 | Happy: undo | Delete → undo | File restored | Integration |

---

# PART V: IMPLEMENTATION ORDER (gstack-eng-mode)

```
Phase 3 (Win) ──┐
Phase 4 (Mac) ──┤── Phase 6 (Unified) → Phase 7 (Hash) ──┬── Phase 8 (Disk Intel)
Phase 5 (Lin) ──┘                                         └── Phase 9 (CQRS)
                                                                    │
                                                              Phase 10 (Watch)
                                                                    │
                                                              Phase 11 (Desktop UI)
                                                                    │
                                                           ═══ v1.0 CUT ═══
                                                                    │
                                                              Phase 12 (Crypto)
                                                                    │
                                                              Phase 13 (P2P)
                                                                   ╱ ╲
                                                            Ph14  Ph15 (Sync)
                                                            (Xfer)    │
                                                               ╲    Ph15.5
                                                                ╲  (Cloud)
                                                                 ╲ ╱
                                                             Phase 16 (Mobile)
                                                                    │
                                                           ═══ v2.0 CUT ═══
                                                                    │
                                                              Phase 17 (Media)
                                                                    │
                                                              Phase 18 (WASM)
                                                                    │
                                                              Phase 19 (Search)
                                                                    │
                                                              Phase 20 (Ext)
                                                                    │
                                                             Phase 20.5 (Int)
                                                                    │
                                                              Phase 21 (Ship)
```

---

# APPENDIX: Verification Checklist (Pre-Phase 3)

Run this before starting Phase 3:

```bash
cd D:/HyprDrive/.claude/worktrees/nervous-khorana

# 1. Compilation
cargo check --workspace

# 2. Lints
cargo clippy --workspace -- -D warnings

# 3. Tests
cargo test --workspace

# 4. Benchmarks
cargo bench --bench benchmarks

# 5. Verify branch name in CI matches repo default
git -C D:/HyprDrive remote show origin | grep "HEAD branch"

# 6. Verify spike/ is deleted
test ! -d D:/HyprDrive/spike && echo "OK: spike deleted"

# 7. Verify plan crate name fixed
grep "usn-journal-rs" "D:/HyprDrive/Implementation Plan" && echo "OK: crate name fixed"
```

All checks must pass before Phase 3 implementation begins.
