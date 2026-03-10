-- Migration 009: FTS5 virtual table for full-text search on file names

CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
    name,
    path,
    extension,
    content='locations',
    content_rowid='rowid'
);

-- Triggers to keep FTS in sync with locations table
CREATE TRIGGER IF NOT EXISTS locations_ai AFTER INSERT ON locations BEGIN
    INSERT INTO files_fts(rowid, name, path, extension)
    VALUES (new.rowid, new.name, new.path, new.extension);
END;

CREATE TRIGGER IF NOT EXISTS locations_ad AFTER DELETE ON locations BEGIN
    INSERT INTO files_fts(files_fts, rowid, name, path, extension)
    VALUES ('delete', old.rowid, old.name, old.path, old.extension);
END;

CREATE TRIGGER IF NOT EXISTS locations_au AFTER UPDATE ON locations BEGIN
    INSERT INTO files_fts(files_fts, rowid, name, path, extension)
    VALUES ('delete', old.rowid, old.name, old.path, old.extension);
    INSERT INTO files_fts(rowid, name, path, extension)
    VALUES (new.rowid, new.name, new.path, new.extension);
END;
