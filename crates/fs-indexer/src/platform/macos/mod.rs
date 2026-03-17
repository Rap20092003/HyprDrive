//! macOS filesystem indexer — FSEvents + getattrlistbulk.
//!
//! TODO Phase 5: Implement macOS-native scanning via:
//! - `getattrlistbulk()` for fast metadata enumeration
//! - FSEvents for filesystem change watching
//! - XPC for privileged helper communication
