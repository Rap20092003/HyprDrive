-- Migration 003: Directory sizes + allocation tracking

CREATE TABLE IF NOT EXISTS dir_sizes (
    location_id          TEXT PRIMARY KEY NOT NULL REFERENCES locations(id) ON DELETE CASCADE,
    file_count           INTEGER NOT NULL DEFAULT 0,
    total_bytes          INTEGER NOT NULL DEFAULT 0,
    allocated_bytes      INTEGER NOT NULL DEFAULT 0,
    cumulative_allocated INTEGER NOT NULL DEFAULT 0,
    updated_at           TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Disk intelligence: find largest directories by cumulative allocation
CREATE INDEX idx_dir_alloc ON dir_sizes(cumulative_allocated DESC);
