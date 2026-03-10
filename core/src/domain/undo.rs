//! Undo system — bounded stack of reversible operations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Maximum number of undo entries kept in the stack.
const MAX_UNDO_ENTRIES: usize = 50;

/// A single undo-able action record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UndoEntry {
    /// Human-readable description ("Moved 5 files to Photos").
    pub description: String,
    /// When this action was performed.
    pub timestamp: DateTime<Utc>,
    /// Serialized inverse action to execute on undo.
    pub inverse_action: String,
}

/// A bounded LIFO stack of undo entries.
///
/// When full (50 entries), pushing evicts the oldest entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndoStack {
    entries: VecDeque<UndoEntry>,
}

impl UndoStack {
    /// Create an empty undo stack.
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_UNDO_ENTRIES),
        }
    }

    /// Push a new entry. If at capacity, the oldest entry is evicted.
    pub fn push(&mut self, entry: UndoEntry) {
        if self.entries.len() >= MAX_UNDO_ENTRIES {
            self.entries.pop_front(); // evict oldest
        }
        self.entries.push_back(entry);
    }

    /// Pop the most recent entry (LIFO).
    pub fn pop(&mut self) -> Option<UndoEntry> {
        self.entries.pop_back()
    }

    /// Peek at the most recent entry without removing it.
    pub fn peek(&self) -> Option<&UndoEntry> {
        self.entries.back()
    }

    /// Number of entries currently in the stack.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(desc: &str) -> UndoEntry {
        UndoEntry {
            description: desc.into(),
            timestamp: Utc::now(),
            inverse_action: format!("undo_{}", desc),
        }
    }

    #[test]
    fn push_pop_lifo() {
        let mut stack = UndoStack::new();
        stack.push(make_entry("first"));
        stack.push(make_entry("second"));
        stack.push(make_entry("third"));

        assert_eq!(stack.pop().map(|e| e.description), Some("third".into()));
        assert_eq!(stack.pop().map(|e| e.description), Some("second".into()));
        assert_eq!(stack.pop().map(|e| e.description), Some("first".into()));
    }

    #[test]
    fn capacity_eviction() {
        let mut stack = UndoStack::new();
        for i in 0..60 {
            stack.push(make_entry(&format!("action_{}", i)));
        }
        assert_eq!(stack.len(), MAX_UNDO_ENTRIES);
        // Oldest should be action_10 (0-9 evicted)
        let oldest = stack.entries.front().map(|e| e.description.clone());
        assert_eq!(oldest, Some("action_10".into()));
    }

    #[test]
    fn empty_pop_returns_none() {
        let mut stack = UndoStack::new();
        assert!(stack.pop().is_none());
        assert!(stack.is_empty());
    }

    #[test]
    fn entry_has_description_and_timestamp() {
        let entry = make_entry("test action");
        assert_eq!(entry.description, "test action");
        assert!(entry.timestamp <= Utc::now());
    }
}
