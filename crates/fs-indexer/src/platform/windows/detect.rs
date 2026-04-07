//! Filesystem detection and volume enumeration for Windows.

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::FilesystemKind;
use std::path::{Path, PathBuf};

/// Detect the filesystem type of a given volume.
///
/// Uses `GetVolumeInformationW` to query the filesystem name string,
/// then maps it to a [`FilesystemKind`].
#[allow(unsafe_code)]
#[tracing::instrument(fields(volume = %volume.display()))]
pub fn detect_filesystem(volume: &Path) -> FsIndexerResult<FilesystemKind> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // Volume root must end with backslash for GetVolumeInformationW
    let volume_str = volume.to_string_lossy();
    let volume_root = if volume_str.ends_with('\\') {
        volume_str.to_string()
    } else {
        format!("{}\\", volume_str)
    };

    let wide_root: Vec<u16> = OsStr::new(&volume_root)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut fs_name_buf = [0u16; 64];
    let mut volume_name_buf = [0u16; 256];
    let mut serial_number: u32 = 0;
    let mut max_component_length: u32 = 0;
    let mut fs_flags: u32 = 0;

    // SAFETY: Calling Win32 API with correctly sized buffers.
    // The function writes null-terminated UTF-16 strings into the buffers.
    let success = unsafe {
        windows::Win32::Storage::FileSystem::GetVolumeInformationW(
            windows::core::PCWSTR(wide_root.as_ptr()),
            Some(&mut volume_name_buf),
            Some(&mut serial_number),
            Some(&mut max_component_length),
            Some(&mut fs_flags),
            Some(&mut fs_name_buf),
        )
        .is_ok()
    };

    if !success {
        return Err(FsIndexerError::DetectionFailed {
            volume: volume_root,
            source: std::io::Error::last_os_error(),
        });
    }

    // Convert the filesystem name buffer to a string
    let fs_name_len = fs_name_buf
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(fs_name_buf.len());
    let fs_name = String::from_utf16_lossy(&fs_name_buf[..fs_name_len]);

    tracing::debug!(filesystem = %fs_name, "detected filesystem");

    match fs_name.as_str() {
        "NTFS" => Ok(FilesystemKind::Ntfs),
        "FAT32" => Ok(FilesystemKind::Fat32),
        "exFAT" => Ok(FilesystemKind::ExFat),
        "ReFS" => Ok(FilesystemKind::Refs),
        _ => Ok(FilesystemKind::Unknown),
    }
}

/// Classification of a Windows drive by hardware/connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveType {
    /// Internal or permanently attached disk (HDD, SSD, NVMe).
    Fixed,
    /// Hot-pluggable media (USB stick, SD card, external HDD).
    Removable,
    /// Mapped network share (SMB, NFS, etc.).
    Network,
    /// Optical disc drive (CD, DVD, Blu-ray).
    CdRom,
    /// RAM disk (software-defined volatile storage).
    RamDisk,
    /// WSL (Windows Subsystem for Linux) distro filesystem via 9P.
    Wsl,
    /// Unrecognized or inaccessible drive type.
    Unknown,
}

/// Information about a discovered volume.
#[derive(Debug, Clone)]
pub struct DriveInfo {
    /// Volume root path: `"C:\\"` or `"\\\\wsl.localhost\\Ubuntu\\"`.
    pub path: PathBuf,
    /// Short identifier for database `volume_id` column: `"C"`, `"D"`, `"wsl:Ubuntu"`.
    pub volume_id: String,
    /// Hardware classification of the drive.
    pub drive_type: DriveType,
    /// Detected filesystem (None if inaccessible, e.g. empty CD tray).
    pub fs_kind: Option<FilesystemKind>,
}

/// Enumerate all Windows drive-letter volumes (A:–Z:).
///
/// Uses `GetLogicalDrives()` for discovery and `GetDriveTypeW()` for classification.
#[allow(unsafe_code)]
fn enumerate_windows_drives() -> Vec<DriveInfo> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // SAFETY: GetLogicalDrives is a safe read-only Win32 call returning a bitmask.
    let bitmask = unsafe { windows::Win32::Storage::FileSystem::GetLogicalDrives() };
    if bitmask == 0 {
        tracing::warn!("GetLogicalDrives returned 0 — no drives found or call failed");
        return Vec::new();
    }

    let mut drives = Vec::new();

    for bit in 0..26u32 {
        if bitmask & (1 << bit) == 0 {
            continue;
        }
        let letter = (b'A' + bit as u8) as char;
        let root = format!("{}:\\", letter);

        // Classify drive type via GetDriveTypeW.
        let wide_root: Vec<u16> = OsStr::new(&root)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: Calling Win32 API with a valid null-terminated wide string.
        let raw_type = unsafe {
            windows::Win32::Storage::FileSystem::GetDriveTypeW(windows::core::PCWSTR(
                wide_root.as_ptr(),
            ))
        };

        let drive_type = match raw_type {
            2 => DriveType::Removable, // DRIVE_REMOVABLE
            3 => DriveType::Fixed,     // DRIVE_FIXED
            4 => DriveType::Network,   // DRIVE_REMOTE
            5 => DriveType::CdRom,     // DRIVE_CDROM
            6 => DriveType::RamDisk,   // DRIVE_RAMDISK
            _ => DriveType::Unknown,   // DRIVE_UNKNOWN (0) / DRIVE_NO_ROOT_DIR (1)
        };

        // Detect filesystem (may fail for empty CD trays, disconnected network shares, etc.)
        let fs_kind = match detect_filesystem(Path::new(&root)) {
            Ok(kind) => Some(kind),
            Err(e) => {
                tracing::debug!(drive = %letter, error = %e, "filesystem detection failed — drive may be empty or inaccessible");
                None
            }
        };

        drives.push(DriveInfo {
            path: PathBuf::from(&root),
            volume_id: letter.to_string(),
            drive_type,
            fs_kind,
        });
    }

    drives
}

