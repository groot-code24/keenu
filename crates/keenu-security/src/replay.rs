// SPDX-License-Identifier: GPL-3.0-or-later

//! Remembers the requests already seen, so a replayed one, or a second answer to one, does nothing.

use std::collections::VecDeque;

/// How many ids are remembered; old ones have long since timed out.
const REMEMBERED: usize = 1024;

/// The ids seen lately.
#[derive(Debug, Default)]
pub struct ReplayGuard {
    seen: VecDeque<String>,
}

impl ReplayGuard {
    /// True the first time `id` is seen, false ever after.
    pub fn first_time(&mut self, id: &str) -> bool {
        if self.seen.iter().any(|seen| seen == id) {
            return false;
        }
        if self.seen.len() == REMEMBERED {
            self.seen.pop_front();
        }
        self.seen.push_back(id.to_owned());
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repeat_is_refused() {
        let mut guard = ReplayGuard::default();
        assert!(guard.first_time("a"));
        assert!(!guard.first_time("a"));
        assert!(guard.first_time("b"));
    }

    #[test]
    fn it_does_not_grow_without_end() {
        let mut guard = ReplayGuard::default();
        for i in 0..REMEMBERED * 2 {
            guard.first_time(&i.to_string());
        }
        assert_eq!(guard.seen.len(), REMEMBERED);
    }
}
