//! Linux filesystem type detection via /proc/mounts and statfs().
//!
//! Detects the filesystem type for a given path by:
//! 1. Parsing `/proc/mounts` for mount entries
//! 2. Finding the longest-prefix mount match for the target path
//! 3. Mapping the filesystem type string to [`FilesystemKind`]
//! 4. Optionally checking `statfs()` for pseudo-filesystem detection

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::FilesystemKind;
use std::path::{Path, PathBuf};

/// Information about a mounted filesystem.
#[derive(Debug, Clone)]
pub struct MountInfo {
    /// Block device path (e.g., "/dev/sda1").
    pub device: String,
    /// Mount point path (e.g., "/home").
    pub mount_point: PathBuf,
    /// Filesystem type string (e.g., "ext4").
    pub fs_type: String,
    /// Mount options (e.g., "rw,relatime").
    pub options: String,
}

/// Known pseudo-filesystem magic numbers from Linux kernel headers.
/// These filesystems should not be indexed.
const PROC_SUPER_MAGIC: i64 = 0x9fa0;
const SYSFS_MAGIC: i64 = 0x6265_6572;
const DEBUGFS_MAGIC: i64 = 0x6462_6720;
const DEVPTS_SUPER_MAGIC: i64 = 0x1cd1;
const SECURITYFS_MAGIC: i64 = 0x7363_6673;
const CGROUP_SUPER_MAGIC: i64 = 0x0027_e0eb;
const CGROUP2_SUPER_MAGIC: i64 = 0x6367_7270;
const TRACEFS_MAGIC: i64 = 0x7472_6163;
const HUGETLBFS_MAGIC: i64 = 0x9584_58f6_u32 as i64;
const BINFMTFS_MAGIC: i64 = 0x4249_4e4d;
const PSTOREFS_MAGIC: i64 = 0x6165_676C;

/// Parse mount entries from /proc/mounts content string.
///
/// Each line has the format: `device mountpoint fstype options dump pass`
fn parse_mounts(content: &str) -> Vec<MountInfo> {
    content
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                Some(MountInfo {
                    device: parts[0].to_string(),
                    mount_point: PathBuf::from(parts[1]),
                    fs_type: parts[2].to_string(),
                    options: parts[3].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Find the mount entry whose mount_point is the longest prefix of `path`.
///
/// This handles nested mounts correctly — e.g., `/home/user` matches
/// `/home` over `/` because `/home` is a longer prefix.
fn find_mount_for_path<'a>(path: &Path, mounts: &'a [MountInfo]) -> Option<&'a MountInfo> {
    mounts
        .iter()
        .filter(|m| path.starts_with(&m.mount_point))
        .max_by_key(|m| m.mount_point.as_os_str().len())
}

/// Map a Linux filesystem type string to [`FilesystemKind`].
fn map_fstype(fs_type: &str) -> FilesystemKind {
    match fs_type {
        "ext4" | "ext3" | "ext2" => FilesystemKind::Ext4,
        "btrfs" => FilesystemKind::Btrfs,
        "xfs" => FilesystemKind::Xfs,
        "zfs" => FilesystemKind::Zfs,
        "tmpfs" | "devtmpfs" => FilesystemKind::Tmpfs,
        "9p" => FilesystemKind::NineP,
        "nfs" | "nfs4" => FilesystemKind::Nfs,
        "overlay" => FilesystemKind::OverlayFs,
        "ntfs" | "ntfs3" => FilesystemKind::Ntfs,
        "vfat" => FilesystemKind::Fat32,
        "exfat" => FilesystemKind::ExFat,
        other if other.starts_with("fuse") => FilesystemKind::Fuse,
        _ => FilesystemKind::Unknown,
    }
}

/// Check if a path is on a pseudo-filesystem that should not be indexed.
///
/// Uses `statfs()` to check the filesystem magic number against known
/// pseudo-filesystem types (proc, sysfs, debugfs, etc.).
#[tracing::instrument(fields(path = %path.display()))]
pub fn is_pseudo_filesystem(path: &Path) -> bool {
    match nix::sys::statfs::statfs(path) {
        Ok(stat) => {
            let magic = stat.filesystem_type().0;
            matches!(
                magic,
                PROC_SUPER_MAGIC
                    | SYSFS_MAGIC
                    | DEBUGFS_MAGIC
                    | DEVPTS_SUPER_MAGIC
                    | SECURITYFS_MAGIC
                    | CGROUP_SUPER_MAGIC
                    | CGROUP2_SUPER_MAGIC
                    | TRACEFS_MAGIC
                    | HUGETLBFS_MAGIC
                    | BINFMTFS_MAGIC
                    | PSTOREFS_MAGIC
            )
        }
        Err(_) => false,
    }
}

/// Detect the filesystem type for a given path.
///
/// Reads `/proc/mounts`, finds the mount entry for the path,
/// and maps the filesystem type string to [`FilesystemKind`].
///
/// Returns [`FsIndexerError::PseudoFilesystem`] if the path is on a
/// pseudo-filesystem (proc, sysfs, etc.).
#[tracing::instrument(fields(path = %path.display()))]
pub fn detect_filesystem(path: &Path) -> FsIndexerResult<FilesystemKind> {
    let content = std::fs::read_to_string("/proc/mounts").map_err(|e| {
        FsIndexerError::DetectionFailed {
            volume: path.display().to_string(),
            source: e,
        }
    })?;

    let mounts = parse_mounts(&content);

    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    let mount = find_mount_for_path(&canonical, &mounts).ok_or_else(|| {
        FsIndexerError::DetectionFailed {
            volume: path.display().to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no mount entry found for path",
            ),
        }
    })?;

    let kind = map_fstype(&mount.fs_type);

    // Check for pseudo-filesystem via statfs magic
    if is_pseudo_filesystem(&canonical) {
        return Err(FsIndexerError::PseudoFilesystem {
            path: canonical.display().to_string(),
            fs_type: mount.fs_type.clone(),
        });
    }

    tracing::debug!(
        path = %path.display(),
        fs_type = %mount.fs_type,
        kind = ?kind,
        mount_point = %mount.mount_point.display(),
        "detected filesystem"
    );

    Ok(kind)
}

