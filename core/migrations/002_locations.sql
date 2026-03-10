-- Migration 002: Locations table + indexes
-- Stores WHERE content lives. Same content at 2 paths = 1 object + 2 locations.

CREATE TABLE IF NOT EXISTS locations (
    id              TEXT PRIMARY KEY NOT NULL,  -- LocationId (UUID)
    object_id       TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    volume_id       TEXT NOT NULL,              -- VolumeId (UUID)
    path            TEXT NOT NULL,
    name            TEXT NOT NULL,
    extension       TEXT,
    parent_id       TEXT REFERENCES locations(id) ON DELETE SET NULL,
    is_directory    INTEGER NOT NULL DEFAULT 0,
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    allocated_bytes INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    modified_at     TEXT NOT NULL,
    accessed_at     TEXT,
    UNIQUE(volume_id, path)
);

-- Used by list_files_fast: keyset pagination by (parent_id, name)
CREATE INDEX idx_loc_parent ON locations(parent_id);
CREATE INDEX idx_loc_sort ON locations(parent_id, name);
CREATE INDEX idx_loc_object ON locations(object_id);
CREATE INDEX idx_loc_accessed ON locations(accessed_at);
