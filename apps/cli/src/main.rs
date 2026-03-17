//! HyprDrive CLI — Thin Client
//!
//! Connects to hyprdrive-daemon via socket. Zero core logic here.
//! All intelligence lives in the daemon.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::dbg_macro,
    missing_docs,
    unsafe_code
)]

use anyhow::Result;

fn main() -> Result<()> {
    println!("HyprDrive CLI v{}", env!("CARGO_PKG_VERSION"));
    println!("TODO: Connect to daemon socket at :7420");

    // TODO Phase 13: Connect to daemon via Unix socket / named pipe
    // TODO: Parse CLI args (clap)

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // Placeholder — ensures this crate appears in `cargo test` output.
    }
}
