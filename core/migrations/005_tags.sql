-- Migration 005: Tags with closure table for hierarchical queries

CREATE TABLE IF NOT EXISTS tags (
    id          TEXT PRIMARY KEY NOT NULL,  -- TagId (UUID)
    name        TEXT NOT NULL,
    color       TEXT,                       -- Hex color e.g. "#FF5733"
    parent_id   TEXT REFERENCES tags(id) ON DELETE SET NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Many-to-many: tags on objects
CREATE TABLE IF NOT EXISTS tags_on_objects (
    tag_id    TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    object_id TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    PRIMARY KEY (tag_id, object_id)
);

CREATE INDEX idx_tag_obj ON tags_on_objects(object_id);

-- Closure table for ancestor/descendant queries
CREATE TABLE IF NOT EXISTS tag_closure (
    ancestor_id   TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    descendant_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    depth         INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (ancestor_id, descendant_id)
);

CREATE INDEX idx_tag_closure_desc ON tag_closure(descendant_id);
