// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! App usage: how long each application was in front, kept day by day.
//! The extension times focus changes and reports stretches; the rules live here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Days kept before the oldest are forgotten.
pub const KEEP_DAYS: u32 = 30;

/// The most one report may add; longer is a broken caller, not a session.
pub const MAX_REPORT_MS: i64 = 24 * 60 * 60 * 1000;

/// Names and ids are cut to these, so a broken caller cannot grow the file without bound.
const MAX_ID_CHARS: usize = 256;
const MAX_NAME_CHARS: usize = 128;

/// Every day's app usage, by date (`YYYY-MM-DD`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppUsage {
    days: BTreeMap<String, Day>,
}

/// One day's apps, by desktop id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Day {
    apps: BTreeMap<String, AppTime>,
}

/// An app's name and time on one day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct AppTime {
    name: String,
    ms: i64,
}

/// An app's total over some days, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppTotal {
    pub id: String,
    pub name: String,
    pub ms: i64,
}

impl AppUsage {
    /// Adds `ms` to `id` on `day`; false for a blank id or a time out of bounds. A newer name replaces the
    /// one already known, so an app renamed in the desktop file reads right.
    pub fn record(&mut self, day: &str, id: &str, name: &str, ms: i64) -> bool {
        let day = day.trim();
        let id = id.trim();
        if day.is_empty() || id.is_empty() || !(1..=MAX_REPORT_MS).contains(&ms) {
            return false;
        }
        let id = cut(id, MAX_ID_CHARS);
        let name = clean_name(name, &id);
        let entry = self
            .days
            .entry(day.to_owned())
            .or_default()
            .apps
            .entry(id)
            .or_default();
        entry.name = name;
        entry.ms = entry.ms.saturating_add(ms);
        true
    }

    /// Forgets days before `oldest`; whether any went.
    pub fn prune(&mut self, oldest: &str) -> bool {
        let before = self.days.len();
        self.days.retain(|day, _| day.as_str() >= oldest);
        self.days.len() != before
    }

    /// Each app's time over `days`, most used first. `days` are in order, oldest first, so a newer name wins.
    pub fn totals<'a>(&self, days: impl IntoIterator<Item = &'a str>) -> Vec<AppTotal> {
        let mut totals: BTreeMap<String, AppTotal> = BTreeMap::new();
        for day in days {
            let Some(apps) = self.days.get(day) else {
                continue;
            };
            for (id, entry) in &apps.apps {
                let total = totals.entry(id.clone()).or_insert_with(|| AppTotal {
                    id: id.clone(),
                    name: String::new(),
                    ms: 0,
                });
                if !entry.name.is_empty() {
                    total.name.clone_from(&entry.name);
                }
                total.ms = total.ms.saturating_add(entry.ms);
            }
        }
        let mut totals: Vec<_> = totals.into_values().collect();
        totals.sort_by(|a, b| {
            b.ms.cmp(&a.ms)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });
        totals
    }

    /// Forgets every day.
    pub fn clear(&mut self) {
        self.days.clear();
    }

    /// Whether no day is kept.
    pub fn is_empty(&self) -> bool {
        self.days.is_empty()
    }
}

/// The name as shown: whitespace on one line, never blank, and short enough for a row.
fn clean_name(name: &str, id: &str) -> String {
    let words: Vec<_> = name.split_whitespace().collect();
    if words.is_empty() {
        return cut(id, MAX_NAME_CHARS);
    }
    cut(&words.join(" "), MAX_NAME_CHARS)
}

/// At most `max` characters, never inside one.
fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_sums_an_app() {
        let mut usage = AppUsage::default();
        assert!(usage.record("2026-10-20", "firefox.desktop", "Firefox", 1_000));
        assert!(usage.record("2026-10-20", "firefox.desktop", "Firefox", 2_500));
        let totals = usage.totals(["2026-10-20"]);
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].id, "firefox.desktop");
        assert_eq!(totals[0].name, "Firefox");
        assert_eq!(totals[0].ms, 3_500);
    }

    #[test]
    fn totals_merge_days_and_keep_the_newest_name() {
        let mut usage = AppUsage::default();
        usage.record("2026-10-19", "kitty.desktop", "Kitty", 1_000);
        usage.record("2026-10-20", "kitty.desktop", "Kitty Terminal", 2_000);
        usage.record("2026-10-20", "firefox.desktop", "Firefox", 4_000);
        let totals = usage.totals(["2026-10-19", "2026-10-20"]);
        assert_eq!(totals[0].id, "firefox.desktop");
        assert_eq!(totals[1].name, "Kitty Terminal");
        assert_eq!(totals[1].ms, 3_000);
    }

    #[test]
    fn names_are_one_line_and_never_blank() {
        let mut usage = AppUsage::default();
        usage.record("2026-10-20", "app.desktop", "  Two\n lines\t ", 1_000);
        usage.record("2026-10-20", "other.desktop", "  ", 1_000);
        let totals = usage.totals(["2026-10-20"]);
        assert_eq!(
            totals
                .iter()
                .find(|app| app.id == "app.desktop")
                .unwrap()
                .name,
            "Two lines"
        );
        assert_eq!(
            totals
                .iter()
                .find(|app| app.id == "other.desktop")
                .unwrap()
                .name,
            "other.desktop"
        );
    }

    #[test]
    fn blank_ids_and_impossible_times_are_refused() {
        let mut usage = AppUsage::default();
        assert!(!usage.record("2026-10-20", "", "App", 1_000));
        assert!(!usage.record("2026-10-20", "app.desktop", "App", 0));
        assert!(!usage.record("2026-10-20", "app.desktop", "App", -5));
        assert!(!usage.record("2026-10-20", "app.desktop", "App", MAX_REPORT_MS + 1));
        assert!(usage.is_empty());
    }

    #[test]
    fn pruning_forgets_old_days_only() {
        let mut usage = AppUsage::default();
        usage.record("2026-09-01", "old.desktop", "Old", 1_000);
        usage.record("2026-10-01", "new.desktop", "New", 1_000);
        assert!(usage.prune("2026-10-01"));
        assert!(!usage.prune("2026-10-01"));
        let totals = usage.totals(["2026-09-01", "2026-10-01"]);
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].id, "new.desktop");
    }

    #[test]
    fn clearing_forgets_every_day() {
        let mut usage = AppUsage::default();
        usage.record("2026-10-20", "app.desktop", "App", 1_000);
        assert!(!usage.is_empty());
        usage.clear();
        assert!(usage.is_empty());
    }

    #[test]
    fn old_files_still_load() {
        let json = r#"{"days":{"2026-10-20":{"apps":{"app.desktop":{"name":"App","ms":5,"extra":true}}}}}"#;
        let usage: AppUsage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.totals(["2026-10-20"])[0].ms, 5);
    }
}