/// Get detailed mount information for a path.
///
/// Returns the [`MountInfo`] for the filesystem containing the given path.
#[tracing::instrument(fields(path = %path.display()))]
pub fn parse_mount_info(path: &Path) -> FsIndexerResult<MountInfo> {
    let content = std::fs::read_to_string("/proc/mounts").map_err(|e| {
        FsIndexerError::DetectionFailed {
            volume: path.display().to_string(),
            source: e,
        }
    })?;

    let mounts = parse_mounts(&content);
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    find_mount_for_path(&canonical, &mounts)
        .cloned()
        .ok_or_else(|| FsIndexerError::DetectionFailed {
            volume: path.display().to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no mount entry found for path",
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_PROC_MOUNTS: &str = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
/dev/sda1 / ext4 rw,relatime,errors=remount-ro 0 0
/dev/sda2 /home ext4 rw,relatime 0 0
tmpfs /tmp tmpfs rw,nosuid,nodev 0 0
C:\\ /mnt/c 9p rw,noatime,dirsync,aname=drvfs 0 0
overlay /var/lib/docker/overlay2/merged overlay rw,lowerdir=... 0 0";

    #[test]
    fn parse_mounts_returns_correct_count() {
        let mounts = parse_mounts(SAMPLE_PROC_MOUNTS);
        assert_eq!(mounts.len(), 7);
    }

    #[test]
    fn parse_mounts_extracts_fields_correctly() {
        let mounts = parse_mounts(SAMPLE_PROC_MOUNTS);
        let root_mount = mounts.iter().find(|m| m.mount_point == PathBuf::from("/"));
        assert!(root_mount.is_some());
        let root = root_mount.expect("checked above");
        assert_eq!(root.device, "/dev/sda1");
        assert_eq!(root.fs_type, "ext4");
        assert!(root.options.contains("relatime"));
    }

    #[test]
    fn find_mount_longest_prefix() {
        let mounts = parse_mounts(SAMPLE_PROC_MOUNTS);
        let path = PathBuf::from("/home/user/Documents");
        let found = find_mount_for_path(&path, &mounts);
        assert!(found.is_some());
        assert_eq!(
            found.expect("checked above").mount_point,
            PathBuf::from("/home")
        );
    }

    #[test]
    fn find_mount_root_fallback() {
        let mounts = parse_mounts(SAMPLE_PROC_MOUNTS);
        let path = PathBuf::from("/etc/config");
        let found = find_mount_for_path(&path, &mounts);
        assert!(found.is_some());
        assert_eq!(
            found.expect("checked above").mount_point,
            PathBuf::from("/")
        );
    }

    #[test]
    fn map_fstype_all_linux_variants() {
        assert_eq!(map_fstype("ext4"), FilesystemKind::Ext4);
        assert_eq!(map_fstype("ext3"), FilesystemKind::Ext4);
        assert_eq!(map_fstype("ext2"), FilesystemKind::Ext4);
        assert_eq!(map_fstype("btrfs"), FilesystemKind::Btrfs);
        assert_eq!(map_fstype("xfs"), FilesystemKind::Xfs);
        assert_eq!(map_fstype("zfs"), FilesystemKind::Zfs);
        assert_eq!(map_fstype("tmpfs"), FilesystemKind::Tmpfs);
        assert_eq!(map_fstype("9p"), FilesystemKind::NineP);
        assert_eq!(map_fstype("nfs"), FilesystemKind::Nfs);
        assert_eq!(map_fstype("nfs4"), FilesystemKind::Nfs);
        assert_eq!(map_fstype("overlay"), FilesystemKind::OverlayFs);
        assert_eq!(map_fstype("fuse.sshfs"), FilesystemKind::Fuse);
        assert_eq!(map_fstype("fuse"), FilesystemKind::Fuse);
        assert_eq!(map_fstype("ntfs"), FilesystemKind::Ntfs);
        assert_eq!(map_fstype("ntfs3"), FilesystemKind::Ntfs);
        assert_eq!(map_fstype("vfat"), FilesystemKind::Fat32);
        assert_eq!(map_fstype("exfat"), FilesystemKind::ExFat);
    }

    #[test]
    fn map_fstype_unknown() {
        assert_eq!(map_fstype("squashfs"), FilesystemKind::Unknown);
        assert_eq!(map_fstype(""), FilesystemKind::Unknown);
    }

    #[test]
    #[ignore] // Requires Linux — run in WSL2
    fn is_pseudo_fs_proc_and_sys() {
        assert!(is_pseudo_filesystem(Path::new("/proc")));
        assert!(is_pseudo_filesystem(Path::new("/sys")));
        assert!(!is_pseudo_filesystem(Path::new("/home")));
    }

    #[test]
    #[ignore] // Requires Linux — run in WSL2
    fn detect_filesystem_root() {
        let kind = detect_filesystem(Path::new("/"));
        assert!(kind.is_ok());
        // WSL2 default is ext4
        assert_eq!(kind.expect("checked above"), FilesystemKind::Ext4);
    }
}
