// SPDX-License-Identifier: GPL-3.0-or-later

//! The order notifications go out in: most urgent first, and one that says the same thing as a recent
//! one is dropped. Delivering them is each platform's job.

use std::collections::{BTreeMap, VecDeque};

/// How much a notification matters, least first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Priority {
    Low,
    Normal,
    High,
}

/// A button on a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// What comes back when it is clicked.
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    /// Same key within [`DEDUPE_MS`] means the same notification.
    pub key: String,
    pub title: String,
    pub body: String,
    pub priority: Priority,
    pub actions: Vec<Action>,
}

/// A repeat of a notification within this long is dropped.
pub const DEDUPE_MS: i64 = 30_000;
/// Waiting notifications kept; beyond this the least urgent, oldest go.
pub const CAPACITY: usize = 64;
/// How many times delivery is tried before a notification is given up on.
pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
struct Queued {
    notification: Notification,
    attempts: u32,
}

/// Notifications waiting to be delivered.
#[derive(Debug, Default)]
pub struct Queue {
    waiting: VecDeque<Queued>,
    /// When each key was last accepted.
    seen: BTreeMap<String, i64>,
}

impl Queue {
    /// Queues `notification` unless it repeats a recent one; true if it was queued.
    pub fn push(&mut self, notification: Notification, now_ms: i64) -> bool {
        self.seen.retain(|_, at| now_ms - *at < DEDUPE_MS);
        if self.seen.contains_key(&notification.key) {
            return false;
        }
        self.seen.insert(notification.key.clone(), now_ms);
        // Behind those at least as urgent, ahead of the rest.
        let at = self
            .waiting
            .iter()
            .position(|queued| queued.notification.priority < notification.priority)
            .unwrap_or(self.waiting.len());
        self.waiting.insert(
            at,
            Queued {
                notification,
                attempts: 0,
            },
        );
        if self.waiting.len() > CAPACITY {
            self.waiting.pop_back();
        }
        true
    }

    /// The next to deliver; give it back with [`Queue::failed`] if that did not work.
    pub fn take_next(&mut self) -> Option<(Notification, u32)> {
        let queued = self.waiting.pop_front()?;
        Some((queued.notification, queued.attempts))
    }

    /// Delivery failed: it goes back to the front, unless tried [`MAX_ATTEMPTS`] times. True if requeued.
    pub fn failed(&mut self, notification: Notification, attempts: u32) -> bool {
        let attempts = attempts + 1;
        if attempts >= MAX_ATTEMPTS {
            return false;
        }
        self.waiting.push_front(Queued {
            notification,
            attempts,
        });
        true
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(key: &str, priority: Priority) -> Notification {
        Notification {
            key: key.into(),
            title: key.into(),
            body: String::new(),
            priority,
            actions: Vec::new(),
        }
    }

    fn keys(queue: &mut Queue) -> Vec<String> {
        std::iter::from_fn(|| queue.take_next().map(|(n, _)| n.key)).collect()
    }

    #[test]
    fn most_urgent_goes_first_and_equals_keep_their_order() {
        let mut queue = Queue::default();
        queue.push(note("low", Priority::Low), 0);
        queue.push(note("a", Priority::Normal), 0);
        queue.push(note("urgent", Priority::High), 0);
        queue.push(note("b", Priority::Normal), 0);
        assert_eq!(keys(&mut queue), ["urgent", "a", "b", "low"]);
    }

    #[test]
    fn a_repeat_is_dropped_until_the_window_passes() {
        let mut queue = Queue::default();
        assert!(queue.push(note("x", Priority::Normal), 0));
        assert!(!queue.push(note("x", Priority::Normal), DEDUPE_MS - 1));
        assert!(queue.push(note("x", Priority::Normal), DEDUPE_MS));
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn failed_delivery_retries_then_gives_up() {
        let mut queue = Queue::default();
        queue.push(note("x", Priority::Normal), 0);
        for attempt in 0..MAX_ATTEMPTS {
            let (notification, attempts) = queue.take_next().unwrap();
            assert_eq!(attempts, attempt);
            let requeued = queue.failed(notification, attempts);
            assert_eq!(requeued, attempt + 1 < MAX_ATTEMPTS);
        }
        assert!(queue.is_empty());
    }

    #[test]
    fn a_full_queue_loses_the_least_urgent() {
        let mut queue = Queue::default();
        queue.push(note("low", Priority::Low), 0);
        for i in 0..CAPACITY {
            queue.push(note(&format!("n{i}"), Priority::Normal), 0);
        }
        assert_eq!(queue.len(), CAPACITY);
        assert!(!keys(&mut queue).contains(&"low".to_owned()));
    }
}
