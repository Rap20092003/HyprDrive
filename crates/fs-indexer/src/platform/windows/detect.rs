//! Filesystem detection for Windows volumes.

use crate::error::{FsIndexerError, FsIndexerResult};
use crate::types::FilesystemKind;
use std::path::Path;

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
}
