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
    println!("Not yet connected — daemon socket support is Phase 13");

    // FIXME(phase-13): connect to daemon via Unix socket / named pipe
    // FIXME(phase-13): parse CLI args with clap

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // Placeholder — ensures this crate appears in `cargo test` output.
    }
}
