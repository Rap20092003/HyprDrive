//! HyprDrive Windows Helper
//!
//! Privileged helper process for MFT/USN journal access.
//! Runs with elevated privileges and communicates with the daemon
//! via Named Pipe IPC using the [`hyprdrive_ipc_protocol`] wire format.
//!
//! ## Usage
//!
//! ```bash
//! # Run in foreground (development)
//! hyprdrive-helper-windows.exe
//!
//! # Install as Windows service (requires admin)
//! powershell -ExecutionPolicy Bypass -File install-service.ps1
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::dbg_macro,
    missing_docs,
    unsafe_code
)]

#[cfg(target_os = "windows")]
mod pipe_server;

fn main() {
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("This binary is only intended for Windows.");
        std::process::exit(1);
    }

    #[cfg(target_os = "windows")]
    {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();

        tracing::info!(
            version = env!("CARGO_PKG_VERSION"),
            pipe = hyprdrive_ipc_protocol::PIPE_NAME,
            "starting HyprDrive Windows Helper"
        );

        if let Err(e) = pipe_server::run() {
            tracing::error!(error = %e, "helper exited with error");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        // Placeholder — ensures this crate appears in `cargo test` output.
    }
}