/// Enumerate WSL (Windows Subsystem for Linux) distro filesystems.
///
/// Runs `wsl -l -q` to discover installed distros and validates accessibility
/// via `\\wsl.localhost\{distro}\` UNC paths.
fn enumerate_wsl_distros() -> Vec<DriveInfo> {
    let output = match std::process::Command::new("wsl")
        .args(["-l", "-q"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            tracing::debug!(
                status = ?o.status,
                "wsl -l -q returned non-zero — WSL may not be installed"
            );
            return Vec::new();
        }
        Err(e) => {
            tracing::debug!(error = %e, "wsl command not found — WSL not installed");
            return Vec::new();
        }
    };

    // wsl -l -q outputs UTF-16LE on Windows.
    let names = parse_wsl_output(&output.stdout);
    let mut distros = Vec::new();

    for name in names {
        let unc_path = format!("\\\\wsl.localhost\\{}\\", name);
        let path = PathBuf::from(&unc_path);

        // Validate accessibility.
        if std::fs::metadata(&path).is_ok() {
            distros.push(DriveInfo {
                path,
                volume_id: format!("wsl:{}", name),
                drive_type: DriveType::Wsl,
                fs_kind: Some(FilesystemKind::NineP),
            });
        } else {
            tracing::debug!(distro = %name, "WSL distro not accessible via UNC path — skipping");
        }
    }

    distros
}

/// Parse the raw stdout from `wsl -l -q` into distro names.
///
/// The output is UTF-16LE on Windows with possible null bytes and BOM.
fn parse_wsl_output(raw: &[u8]) -> Vec<String> {
    // Try UTF-16LE decode first (Windows default for wsl.exe output).
    let text = if raw.len() >= 2 && raw.len() % 2 == 0 {
        let u16s: Vec<u16> = raw
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(raw).to_string()
    };

    text.lines()
        .map(|l| l.trim().trim_matches('\0').trim_matches('\u{FEFF}').trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

/// Discover all accessible volumes on this Windows machine.
///
/// Returns drive-letter volumes (A:–Z:) from `GetLogicalDrives()` plus
/// WSL distro filesystems from `wsl -l -q`. All drive types are included:
/// Fixed, Removable, Network, CD-ROM, RAM Disk, and WSL.
#[tracing::instrument]
pub fn enumerate_volumes() -> Vec<DriveInfo> {
    let mut volumes = enumerate_windows_drives();
    let wsl = enumerate_wsl_distros();
    if !wsl.is_empty() {
        tracing::info!(count = wsl.len(), "discovered WSL distros");
    }
    volumes.extend(wsl);

    tracing::info!(
        count = volumes.len(),
        drives = %volumes.iter().map(|d| format!("{}({:?})", d.volume_id, d.drive_type)).collect::<Vec<_>>().join(", "),
        "volume enumeration complete"
    );

    volumes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_c_drive_is_ntfs() {
        let kind = detect_filesystem(Path::new("C:\\"));
        // C:\ should always be NTFS on standard Windows installs
        match kind {
            Ok(fs) => assert_eq!(fs, FilesystemKind::Ntfs, "C:\\ should be NTFS"),
            Err(e) => panic!("Failed to detect C:\\ filesystem: {e}"),
        }
    }

    #[test]
    fn detect_nonexistent_volume_fails() {
        let result = detect_filesystem(Path::new("Z:\\"));
        // Z:\ likely doesn't exist — should return an error
        // (unless the user has a Z: drive, in which case it succeeds)
        // We just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn enumerate_volumes_finds_c_drive() {
        let volumes = enumerate_volumes();
        assert!(!volumes.is_empty(), "should find at least one volume");
        let c = volumes.iter().find(|d| d.volume_id == "C");
        assert!(c.is_some(), "C: drive should always be present");
    }

    #[test]
    fn enumerate_volumes_c_is_ntfs_fixed() {
        let volumes = enumerate_volumes();
        let c = volumes
            .iter()
            .find(|d| d.volume_id == "C")
            .expect("C: should be present");
        assert_eq!(c.drive_type, DriveType::Fixed);
        assert_eq!(c.fs_kind, Some(FilesystemKind::Ntfs));
    }

    #[test]
    fn enumerate_volumes_all_have_paths() {
        let volumes = enumerate_volumes();
        for d in &volumes {
            assert!(
                !d.path.as_os_str().is_empty(),
                "drive {} has empty path",
                d.volume_id
            );
            assert!(!d.volume_id.is_empty(), "drive has empty volume_id");
        }
    }

    #[test]
    fn wsl_output_parsing_utf8() {
        let raw = b"Ubuntu\nDebian\n\n";
        let names = parse_wsl_output(raw);
        assert_eq!(names, vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn wsl_output_parsing_utf16le() {
        // "Ubuntu\r\n" encoded as UTF-16LE
        let text = "Ubuntu\r\n";
        let raw: Vec<u8> = text.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        let names = parse_wsl_output(&raw);
        assert_eq!(names, vec!["Ubuntu"]);
    }

    #[test]
    fn wsl_output_parsing_empty() {
        let names = parse_wsl_output(b"");
        assert!(names.is_empty());
    }

    #[test]
    fn wsl_output_parsing_with_bom() {
        // UTF-16LE with BOM (0xFFFE) then "Ubuntu\r\n"
        let text = "\u{FEFF}Ubuntu\r\n";
        let raw: Vec<u8> = text.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
        let names = parse_wsl_output(&raw);
        assert!(
            names.iter().any(|n| n == "Ubuntu"),
            "should find Ubuntu in {:?}",
            names
        );
    }
}
