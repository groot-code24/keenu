// SPDX-License-Identifier: GPL-3.0-or-later

//! The opt-in record of which application was in front, on hosts that time it themselves.
//!
//! By construction it can hold only an application's identifier, when it came to the front and when it
//! left. There is no field for a window title, a document, a keystroke or a screenshot.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "usage.json";
/// Stretches shorter than this are switching past an application, not using it.
const MIN_SESSION_MS: i64 = 1_000;

/// One stretch of an application being in front.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub app: String,
    pub start_ms: i64,
    pub stop_ms: i64,
}

impl Session {
    pub fn duration_ms(&self) -> i64 {
        self.stop_ms - self.start_ms
    }
}

/// All recorded sessions, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageLog {
    sessions: Vec<Session>,
}

/// One row of a summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSummary {
    pub app: String,
    pub category: String,
    pub duration_ms: i64,
}

impl UsageLog {
    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    /// Records a stretch; one too short, or running backwards from a clock change, is dropped. True if kept.
    pub fn record(&mut self, app: &str, start_ms: i64, stop_ms: i64) -> bool {
        if app.is_empty() || stop_ms - start_ms < MIN_SESSION_MS {
            return false;
        }
        self.sessions.push(Session {
            app: app.to_owned(),
            start_ms,
            stop_ms,
        });
        true
    }

    /// Drops sessions that ended before `cutoff_ms`; true if any went.
    pub fn prune(&mut self, cutoff_ms: i64) -> bool {
        let before = self.sessions.len();
        self.sessions.retain(|session| session.stop_ms >= cutoff_ms);
        self.sessions.len() != before
    }

    /// Deletes everything.
    pub fn clear(&mut self) {
        self.sessions.clear();
    }

    /// Deletes one application's sessions; how many there were.
    pub fn delete_app(&mut self, app: &str) -> usize {
        let before = self.sessions.len();
        self.sessions.retain(|session| session.app != app);
        before - self.sessions.len()
    }

    /// Time per application from `since_ms`, most used first. The part of a session before then does not count.
    pub fn summary(&self, since_ms: i64, categories: &BTreeMap<String, String>) -> Vec<AppSummary> {
        let mut totals: BTreeMap<&str, i64> = BTreeMap::new();
        for session in &self.sessions {
            let counted = session.stop_ms - session.start_ms.max(since_ms);
            if counted > 0 {
                *totals.entry(&session.app).or_default() += counted;
            }
        }
        let mut rows: Vec<AppSummary> = totals
            .into_iter()
            .map(|(app, duration_ms)| AppSummary {
                app: app.to_owned(),
                category: categories.get(app).cloned().unwrap_or_default(),
                duration_ms,
            })
            .collect();
        rows.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms).then(a.app.cmp(&b.app)));
        rows
    }

    /// The whole log as CSV, for the user's own export.
    pub fn to_csv(&self, categories: &BTreeMap<String, String>) -> String {
        let mut out = String::from("app,category,start_ms,stop_ms,duration_ms\n");
        for session in &self.sessions {
            let category = categories.get(&session.app).map_or("", String::as_str);
            out.push_str(&format!(
                "{},{},{},{},{}\n",
                csv_field(&session.app),
                csv_field(category),
                session.start_ms,
                session.stop_ms,
                session.duration_ms()
            ));
        }
        out
    }
}

/// Quotes a CSV field, and keeps a spreadsheet from running one as a formula.
fn csv_field(value: &str) -> String {
    let guarded = if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_what_is_long_enough_and_runs_forwards() {
        let mut log = UsageLog::default();
        assert!(log.record("code.exe", 0, 5_000));
        assert!(!log.record("code.exe", 5_000, 5_500));
        assert!(!log.record("code.exe", 9_000, 1_000));
        assert!(!log.record("", 0, 5_000));
        assert_eq!(log.sessions().len(), 1);
    }

    #[test]
    fn a_saved_session_holds_nothing_but_app_and_times() {
        let mut log = UsageLog::default();
        log.record("code.exe", 0, 5_000);
        let json = serde_json::to_value(&log.sessions()[0]).unwrap();
        let mut fields: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        fields.sort_unstable();
        assert_eq!(fields, ["app", "start_ms", "stop_ms"]);
    }

    #[test]
    fn summarises_with_categories_most_used_first() {
        let mut log = UsageLog::default();
        log.record("a.exe", 0, 2_000);
        log.record("b.exe", 0, 9_000);
        log.record("a.exe", 10_000, 12_000);
        let categories = BTreeMap::from([("b.exe".to_owned(), "Work".to_owned())]);
        let rows = log.summary(0, &categories);
        assert_eq!(rows[0].app, "b.exe");
        assert_eq!(rows[0].category, "Work");
        assert_eq!(
            (rows[1].app.as_str(), rows[1].duration_ms),
            ("a.exe", 4_000)
        );
    }

    #[test]
    fn a_summary_counts_only_from_when_it_asks() {
        let mut log = UsageLog::default();
        log.record("a.exe", 0, 10_000);
        let rows = log.summary(8_000, &BTreeMap::new());
        assert_eq!(rows[0].duration_ms, 2_000);
        assert!(log.summary(20_000, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn retention_and_deleting_remove_what_they_say() {
        let mut log = UsageLog::default();
        log.record("old.exe", 0, 2_000);
        log.record("new.exe", 10_000, 12_000);
        log.record("other.exe", 10_000, 12_000);
        assert!(log.prune(5_000));
        assert!(!log.prune(5_000));
        assert_eq!(log.delete_app("other.exe"), 1);
        assert_eq!(log.sessions()[0].app, "new.exe");
        log.clear();
        assert!(log.sessions().is_empty());
    }

    #[test]
    fn exports_csv_a_spreadsheet_cannot_be_tricked_by() {
        let mut log = UsageLog::default();
        log.record("=cmd.exe", 0, 2_000);
        log.record("a,b.exe", 0, 2_000);
        let csv = log.to_csv(&BTreeMap::new());
        assert!(csv.contains("'=cmd.exe,,0,2000,2000"), "{csv}");
        assert!(csv.contains("\"a,b.exe\",,0,2000,2000"), "{csv}");
    }
}
