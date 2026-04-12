//! Compile-time action registration via the `inventory` crate.
//!
//! Each action module calls `inventory::submit!` at compile time to register
//! its metadata. The [`ActionRegistry`] collects all registrations at runtime.

use std::collections::HashMap;

/// Static metadata for a registered action.
///
/// Each action registers itself at compile time via:
/// ```rust,ignore
/// inventory::submit! {
///     ActionMeta { name: "copy_file", description: "...", undoable: true }
/// }
/// ```
pub struct ActionMeta {
    /// Unique snake_case action name (used as RPC endpoint key).
    pub name: &'static str,
    /// Human-readable description shown in UI and audit logs.
    pub description: &'static str,
    /// Whether the action produces a meaningful undo operation.
    pub undoable: bool,
}

inventory::collect!(ActionMeta);

/// Runtime registry built from all compile-time `inventory::submit!` calls.
pub struct ActionRegistry {
    actions: HashMap<&'static str, &'static ActionMeta>,
}

impl ActionRegistry {
    /// Build the registry by iterating all compile-time registrations.
    pub fn build() -> Self {
        let mut actions = HashMap::new();
        for meta in inventory::iter::<ActionMeta> {
            actions.insert(meta.name, meta);
        }
        Self { actions }
    }

    /// List all registered action names.
    pub fn list(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = self.actions.keys().copied().collect();
        names.sort_unstable();
        names
    }

    /// Look up action metadata by name.
    pub fn get(&self, name: &str) -> Option<&&'static ActionMeta> {
        self.actions.get(name)
    }

    /// Number of registered actions.
    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Returns `true` if no actions have been registered.
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}
