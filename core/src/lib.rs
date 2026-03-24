//! HyprDrive Core — Virtual Distributed File System
//!
//! This crate contains the core implementation of HyprDrive's VDFS:
//! domain models, operations (CQRS), infrastructure (DB, events, jobs),
//! and high-level services (sync, network, media processing).

#![allow(missing_docs)]

pub mod db;
pub mod domain;
