//! Priority scan ordering — determines which directories to index first.
//!
//! When HyprDrive starts a full scan, it doesn't just walk directories
//! in arbitrary order. User-facing directories (Desktop, Documents, Downloads)
//! are indexed first so the UI can show results within seconds, while
//! low-priority directories (node_modules, .git, build artifacts) are scanned last.
//!
//! # Priority Tiers
//!
//! | Priority | Directories                           | Why first?                        |
//! |----------|---------------------------------------|-----------------------------------|
//! | 0 (high) | Desktop, Documents, Downloads, Photos | User sees these immediately       |
//! | 1        | Music, Videos, home root              | Commonly browsed                  |
//! | 2        | Program Files, /usr, /opt             | System software                   |
//! | 3        | AppData, .cache, Library              | Hidden from most users            |
//! | 4 (low)  | node_modules, .git, target, __pycache__| Bulk noise, scan last            |

use std::path::Path;

/// Priority tier for scan ordering (lower = scanned first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScanPriority(u8);

impl ScanPriority {
    /// Highest priority — user-facing directories.
    pub const USER_FACING: Self = Self(0);
    /// Common media/home directories.
    pub const COMMON: Self = Self(1);
    /// System/program directories.
    pub const SYSTEM: Self = Self(2);
    /// Hidden/cache directories.
    pub const HIDDEN: Self = Self(3);
    /// Bulk/noise directories (node_modules, .git, build artifacts).
    pub const BULK_NOISE: Self = Self(4);
    /// Default priority for unrecognized paths.
    pub const DEFAULT: Self = Self(2);

    /// Raw numeric value (0 = highest priority).
    pub fn value(self) -> u8 {
        self.0
    }
}

/// Classify a directory path into a scan priority tier.
///
/// The classification is based on the last path component and well-known
/// directory patterns. Case-insensitive on Windows, case-sensitive on Linux/macOS.
pub fn classify_priority(path: &Path) -> ScanPriority {
    // Check for bulk noise patterns first (these can appear deep in any tree)
    if is_bulk_noise(path) {
        return ScanPriority::BULK_NOISE;
    }

    // Check for hidden/cache directories
    if is_hidden_cache(path) {
        return ScanPriority::HIDDEN;
    }

    // Check well-known directory names from the path
    let path_str = path.to_string_lossy();
    let path_lower = path_str.to_ascii_lowercase();

    // Tier 0: User-facing
    if matches_any_component(&path_lower, &[
        "desktop", "documents", "downloads", "pictures", "photos",
    ]) {
        return ScanPriority::USER_FACING;
    }

    // Tier 1: Common media/home
    if matches_any_component(&path_lower, &[
        "music", "videos", "movies", "home",
    ]) {
        return ScanPriority::COMMON;
    }

    // Tier 2: System directories
    if matches_any_component(&path_lower, &[
        "program files", "program files (x86)", "programdata",
        "usr", "opt", "etc", "var",
    ]) {
        return ScanPriority::SYSTEM;
    }

    ScanPriority::DEFAULT
}

/// Sort a list of paths by scan priority (highest priority first).
///
/// Within the same priority tier, paths are sorted alphabetically
/// for deterministic ordering.
pub fn sort_by_priority(paths: &mut [impl AsRef<Path>]) {
    paths.sort_by(|a, b| {
        let pa = classify_priority(a.as_ref());
        let pb = classify_priority(b.as_ref());
        pa.cmp(&pb).then_with(|| {
            a.as_ref()
                .to_string_lossy()
                .cmp(&b.as_ref().to_string_lossy())
        })
    });
}

/// Check if a path contains a "bulk noise" directory component.
fn is_bulk_noise(path: &Path) -> bool {
    static NOISE_NAMES: &[&str] = &[
        "node_modules",
        ".git",
        "target",        // Rust build output
        "__pycache__",
        ".tox",
        ".mypy_cache",
        "dist",
        "build",
        ".next",         // Next.js
        ".nuxt",         // Nuxt.js
        ".cache",
        "vendor",        // Go/PHP
        ".gradle",
        ".m2",           // Maven
    ];

    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        let name_lower = name.to_ascii_lowercase();
        if NOISE_NAMES.contains(&name_lower.as_str()) {
            return true;
        }
    }
    false
}

/// Check if a path contains hidden or cache directories.
fn is_hidden_cache(path: &Path) -> bool {
    static HIDDEN_NAMES: &[&str] = &[
        "appdata",
        ".local",
        ".config",
        "library",       // macOS ~/Library
        ".cache",
        ".thumbnails",
        "temp",
        "tmp",
    ];

    for component in path.components() {
        let name = component.as_os_str().to_string_lossy();
        let name_lower = name.to_ascii_lowercase();
        if HIDDEN_NAMES.contains(&name_lower.as_str()) {
            return true;
        }
    }
    false
}

