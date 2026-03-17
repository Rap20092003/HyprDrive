//! HyprDrive Linux Helper
//!
//! Setuid helper for fanotify filesystem monitoring.
//! Enables io_uring + getdents64 high-performance scanning.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::dbg_macro,
    missing_docs,
    unsafe_code
)]

fn main() {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("This binary is only intended for Linux.");
        std::process::exit(1);
    }

    #[cfg(target_os = "linux")]
    {
        println!("HyprDrive Linux Helper v{}", env!("CARGO_PKG_VERSION"));
        // TODO Phase 5: fanotify + io_uring
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // Placeholder — ensures this crate appears in `cargo test` output.
    }
}
