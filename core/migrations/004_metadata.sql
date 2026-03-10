-- Migration 004: Metadata key-value store with namespace

CREATE TABLE IF NOT EXISTS metadata (
    object_id   TEXT NOT NULL REFERENCES objects(id) ON DELETE CASCADE,
    namespace   TEXT NOT NULL,  -- "exif", "xmp", "custom", "system"
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    PRIMARY KEY (object_id, namespace, key)
);

CREATE INDEX idx_meta_ns ON metadata(namespace);
