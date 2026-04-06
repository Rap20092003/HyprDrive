//! WizTree-speed disk analysis engine (treemap, aggregation, insights)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::dbg_macro,
    missing_docs,
    unsafe_code
)]

pub mod bubble_up;
pub mod patterns;
pub mod treemap;

pub use bubble_up::{compute_bubble_up, DirSizeDelta};
pub use patterns::{build_artifact_sql_fragment, is_build_artifact_dir, BUILD_ARTIFACT_PATTERNS};
pub use treemap::{squarify, Rect, TreemapItem, TreemapNode};
