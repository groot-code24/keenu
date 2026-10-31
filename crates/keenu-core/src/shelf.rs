// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! The file shelf: files parked on the island to drag later; it never copies or moves them.

use serde::{Deserialize, Serialize};

/// The shelf holds this many files, all in view at once; to add another,
/// one must be removed first.
pub const CAPACITY: usize = 6;

/// One file parked on the shelf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShelfItem {
    /// `file://` URI of the item.
    pub uri: String,
    /// Display name, usually the file name.
    pub name: String,
    pub added_at_ms: i64,
}

/// Files parked on the island, at most [`CAPACITY`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Shelf {
    items: Vec<ShelfItem>,
}

impl Shelf {
    /// The parked files, newest first.
    pub fn items(&self) -> &[ShelfItem] {
        &self.items
    }

    /// Adds an item at the front, moving it there if already present.
    /// Returns false, changing nothing, when a new item finds the shelf full.
    pub fn add(&mut self, item: ShelfItem) -> bool {
        let present = self.items.iter().any(|existing| existing.uri == item.uri);
        if !present && self.items.len() >= CAPACITY {
            return false;
        }
        self.items.retain(|existing| existing.uri != item.uri);
        self.items.insert(0, item);
        true
    }

    /// Takes the file with `uri` off the shelf; true if it was there.
    pub fn remove(&mut self, uri: &str) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.uri != uri);
        self.items.len() != before
    }

    /// Empties the shelf; the files themselves stay where they are.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Keeps only the items for which `exists` returns true; true if any were dropped.
    pub fn retain_existing(&mut self, exists: impl Fn(&ShelfItem) -> bool) -> bool {
        let before = self.items.len();
        self.items.retain(exists);
        self.items.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(uri: &str) -> ShelfItem {
        ShelfItem {
            uri: uri.into(),
            name: uri.into(),
            added_at_ms: 0,
        }
    }

    #[test]
    fn re_adding_moves_to_front() {
        let mut shelf = Shelf::default();
        shelf.add(item("a"));
        shelf.add(item("b"));
        shelf.add(item("a"));
        let uris: Vec<_> = shelf.items().iter().map(|i| i.uri.as_str()).collect();
        assert_eq!(uris, ["a", "b"]);
    }

    #[test]
    fn a_full_shelf_refuses_new_files() {
        let mut shelf = Shelf::default();
        for n in 0..CAPACITY {
            assert!(shelf.add(item(&n.to_string())));
        }
        assert!(!shelf.add(item("new")));
        assert_eq!(shelf.items().len(), CAPACITY);
        // A file already there can still move to the front.
        assert!(shelf.add(item("0")));
        assert_eq!(shelf.items()[0].uri, "0");
        assert!(shelf.remove("1"));
        assert!(shelf.add(item("new")));
    }
}
