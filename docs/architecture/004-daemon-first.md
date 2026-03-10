# ADR-004: Daemon-First Architecture

## Status
Accepted

## Context
File management applications can be structured in two ways:
1. **Library-first**: Core logic embedded in each UI (Tauri, mobile, web). Each app is self-contained.
2. **Daemon-first**: A background process owns all logic. UIs are thin clients that connect via IPC.

Spacedrive evolved from library-first toward daemon-first (their CLI now starts `sd-daemon` and Tauri connects to it). HyprDrive makes this the foundational decision from day one.

## Decision
**`hyprdrive-daemon` is THE system.** All UIs (desktop, web, mobile, CLI) are thin clients that connect via WebSocket or socket.

## Consequences

### Positive
- **Single source of truth**: One process owns the database. No lock contention between multiple apps.
- **Background processing**: Indexing, syncing, and media processing continue when UI is closed.
- **Multi-client**: Open the web UI and desktop app simultaneously — both see the same state.
- **Simpler testing**: Test the daemon in isolation without any UI framework.
- **Resource efficiency**: One database connection pool, one Iroh node, one event bus — not duplicated per app.
- **Crash isolation**: If the Tauri window crashes, the daemon (and your indexing job) keeps running.

### Negative
- **Installation complexity**: Users must have the daemon running. Mitigated: Tauri auto-starts daemon on launch, and a system service handles persistence.
- **Mobile exception**: Phones can't run reliable background daemons (iOS kills them). Mobile embeds `hyprdrive-core` in-process and connects to desktop daemons as a peer.
- **IPC overhead**: Every UI interaction crosses a process boundary (WebSocket/socket). Mitigated: binary serialization (MessagePack) and batched events keep latency < 1ms.

### Neutral
- Daemon port `:7420` for WebSocket, `:7421` for HTTP API, `:7422` for Prometheus metrics.
- CLI connects via same mechanism as Tauri — zero special handling.

## References
- [Syncthing uses daemon architecture](https://docs.syncthing.net/)
- [Spacedrive moving to daemon-first](https://github.com/spacedriveapp/spacedrive)
