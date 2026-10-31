// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! A plain to-do list: write it down, tick it off. Finished tasks are cleared
//! the next day.

use serde::{Deserialize, Serialize};

// Older lists also stored due times and alert flags; serde skips them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u64,
    pub title: String,
    pub done: bool,
    /// When the task was ticked off.
    #[serde(default)]
    pub done_at_ms: Option<i64>,
}

/// The tasks in the order they were added, with ids that never repeat.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TodoList {
    items: Vec<TodoItem>,
    next_id: u64,
}

impl TodoList {
    /// The tasks, oldest first.
    pub fn items(&self) -> &[TodoItem] {
        &self.items
    }

    /// Adds a task and returns its id; `None` for a blank title.
    pub fn add(&mut self, title: &str) -> Option<u64> {
        let title = title.trim();
        if title.is_empty() {
            return None;
        }
        self.next_id += 1;
        self.items.push(TodoItem {
            id: self.next_id,
            title: title.to_owned(),
            done: false,
            done_at_ms: None,
        });
        Some(self.next_id)
    }

    /// Ticks a task off (noting when) or back on; returns whether it was found.
    pub fn set_done(&mut self, id: u64, done: bool, now: i64) -> bool {
        self.find(id)
            .map(|item| {
                item.done = done;
                item.done_at_ms = done.then_some(now);
            })
            .is_some()
    }

    /// Gives a task a new, trimmed title; false when blank or not found.
    pub fn rename(&mut self, id: u64, title: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        self.find(id)
            .map(|item| item.title = title.to_owned())
            .is_some()
    }

    /// Drops tasks finished before `day_start`, so each day starts with only
    /// what is still open. Returns whether anything was removed.
    pub fn drop_done_before(&mut self, day_start: i64) -> bool {
        let before = self.items.len();
        self.items
            .retain(|item| !item.done || item.done_at_ms.is_some_and(|at| at >= day_start));
        self.items.len() != before
    }

    /// Deletes a task; returns whether it was there.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() != before
    }

    /// Deletes every finished task.
    pub fn clear_done(&mut self) {
        self.items.retain(|item| !item.done);
    }

    /// The task with this id, to change in place.
    fn find(&mut self, id: u64) -> Option<&mut TodoItem> {
        self.items.iter_mut().find(|item| item.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_trimmed_and_blank_ones_refused() {
        let mut list = TodoList::default();
        assert_eq!(list.add("   "), None);
        let id = list.add("  Call mum @18:30 ").unwrap();
        // Text that once meant a due time is now just part of the title.
        assert_eq!(list.items()[0].title, "Call mum @18:30");
        assert_eq!(list.items()[0].id, id);
    }

    #[test]
    fn ticking_off_records_when() {
        let mut list = TodoList::default();
        let id = list.add("A").unwrap();
        assert!(list.set_done(id, true, 42));
        assert_eq!(list.items()[0].done_at_ms, Some(42));
        assert!(list.set_done(id, false, 50));
        assert_eq!(list.items()[0].done_at_ms, None);
        assert!(!list.set_done(id + 1, true, 1));
    }

    #[test]
    fn finished_tasks_leave_the_next_day() {
        let mut list = TodoList::default();
        let old = list.add("Yesterday").unwrap();
        let today = list.add("Today").unwrap();
        list.add("Open");
        list.set_done(old, true, 100);
        list.set_done(today, true, 1_000);
        assert!(list.drop_done_before(500));
        let titles: Vec<_> = list.items().iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Today", "Open"]);
    }

    #[test]
    fn rename_rejects_empty_titles() {
        let mut list = TodoList::default();
        let id = list.add("A").unwrap();
        assert!(!list.rename(id, "  "));
        assert!(list.rename(id, "B"));
        assert_eq!(list.items()[0].title, "B");
    }

    #[test]
    fn old_lists_with_due_times_still_load() {
        let json = r#"{"items":[{"id":3,"title":"Old","due_at_ms":5,"done":false,"alerted":true}],"next_id":3}"#;
        let list: TodoList = serde_json::from_str(json).unwrap();
        assert_eq!(list.items()[0].title, "Old");
        assert_eq!(list.items()[0].done_at_ms, None);
    }
}
