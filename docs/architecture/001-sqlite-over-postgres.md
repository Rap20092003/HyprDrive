# ADR-001: SQLite over PostgreSQL

## Status
Accepted

## Context
HyprDrive needs a database for file metadata, tags, sync operations, and search indexes. The choice is between SQLite (embedded) and PostgreSQL (server-based).

HyprDrive is a **local-first** application that runs on user devices (laptops, phones, NAS). Users should not need to install or manage a database server. The daemon must work offline with zero configuration.

## Decision
Use **SQLite** (via SeaORM) as the sole database engine.

## Consequences

### Positive
- **Zero install**: SQLite is compiled into the binary. No `pg_install` step.
- **Local-first**: Database file lives alongside the library. `cp library.db backup.db` is a backup.
- **Cross-platform**: Works identically on Windows, macOS, Linux, iOS, Android.
- **Single-writer WAL mode**: Provides concurrent readers with write serialization — matches daemon architecture perfectly (one daemon process writes).
- **Performance**: With proper indexing and WAL mode, SQLite handles millions of rows at microsecond latency. Our target is `list_files_fast(100k) < 5ms`.
- **Testability**: In-memory SQLite for unit tests — no Docker, no test database server.

### Negative
- **Single-writer limitation**: Only one process can write at a time. Mitigated by daemon-first architecture (single writer by design).
- **No built-in replication**: Sync must be implemented in application code (CRDT layer handles this).
- **Limited concurrent write throughput**: Not suitable for high-write workloads. Mitigated by batching writes and using redb for high-frequency caches.

### Neutral
- SeaORM provides type-safe query building similar to what we'd get with any ORM.
- Migration tooling works the same regardless of database backend.

## References
- [SQLite When to Use](https://www.sqlite.org/whentouse.html)
- [Spacedrive uses the same approach](https://github.com/spacedriveapp/spacedrive)
