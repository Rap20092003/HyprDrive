-- Migration 010: Add file reference number (fid) to locations table.
-- NTFS File Reference Number (FRN) / Unix inode for O(1) change event lookups.
-- USN journal delete/move events carry only fid, not path.
ALTER TABLE locations ADD COLUMN fid INTEGER;
CREATE INDEX idx_loc_fid ON locations(volume_id, fid);
