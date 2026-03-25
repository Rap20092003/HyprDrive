-- Migration 012: Add hash_state to objects for deferred content hashing.
-- 'content' = real BLAKE3 hash, 'deferred' = synthetic placeholder.

ALTER TABLE objects ADD COLUMN hash_state TEXT NOT NULL DEFAULT 'content';

-- Partial index: only deferred entries are queried by the background hasher.
-- Content-addressed entries (99%+) don't bloat the index.
CREATE INDEX idx_objects_deferred ON objects(hash_state) WHERE hash_state = 'deferred';
