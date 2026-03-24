-- Migration 011: Cursor store for USN journal / inotify position tracking.
-- Persists watcher cursors so the daemon can resume from where it left off
-- after a restart, avoiding full rescans.
CREATE TABLE IF NOT EXISTS cursor_store (
    volume_key  TEXT PRIMARY KEY NOT NULL,
    cursor_json TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
