-- Migration 001: Objects table
-- Stores content identity (hash-addressed). Same content = same ObjectId.

CREATE TABLE IF NOT EXISTS objects (
    id          TEXT PRIMARY KEY NOT NULL,  -- ObjectId (BLAKE3 hash)
    kind        TEXT NOT NULL DEFAULT 'File',  -- ObjectKind: File, Directory, Symlink
    mime_type   TEXT,
    size_bytes  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
