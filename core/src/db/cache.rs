#![allow(clippy::result_large_err)]
//! redb caches for hot-path lookups.
//!
//! Six typed caches per the plan (§2.4 + §3.5):
//! 1. INODE_CACHE: (vol, inode, mtime) → object_id
//! 2. THUMB_MANIFEST: object_id → ThumbRecord
//! 3. QUERY_CACHE: query_hash → serialized result
//! 4. XFER_CHECKPOINTS: transfer_id → checkpoint
//! 5. DIR_SIZE_CACHE: location_id → DirSizeRecord
//! 6. USN_CURSORS: volume_key → JSON UsnCursor (Phase 3.5)

use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ═══ Table Definitions ═══

/// Inode cache: key = "vol:inode:mtime", value = object_id
const INODE_CACHE: TableDefinition<&str, &str> = TableDefinition::new("inode_cache");

/// Thumbnail manifest: key = object_id, value = JSON ThumbRecord
const THUMB_MANIFEST: TableDefinition<&str, &str> = TableDefinition::new("thumb_manifest");

/// Query cache: key = query_hash, value = serialized result
/// Reserved for Phase 9 (CQRS query caching).
const _QUERY_CACHE: TableDefinition<&str, &[u8]> = TableDefinition::new("query_cache");

/// Transfer checkpoints: key = transfer_id, value = JSON checkpoint
const XFER_CHECKPOINTS: TableDefinition<&str, &str> = TableDefinition::new("xfer_checkpoints");

/// Directory size cache: key = location_id, value = JSON DirSizeRecord
const DIR_SIZE_CACHE: TableDefinition<&str, &str> = TableDefinition::new("dir_size_cache");

/// USN journal cursor cache: key = volume letter (e.g. "C"), value = JSON UsnCursor
const USN_CURSORS: TableDefinition<&str, &str> = TableDefinition::new("usn_cursors");

// ═══ Types ═══

/// Thumbnail record stored in the manifest cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbRecord {
    /// Path to the thumbnail file
    pub path: String,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Size in bytes
    pub size: u64,
}

/// Directory size record stored in the cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirSizeRecord {
    /// Number of files
    pub file_count: u64,
    /// Total bytes
    pub total_bytes: u64,
    /// Cumulative allocated bytes
    pub cumulative_allocated: u64,
}

// ═══ Cache Operations ═══

/// Open or create the redb cache database.
pub fn open_cache(cache_path: &Path) -> Result<Database, redb::DatabaseError> {
    Database::create(cache_path)
}

/// Inode cache operations.
pub mod inode {
    use super::*;

    /// Build a cache key from volume, inode, and mtime.
    pub fn cache_key(volume_id: &str, inode: u64, mtime: i64) -> String {
        format!("{volume_id}:{inode}:{mtime}")
    }

    /// Build a cache key from volume, inode, mtime, AND size.
    ///
    /// Stronger invalidation than [`cache_key`]: detects in-place overwrites
    /// where mtime is preserved but size changes (e.g. truncation).
    /// Uses fixed-width numeric encoding to prevent ambiguity with
    /// volume IDs containing `:`.
    pub fn cache_key_v2(volume_id: &str, inode: u64, mtime: i64, size: u64) -> String {
        // Length-prefix the variable-length volume_id, then use fixed-width hex for numerics.
        format!(
            "v2:{}:{}:{:016x}:{:016x}:{:016x}",
            volume_id.len(),
            volume_id,
            inode,
            mtime as u64,
            size
        )
    }

