//! HyprDrive Windows Helper
//!
//! Privileged helper service for MFT access.
//! Runs with SeManageVolumePrivilege to read NTFS MFT directly.
//! Communicates with the daemon via named pipe IPC.

fn main() {
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("This binary is only intended for Windows.");
        std::process::exit(1);
    }

    #[cfg(target_os = "windows")]
    {
        println!("HyprDrive Windows Helper v{}", env!("CARGO_PKG_VERSION"));
        // TODO Phase 3: MFT reader + named pipe IPC
    }
}
