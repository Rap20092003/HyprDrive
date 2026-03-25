-- Migration 013: Add CHECK constraint on hash_state.
-- Prevents invalid values from entering the objects table.
-- SQLite does not support ALTER TABLE ADD CONSTRAINT, so we recreate
-- the column with a CHECK. However, ALTER TABLE ADD COLUMN ... CHECK
-- is also not supported on existing columns. Instead, we add a trigger
-- to enforce the constraint on INSERT and UPDATE.

CREATE TRIGGER IF NOT EXISTS trg_objects_hash_state_insert
BEFORE INSERT ON objects
FOR EACH ROW
WHEN NEW.hash_state NOT IN ('content', 'deferred')
BEGIN
    SELECT RAISE(ABORT, 'hash_state must be content or deferred');
END;

CREATE TRIGGER IF NOT EXISTS trg_objects_hash_state_update
BEFORE UPDATE OF hash_state ON objects
FOR EACH ROW
WHEN NEW.hash_state NOT IN ('content', 'deferred')
BEGIN
    SELECT RAISE(ABORT, 'hash_state must be content or deferred');
END;
