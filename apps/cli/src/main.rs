//! HyprDrive CLI — Thin Client
//!
//! Connects to hyprdrive-daemon via socket. Zero core logic here.
//! All intelligence lives in the daemon.

use anyhow::Result;

fn main() -> Result<()> {
    println!("HyprDrive CLI v{}", env!("CARGO_PKG_VERSION"));
    println!("TODO: Connect to daemon socket at :7420");

    // TODO Phase 13: Connect to daemon via Unix socket / named pipe
    // TODO: Parse CLI args (clap)

    Ok(())
}
