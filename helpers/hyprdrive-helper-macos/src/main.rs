//! HyprDrive macOS Helper
//!
//! XPC service for Full Disk Access on macOS.
//! Enables getattrlistbulk scanning of protected directories.

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
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("This binary is only intended for macOS.");
        std::process::exit(1);
    }

    #[cfg(target_os = "macos")]
    {
        println!("HyprDrive macOS Helper v{}", env!("CARGO_PKG_VERSION"));
        // TODO Phase 4: XPC service + getattrlistbulk
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // Placeholder — ensures this crate appears in `cargo test` output.
    }
}
