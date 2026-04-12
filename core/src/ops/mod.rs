//! CQRS Operations Layer — commands and queries for the HyprDrive frontend.
//!
//! Every file-system mutation is expressed as a [`CoreAction`] that:
//! - Accepts a typed `Input`
//! - Produces a typed `Output`
//! - Always returns an `UndoEntry` (type-enforced — no opt-out)
//!
//! Actions are registered at compile time via the `inventory` crate so the
//! rspc router can discover them without a manual list.

pub mod actions;
pub mod context;
pub mod error;
pub mod registry;

pub use context::{IndexContext, OperationsContext, SessionContext, StorageContext};
pub use error::OpsError;
pub use registry::ActionRegistry;

use crate::domain::undo::UndoEntry;

/// Trait implemented by every file-system action.
///
/// # Type parameters
/// - `Input`: JSON-deserializable command parameters (sent from the UI)
/// - `Output`: JSON-serializable result returned to the UI
///
/// # Undo contract
/// Every `execute()` call **must** produce a valid [`UndoEntry`] whose
/// `inverse_action` field encodes enough information to reverse the effect.
/// This is enforced at the type level by the `(Output, UndoEntry)` return tuple.
pub trait CoreAction: Send + Sync + 'static {
    /// Parameters the caller provides (deserialized from JSON by rspc).
    type Input: serde::Serialize + serde::de::DeserializeOwned + Send + Sync;

    /// Result returned to the caller (serialized to JSON by rspc).
    type Output: serde::Serialize + Send + Sync;

    /// Human-readable action name used for registry lookup and audit logs.
    fn name(&self) -> &'static str;

    /// Execute the action.
    ///
    /// On success, returns `(output, undo_entry)`.  The caller is responsible
    /// for pushing `undo_entry` onto the [`OperationsContext::undo_stack`].
    fn execute(
        &self,
        ctx: &OperationsContext,
        input: Self::Input,
    ) -> impl std::future::Future<Output = Result<(Self::Output, UndoEntry), OpsError>> + Send;
}