    /// Insert an entry into the inode cache.
    pub fn insert(db: &Database, key: &str, object_id: &str) -> Result<(), redb::Error> {
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(INODE_CACHE)?;
            table.insert(key, object_id)?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Look up an object_id by inode cache key.
    pub fn get(db: &Database, key: &str) -> Result<Option<String>, redb::Error> {
        let txn = db.begin_read()?;
        match txn.open_table(INODE_CACHE) {
            Ok(table) => {
                let result = table.get(key)?;
                Ok(result.map(|v| v.value().to_string()))
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Look up multiple keys in a single read transaction.
    ///
    /// Returns results in the same order as input keys.
    /// Missing keys return `None` in their position.
    /// Much more efficient than calling [`get`] in a loop because
    /// it amortises the transaction overhead across all lookups.
    pub fn get_batch(db: &Database, keys: &[&str]) -> Result<Vec<Option<String>>, redb::Error> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let txn = db.begin_read()?;
        match txn.open_table(INODE_CACHE) {
            Ok(table) => {
                let mut results = Vec::with_capacity(keys.len());
                for key in keys {
                    let value = table.get(*key)?.map(|v| v.value().to_string());
                    results.push(value);
                }
                Ok(results)
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(vec![None; keys.len()]),
            Err(e) => Err(e.into()),
        }
    }

    /// Insert multiple entries in a single write transaction.
    ///
    /// Much more efficient than calling [`insert`] in a loop because
    /// it amortises the transaction + fsync overhead across all writes.
    pub fn insert_batch(db: &Database, entries: &[(&str, &str)]) -> Result<(), redb::Error> {
        if entries.is_empty() {
            return Ok(());
        }
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(INODE_CACHE)?;
            for (key, object_id) in entries {
                table.insert(*key, *object_id)?;
            }
        }
        txn.commit()?;
        Ok(())
    }
}

/// Thumbnail manifest operations.
pub mod thumb {
    use super::*;

    /// Insert a thumb record.
    pub fn insert(db: &Database, object_id: &str, record: &ThumbRecord) -> Result<(), redb::Error> {
        let json = serde_json::to_string(record).map_err(|e| {
            redb::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(THUMB_MANIFEST)?;
            table.insert(object_id, json.as_str())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Look up a thumb record.
    pub fn get(db: &Database, object_id: &str) -> Result<Option<ThumbRecord>, redb::Error> {
        let txn = db.begin_read()?;
        match txn.open_table(THUMB_MANIFEST) {
            Ok(table) => match table.get(object_id)? {
                Some(v) => {
                    let record: ThumbRecord = serde_json::from_str(v.value()).map_err(|e| {
                        redb::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                    })?;
                    Ok(Some(record))
                }
                None => Ok(None),
            },
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

/// Transfer checkpoint operations.
pub mod xfer {
    use super::*;

    /// Insert or update a checkpoint.
    pub fn upsert(
        db: &Database,
        transfer_id: &str,
        checkpoint_json: &str,
    ) -> Result<(), redb::Error> {
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(XFER_CHECKPOINTS)?;
            table.insert(transfer_id, checkpoint_json)?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Get a checkpoint.
    pub fn get(db: &Database, transfer_id: &str) -> Result<Option<String>, redb::Error> {
        let txn = db.begin_read()?;
        match txn.open_table(XFER_CHECKPOINTS) {
            Ok(table) => Ok(table.get(transfer_id)?.map(|v| v.value().to_string())),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a checkpoint.
    pub fn delete(db: &Database, transfer_id: &str) -> Result<(), redb::Error> {
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(XFER_CHECKPOINTS)?;
            table.remove(transfer_id)?;
        }
        txn.commit()?;
        Ok(())
    }
}

/// USN journal cursor persistence for real-time monitoring.
///
/// Stores the last-known USN journal cursor per volume so the listener
/// can resume after daemon restart without missing filesystem changes.
pub mod cursor {
    use super::*;

    /// USN journal cursor for tracking delta position.
    ///
    /// Stored as JSON in redb, keyed by volume letter (e.g. "C").
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub struct UsnCursorRecord {
        /// The USN journal ID — changes if journal is deleted/recreated.
        pub journal_id: u64,
        /// The next USN to read from.
        pub next_usn: i64,
    }

    /// Save a USN cursor for a volume.
    #[tracing::instrument(skip(db), fields(volume_key))]
    pub fn save(
        db: &Database,
        volume_key: &str,
        record: &UsnCursorRecord,
    ) -> Result<(), redb::Error> {
        let json = serde_json::to_string(record).map_err(|e| {
            redb::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(USN_CURSORS)?;
            table.insert(volume_key, json.as_str())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Load a USN cursor for a volume. Returns `None` if not found.
    #[tracing::instrument(skip(db), fields(volume_key))]
    pub fn load(db: &Database, volume_key: &str) -> Result<Option<UsnCursorRecord>, redb::Error> {
        let txn = db.begin_read()?;
        match txn.open_table(USN_CURSORS) {
            Ok(table) => match table.get(volume_key)? {
                Some(v) => {
                    let record: UsnCursorRecord = serde_json::from_str(v.value()).map_err(|e| {
                        redb::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                    })?;
                    Ok(Some(record))
                }
                None => Ok(None),
            },
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete a USN cursor for a volume.
    #[tracing::instrument(skip(db), fields(volume_key))]
    pub fn delete(db: &Database, volume_key: &str) -> Result<(), redb::Error> {
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(USN_CURSORS)?;
            table.remove(volume_key)?;
        }
        txn.commit()?;
        Ok(())
    }
}

/// Directory size cache operations.
pub mod dir_size {
    use super::*;

    /// Insert or update a directory size record.
    pub fn upsert(
        db: &Database,
        location_id: &str,
        record: &DirSizeRecord,
    ) -> Result<(), redb::Error> {
        let json = serde_json::to_string(record).map_err(|e| {
            redb::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        })?;
        let txn = db.begin_write()?;
        {
            let mut table = txn.open_table(DIR_SIZE_CACHE)?;
            table.insert(location_id, json.as_str())?;
        }
        txn.commit()?;
        Ok(())
    }

    /// Get a directory size record.
    pub fn get(db: &Database, location_id: &str) -> Result<Option<DirSizeRecord>, redb::Error> {
        let txn = db.begin_read()?;
        match txn.open_table(DIR_SIZE_CACHE) {
            Ok(table) => match table.get(location_id)? {
                Some(v) => {
                    let record: DirSizeRecord = serde_json::from_str(v.value()).map_err(|e| {
                        redb::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                    })?;
                    Ok(Some(record))
                }
                None => Ok(None),
            },
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (Database, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let db = Database::create(dir.path().join("cache.redb")).expect("create db");
        (db, dir)
    }

    // ═══ Inode Cache Tests ═══

    #[test]
    fn test_inode_insert_and_hit() {
        let (db, _dir) = test_db();
        let key = inode::cache_key("vol1", 12345, 1700000000);
        inode::insert(&db, &key, "obj_abc123").expect("insert");
        let result = inode::get(&db, &key).expect("get");
        assert_eq!(result.as_deref(), Some("obj_abc123"));
    }

    #[test]
    fn test_inode_miss() {
        let (db, _dir) = test_db();
        let result = inode::get(&db, "nonexistent").expect("get");
        assert_eq!(result, None);
    }

    #[test]
    fn test_inode_overwrite() {
        let (db, _dir) = test_db();
        let key = inode::cache_key("vol1", 12345, 1700000000);
        inode::insert(&db, &key, "obj_old").expect("insert");
        inode::insert(&db, &key, "obj_new").expect("overwrite");
        let result = inode::get(&db, &key).expect("get");
        assert_eq!(result.as_deref(), Some("obj_new"));
    }

    #[test]
    fn test_inode_get_batch_all_hits() {
        let (db, _dir) = test_db();
        inode::insert(&db, "a", "1").unwrap();
        inode::insert(&db, "b", "2").unwrap();
        inode::insert(&db, "c", "3").unwrap();
        let results = inode::get_batch(&db, &["a", "b", "c"]).unwrap();
        assert_eq!(
            results,
            vec![Some("1".into()), Some("2".into()), Some("3".into())]
        );
    }

    #[test]
    fn test_inode_get_batch_mixed_hits_misses() {
        let (db, _dir) = test_db();
        inode::insert(&db, "x", "val").unwrap();
        let results = inode::get_batch(&db, &["x", "missing", "also_missing"]).unwrap();
        assert_eq!(results, vec![Some("val".into()), None, None]);
    }

    #[test]
    fn test_inode_get_batch_empty() {
        let (db, _dir) = test_db();
        let results = inode::get_batch(&db, &[]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_inode_get_batch_no_table_yet() {
        // Fresh DB with no writes — table doesn't exist
        let (db, _dir) = test_db();
        let results = inode::get_batch(&db, &["a", "b"]).unwrap();
        assert_eq!(results, vec![None, None]);
    }

    // ═══ Inode Cache v2 Key Tests ═══

    #[test]
    fn test_cache_key_v2_format() {
        let key = inode::cache_key_v2("vol1", 12345, 1700000000, 4096);
        // Length-prefixed volume_id + fixed-width hex for numerics.
        assert_eq!(
            key,
            "v2:4:vol1:0000000000003039:000000006553f100:0000000000001000"
        );
    }

    #[test]
    fn test_cache_key_v2_different_size_different_key() {
        let k1 = inode::cache_key_v2("vol1", 100, 999, 4096);
        let k2 = inode::cache_key_v2("vol1", 100, 999, 8192);
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_inode_insert_batch_empty() {
        let (db, _dir) = test_db();
        inode::insert_batch(&db, &[]).unwrap();
        // No crash, no-op
    }

    #[test]
    fn test_inode_insert_batch_multiple() {
        let (db, _dir) = test_db();
        let entries = vec![("k1", "obj1"), ("k2", "obj2"), ("k3", "obj3")];
        inode::insert_batch(&db, &entries).unwrap();

        assert_eq!(inode::get(&db, "k1").unwrap().as_deref(), Some("obj1"));
        assert_eq!(inode::get(&db, "k2").unwrap().as_deref(), Some("obj2"));
        assert_eq!(inode::get(&db, "k3").unwrap().as_deref(), Some("obj3"));
    }

    #[test]
    fn test_inode_insert_batch_overwrites() {
        let (db, _dir) = test_db();
        inode::insert(&db, "k1", "old_value").unwrap();
        inode::insert_batch(&db, &[("k1", "new_value"), ("k2", "v2")]).unwrap();
        assert_eq!(inode::get(&db, "k1").unwrap().as_deref(), Some("new_value"));
        assert_eq!(inode::get(&db, "k2").unwrap().as_deref(), Some("v2"));
    }

    // ═══ Thumb Manifest Tests ═══

    #[test]
    fn test_thumb_insert_and_lookup() {
        let (db, _dir) = test_db();
        let record = ThumbRecord {
            path: "/thumbs/abc.webp".to_string(),
            width: 256,
            height: 256,
            size: 8192,
        };
        thumb::insert(&db, "obj1", &record).expect("insert");
        let result = thumb::get(&db, "obj1").expect("get");
        assert!(result.is_some());
        let r = result.expect("some");
        assert_eq!(r.width, 256);
        assert_eq!(r.path, "/thumbs/abc.webp");
    }

    #[test]
    fn test_thumb_miss() {
        let (db, _dir) = test_db();
        let result = thumb::get(&db, "nonexistent").expect("get");
        assert!(result.is_none());
    }

    // ═══ Transfer Checkpoint Tests ═══

    #[test]
    fn test_xfer_insert_and_get() {
        let (db, _dir) = test_db();
        xfer::upsert(&db, "xfer1", r#"{"chunks_done": [0,1,2]}"#).expect("upsert");
        let result = xfer::get(&db, "xfer1").expect("get");
        assert!(result.is_some());
        assert!(result.expect("some").contains("chunks_done"));
    }

    #[test]
    fn test_xfer_update() {
        let (db, _dir) = test_db();
        xfer::upsert(&db, "xfer1", r#"{"chunks_done": [0]}"#).expect("insert");
        xfer::upsert(&db, "xfer1", r#"{"chunks_done": [0,1,2]}"#).expect("update");
        let result = xfer::get(&db, "xfer1").expect("get").expect("some");
        assert!(result.contains("[0,1,2]"));
    }

    #[test]
    fn test_xfer_delete() {
        let (db, _dir) = test_db();
        xfer::upsert(&db, "xfer1", "{}").expect("insert");
        xfer::delete(&db, "xfer1").expect("delete");
        let result = xfer::get(&db, "xfer1").expect("get");
        assert!(result.is_none());
    }

    // ═══ Dir Size Cache Tests ═══

    #[test]
    fn test_dir_size_insert_and_get() {
        let (db, _dir) = test_db();
        let record = DirSizeRecord {
            file_count: 42,
            total_bytes: 1024 * 1024,
            cumulative_allocated: 2 * 1024 * 1024,
        };
        dir_size::upsert(&db, "loc1", &record).expect("upsert");
        let result = dir_size::get(&db, "loc1").expect("get").expect("some");
        assert_eq!(result.file_count, 42);
        assert_eq!(result.cumulative_allocated, 2 * 1024 * 1024);
    }

    // ═══ USN Cursor Tests ═══

    #[test]
    fn test_cursor_save_and_load() {
        let (db, _dir) = test_db();
        let record = cursor::UsnCursorRecord {
            journal_id: 12345,
            next_usn: 67890,
        };
        cursor::save(&db, "C", &record).expect("save");
        let loaded = cursor::load(&db, "C").expect("load");
        assert_eq!(loaded, Some(record));
    }

    #[test]
    fn test_cursor_miss() {
        let (db, _dir) = test_db();
        let result = cursor::load(&db, "Z").expect("load");
        assert_eq!(result, None);
    }

    #[test]
    fn test_cursor_overwrite() {
        let (db, _dir) = test_db();
        let r1 = cursor::UsnCursorRecord {
            journal_id: 100,
            next_usn: 200,
        };
        cursor::save(&db, "C", &r1).expect("save");

        let r2 = cursor::UsnCursorRecord {
            journal_id: 100,
            next_usn: 500,
        };
        cursor::save(&db, "C", &r2).expect("overwrite");

        let loaded = cursor::load(&db, "C").expect("load");
        assert_eq!(loaded, Some(r2));
    }

    #[test]
    fn test_cursor_delete() {
        let (db, _dir) = test_db();
        let record = cursor::UsnCursorRecord {
            journal_id: 1,
            next_usn: 2,
        };
        cursor::save(&db, "D", &record).expect("save");
        cursor::delete(&db, "D").expect("delete");
        let result = cursor::load(&db, "D").expect("load");
        assert_eq!(result, None);
    }

    #[test]
    fn test_cursor_multiple_volumes() {
        let (db, _dir) = test_db();
        let c_cursor = cursor::UsnCursorRecord {
            journal_id: 1,
            next_usn: 100,
        };
        let d_cursor = cursor::UsnCursorRecord {
            journal_id: 2,
            next_usn: 200,
        };
        cursor::save(&db, "C", &c_cursor).expect("save C");
        cursor::save(&db, "D", &d_cursor).expect("save D");

        assert_eq!(cursor::load(&db, "C").expect("load"), Some(c_cursor));
        assert_eq!(cursor::load(&db, "D").expect("load"), Some(d_cursor));
    }

    #[test]
    fn test_dir_size_update_cumulative() {
        let (db, _dir) = test_db();
        let r1 = DirSizeRecord {
            file_count: 10,
            total_bytes: 1000,
            cumulative_allocated: 2000,
        };
        dir_size::upsert(&db, "loc1", &r1).expect("insert");

        let r2 = DirSizeRecord {
            file_count: 20,
            total_bytes: 2000,
            cumulative_allocated: 5000,
        };
        dir_size::upsert(&db, "loc1", &r2).expect("update");

        let result = dir_size::get(&db, "loc1").expect("get").expect("some");
        assert_eq!(result.file_count, 20);
        assert_eq!(result.cumulative_allocated, 5000);
    }
}
