-- Migration 006: Virtual folders, temporal index, backlinks

CREATE TABLE IF NOT EXISTS virtual_folders (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL,
    filter_json TEXT NOT NULL,               -- Serialized FilterExpr
    icon        TEXT,
    color       TEXT,
    pinned      INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Temporal index for timeline views (date-based queries)
CREATE TABLE IF NOT EXISTS temporal_index (
    location_id TEXT NOT NULL REFERENCES locations(id) ON DELETE CASCADE,
    year        INTEGER NOT NULL,
    month       INTEGER NOT NULL,
    day         INTEGER NOT NULL,
    PRIMARY KEY (location_id)
);

CREATE INDEX idx_temporal_ymd ON temporal_index(year, month, day);

-- Backlinks: track which objects reference other objects
CREATE TABLE IF NOT EXISTS backlinks (
    source_object_id TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    target_object_id TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    link_type        TEXT NOT NULL DEFAULT 'reference',
    PRIMARY KEY (source_object_id, target_object_id)
);

CREATE INDEX idx_backlink_target ON backlinks(target_object_id);
