//! HyprDrive Core — Virtual Distributed File System
//!
//! This crate contains the core implementation of HyprDrive's VDFS:
//! domain models, operations (CQRS), infrastructure (DB, events, jobs),
//! and high-level services (sync, network, media processing).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::dbg_macro,
    missing_docs,
    unsafe_code
)]

pub mod db;
pub mod domain;
