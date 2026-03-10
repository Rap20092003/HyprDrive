# ADR-005: Tiered Resource Strategy — ML is Optional

- **Status**: Accepted
- **Date**: 2026-03-10
- **Decision**: All machine learning features are opt-in. The core daemon must run on constrained devices (≤50 MB RAM).

## Context

HyprDrive targets a wide range of devices — from Raspberry Pis and phones to high-end workstations. Immich (our ML reference) requires PostgreSQL + Redis + ML models = 2–4 GB RAM minimum, making it server-only.

We need to support powerful ML features (CLIP smart search, facial recognition, OCR) without forcing every device to pay the resource cost.

## Decision

### Three-Tier Resource Model

| Tier | What Runs | RAM Budget | Target Device |
|------|-----------|-----------|---------------|
| **Tier 1: Core** | Daemon, indexer, sync, search-by-name, tags, filters | **≤ 50 MB** | Raspberry Pi, phone, laptop |
| **Tier 2: Local ML** | CLIP embeddings, face detection, OCR (separate process) | **+ 500 MB** | Laptop plugged in, desktop |
| **Tier 3: Remote ML** | Offload ML to a home server/NAS, sync results back | **≈ 0 extra** | Phone, thin client |

### Rules

1. **Core must never depend on ML** — all ML features degrade gracefully (search falls back to FTS5 text search)
2. **ML runs as a separate process** — crashes or OOM in ML never take down the daemon
3. **ML is lazy** — models load on first use, not at startup
4. **ML is pausable** — user can pause/resume ML jobs when on battery or low resources
5. **Results are cached** — once an embedding is computed, it's stored in the database forever (no re-computation)
6. **Remote offload is transparent** — same API whether ML runs locally or on a server

### Resource Budgets (Enforced)

```
Core daemon idle:         ≤ 20 MB
Core daemon indexing:     ≤ 50 MB
Core + CLIP model:        ≤ 600 MB
Core + CLIP + faces:      ≤ 900 MB
SQLite per 100k files:    ≤ 10 MB
```

## Consequences

- **Pro**: Runs on any device — Pi, phone, laptop, server
- **Pro**: ML features available without mandatory heavyweight dependencies
- **Pro**: Users on constrained devices still get full file management
- **Con**: Smart search unavailable without ML opt-in (falls back to text search)
- **Con**: Remote ML requires a second device running the ML worker

## Alternatives Considered

1. **Always-on ML (Immich approach)** — Rejected: excludes low-power devices
2. **Cloud-only ML** — Rejected: violates local-first principle
3. **No ML at all** — Rejected: CLIP search is too valuable to skip