/// Check if any path component matches one of the given names.
fn matches_any_component(path_lower: &str, names: &[&str]) -> bool {
    // Split on both / and \ to handle cross-platform paths
    let components: Vec<&str> = path_lower.split(['/', '\\']).collect();
    for component in components {
        if names.contains(&component) {
            return true;
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn desktop_is_highest_priority() {
        let p = classify_priority(Path::new("C:\\Users\\alice\\Desktop"));
        assert_eq!(p, ScanPriority::USER_FACING);
    }

    #[test]
    fn documents_is_highest_priority() {
        let p = classify_priority(Path::new("/home/alice/Documents"));
        assert_eq!(p, ScanPriority::USER_FACING);
    }

    #[test]
    fn downloads_is_highest_priority() {
        let p = classify_priority(Path::new("/home/alice/Downloads"));
        assert_eq!(p, ScanPriority::USER_FACING);
    }

    #[test]
    fn pictures_is_highest_priority() {
        let p = classify_priority(Path::new("C:\\Users\\alice\\Pictures"));
        assert_eq!(p, ScanPriority::USER_FACING);
    }

    #[test]
    fn music_is_common_priority() {
        let p = classify_priority(Path::new("/home/alice/Music"));
        assert_eq!(p, ScanPriority::COMMON);
    }

    #[test]
    fn node_modules_is_bulk_noise() {
        let p = classify_priority(Path::new("/home/alice/projects/my-app/node_modules"));
        assert_eq!(p, ScanPriority::BULK_NOISE);
    }

    #[test]
    fn git_dir_is_bulk_noise() {
        let p = classify_priority(Path::new("/home/alice/projects/repo/.git"));
        assert_eq!(p, ScanPriority::BULK_NOISE);
    }

    #[test]
    fn rust_target_is_bulk_noise() {
        let p = classify_priority(Path::new("D:\\Projects\\myapp\\target"));
        assert_eq!(p, ScanPriority::BULK_NOISE);
    }

    #[test]
    fn pycache_is_bulk_noise() {
        let p = classify_priority(Path::new("/home/alice/project/__pycache__"));
        assert_eq!(p, ScanPriority::BULK_NOISE);
    }

    #[test]
    fn appdata_is_hidden() {
        let p = classify_priority(Path::new("C:\\Users\\alice\\AppData\\Local\\Temp"));
        assert_eq!(p, ScanPriority::HIDDEN);
    }

    #[test]
    fn program_files_is_system() {
        let p = classify_priority(Path::new("C:\\Program Files\\Git"));
        assert_eq!(p, ScanPriority::SYSTEM);
    }

    #[test]
    fn unknown_path_gets_default() {
        let p = classify_priority(Path::new("/mnt/external/random-dir"));
        assert_eq!(p, ScanPriority::DEFAULT);
    }

    #[test]
    fn sort_by_priority_orders_correctly() {
        let mut paths: Vec<PathBuf> = vec![
            PathBuf::from("/home/alice/projects/app/node_modules"),
            PathBuf::from("/home/alice/Documents"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/home/alice/Music"),
            PathBuf::from("/home/alice/.config/nvim"),
            PathBuf::from("/home/alice/Downloads"),
        ];

        sort_by_priority(&mut paths);

        // User-facing first (Documents, Downloads), then Common (Music),
        // then System (/usr), then Hidden (.config), then Bulk (node_modules)
        assert_eq!(paths[0], PathBuf::from("/home/alice/Documents"));
        assert_eq!(paths[1], PathBuf::from("/home/alice/Downloads"));
        assert_eq!(paths[2], PathBuf::from("/home/alice/Music"));
        assert_eq!(paths[3], PathBuf::from("/usr/local/bin"));
        assert_eq!(paths[4], PathBuf::from("/home/alice/.config/nvim"));
        assert_eq!(paths[5], PathBuf::from("/home/alice/projects/app/node_modules"));
    }

    #[test]
    fn sort_is_stable_within_tier() {
        let mut paths: Vec<PathBuf> = vec![
            PathBuf::from("/home/alice/Downloads"),
            PathBuf::from("/home/alice/Desktop"),
            PathBuf::from("/home/alice/Documents"),
        ];

        sort_by_priority(&mut paths);

        // All tier 0, sorted alphabetically
        assert_eq!(paths[0], PathBuf::from("/home/alice/Desktop"));
        assert_eq!(paths[1], PathBuf::from("/home/alice/Documents"));
        assert_eq!(paths[2], PathBuf::from("/home/alice/Downloads"));
    }

    #[test]
    fn priority_ordering() {
        assert!(ScanPriority::USER_FACING < ScanPriority::COMMON);
        assert!(ScanPriority::COMMON < ScanPriority::SYSTEM);
        assert!(ScanPriority::SYSTEM < ScanPriority::HIDDEN);
        assert!(ScanPriority::HIDDEN < ScanPriority::BULK_NOISE);
    }
}
