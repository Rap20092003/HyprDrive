-- Migration 007: Sync operations for distributed sync

CREATE TABLE IF NOT EXISTS sync_operations (
    id          TEXT PRIMARY KEY NOT NULL,   -- ULID
    device_id   TEXT NOT NULL,
    entity_type TEXT NOT NULL,               -- "object", "location", "tag"
    entity_id   TEXT NOT NULL,
    action      TEXT NOT NULL,               -- "create", "update", "delete"
    payload     TEXT,                        -- JSON diff
    clock_json  TEXT NOT NULL,               -- Serialized VectorClock
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Delta sync: fetch operations from a device after a point
CREATE INDEX idx_sync_device ON sync_operations(device_id, id);
CREATE INDEX idx_sync_entity ON sync_operations(entity_type, entity_id);
