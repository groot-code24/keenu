// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! "Remind me in ten minutes": reminders kept until shown, so one due while the
//! screen was locked still shows afterwards.

use serde::{Deserialize, Serialize};

/// The most reminders kept at once.
pub const CAPACITY: usize = 50;

/// One reminder: what to say and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: u64,
    pub text: String,
    /// When it is due, in milliseconds since the Unix epoch.
    pub due_ms: i64,
}

/// The pending reminders, kept sorted by due time, with ids that never repeat.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Reminders {
    items: Vec<Reminder>,
    next_id: u64,
}

impl Reminders {
    /// By due time, soonest first.
    pub fn items(&self) -> &[Reminder] {
        &self.items
    }

    /// Adds a reminder and returns its id; `None` when blank or full.
    pub fn add(&mut self, text: &str, due_ms: i64) -> Option<u64> {
        let text = text.trim();
        if text.is_empty() || self.items.len() >= CAPACITY {
            return None;
        }
        self.next_id += 1;
        let at = self.items.partition_point(|item| item.due_ms <= due_ms);
        self.items.insert(
            at,
            Reminder {
                id: self.next_id,
                text: text.to_owned(),
                due_ms,
            },
        );
        Some(self.next_id)
    }

    /// Removes a shown reminder; returns whether it was there.
    pub fn dismiss(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        self.items.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_reminders_soonest_first() {
        let mut reminders = Reminders::default();
        let late = reminders.add("Call mum", 3000).unwrap();
        let soon = reminders.add("Tea", 1000).unwrap();
        let ids: Vec<u64> = reminders.items().iter().map(|r| r.id).collect();
        assert_eq!(ids, [soon, late]);
    }

    #[test]
    fn refuses_blank_text_and_too_many() {
        let mut reminders = Reminders::default();
        assert_eq!(reminders.add("  ", 1000), None);
        for n in 0..CAPACITY {
            assert!(reminders.add("x", n as i64).is_some());
        }
        assert_eq!(reminders.add("one more", 0), None);
    }

    #[test]
    fn dismisses_by_id() {
        let mut reminders = Reminders::default();
        let id = reminders.add("Tea", 1000).unwrap();
        assert!(reminders.dismiss(id));
        assert!(!reminders.dismiss(id));
        assert!(reminders.items().is_empty());
    }
}
