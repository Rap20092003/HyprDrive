//! Perceptual image matching using blockhash.
//!
//! Finds visually similar images even when resized, recompressed, or
//! slightly modified. Uses the `image_hasher` crate for blockhash computation
//! and Hamming distance for similarity scoring.
//!
//! This module is behind the `perceptual` feature flag.

#[cfg(feature = "perceptual")]
use crate::error::DeduplicateError;
#[cfg(feature = "perceptual")]
use crate::FileEntry;

/// Image file extensions recognized by the perceptual matcher.
pub const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff", "tif"];

/// Check if a file extension indicates an image.
pub fn is_image(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

/// A perceptual match between two images.
#[derive(Debug, Clone)]
pub struct PerceptualMatch {
    /// Index of first image in the input slice.
    pub idx_a: usize,
    /// Index of second image in the input slice.
    pub idx_b: usize,
    /// Hamming distance (lower = more similar, 0 = identical).
    pub distance: u32,
}

/// Find visually similar images using perceptual hashing.
///
/// Only processes files with image extensions. Computes blockhash for each
/// image and compares pairwise Hamming distance against `threshold`.
#[cfg(feature = "perceptual")]
#[tracing::instrument(skip(files), fields(file_count = files.len(), threshold))]
pub fn find_similar_images(
    files: &[FileEntry],
    threshold: u32,
) -> Result<Vec<PerceptualMatch>, DeduplicateError> {
    use rayon::prelude::*;

    // Filter to image files only
    let images: Vec<(usize, &FileEntry)> = files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.extension.as_deref().map(is_image).unwrap_or(false))
        .collect();

    if images.len() < 2 {
        return Ok(Vec::new());
    }

    // Compute hashes in parallel
    let hasher = image_hasher::HasherConfig::new()
        .hash_size(16, 16)
        .to_hasher();

    let hashes: Vec<Option<(usize, image_hasher::ImageHash)>> = images
        .par_iter()
        .map(|(idx, f)| {
            match image::open(&f.path) {
                Ok(img) => {
                    let hash = hasher.hash_image(&img);
                    Some((*idx, hash))
                }
                Err(e) => {
                    tracing::warn!(path = %f.path.display(), error = %e, "Failed to open image, skipping");
                    None
                }
            }
        })
        .collect();

    let valid_hashes: Vec<(usize, image_hasher::ImageHash)> =
        hashes.into_iter().flatten().collect();

    // Pairwise distance
    let mut matches = Vec::new();
    for i in 0..valid_hashes.len() {
        for j in (i + 1)..valid_hashes.len() {
            let dist = valid_hashes[i].1.dist(&valid_hashes[j].1);
            if dist <= threshold {
                matches.push(PerceptualMatch {
                    idx_a: valid_hashes[i].0,
                    idx_b: valid_hashes[j].0,
                    distance: dist,
                });
            }
        }
    }

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_recognizes_common_formats() {
        assert!(is_image("jpg"));
        assert!(is_image("JPEG"));
        assert!(is_image("png"));
        assert!(is_image("webp"));
        assert!(is_image("bmp"));
        assert!(is_image("gif"));
        assert!(is_image("tiff"));
        assert!(is_image("tif"));
    }

    #[test]
    fn is_image_rejects_non_images() {
        assert!(!is_image("pdf"));
        assert!(!is_image("txt"));
        assert!(!is_image("mp3"));
        assert!(!is_image("doc"));
    }

    #[cfg(feature = "perceptual")]
    #[test]
    fn find_similar_empty_input() {
        let result = find_similar_images(&[], 10).unwrap();
        assert!(result.is_empty());
    }

    #[cfg(feature = "perceptual")]
    #[test]
    fn find_similar_filters_non_images() {
        use std::path::PathBuf;
        let files = vec![FileEntry {
            path: PathBuf::from("/test/file.txt"),
            size: 100,
            name: "file.txt".to_string(),
            extension: Some("txt".to_string()),
            modified_at: 0,
            inode: None,
        }];
        let result = find_similar_images(&files, 10).unwrap();
        assert!(result.is_empty());
    }
}
