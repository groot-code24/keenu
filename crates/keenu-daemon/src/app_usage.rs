// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! App usage: the extension times the app in front and reports its stretches here. Kept day by day
//! for a month, in `~/.local/share/keenu/app-usage.json`, and shown on the Apps page.

use chrono::{Local, TimeZone};
use keenu_core::app_usage::{AppTotal, AppUsage, KEEP_DAYS, MAX_REPORT_MS};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use zbus::object_server::InterfaceRef;
use zbus::zvariant::Type;
use zbus::{fdo, interface};

use crate::clock::{days_back, local_midnight, next_midnight, wait_until};
use crate::store::Store;

/// One timed stretch: an app, when it began (milliseconds since the epoch), and how long it lasted.
#[derive(Debug, Deserialize, Type)]
struct Stretch {
    id: String,
    name: String,
    start_ms: i64,
    ms: i64,
}

/// The most stretches one report may carry; the extension sends a minute's worth.
const MAX_STRETCHES: usize = 1_000;

/// What `GetStats` returns: today, the last seven days including today, and the last 30.
#[derive(Serialize)]
struct Stats {
    today: Vec<AppTotal>,
    week: Vec<AppTotal>,
    month: Vec<AppTotal>,
}

/// The AppUsage1 D-Bus object: the times and their file.
pub struct AppUsageService {
    usage: AppUsage,
    store: Store<AppUsage>,
}

impl AppUsageService {
    /// Loads the stored days, dropping those past the month kept.
    pub fn load() -> Self {
        let mut store = Store::new("app-usage");
        let mut usage: AppUsage = store.load();
        let keep = days_back(KEEP_DAYS);
        if keep.first().is_some_and(|oldest| usage.prune(oldest)) {
            store.save(&usage);
        }
        Self { usage, store }
    }

    /// Today's apps, the last seven days', and the last 30 (all that is kept).
    fn snapshot(&self) -> Stats {
        let days = days_back(KEEP_DAYS);
        let today = days.last().map_or("", String::as_str);
        let week = &days[days.len().saturating_sub(7)..];
        Stats {
            today: self.usage.totals([today]),
            week: self.usage.totals(week.iter().map(String::as_str)),
            month: self.usage.totals(days.iter().map(String::as_str)),
        }
    }
}

#[interface(name = "io.github.zickrian.isle.AppUsage1")]
impl AppUsageService {
    /// Adds stretches the extension timed; a stretch past midnight is split between the two days.
    /// GNOME Shell's to report, no other program's.
    async fn report(
        &mut self,
        stretches: Vec<Stretch>,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<()> {
        crate::caller::shell_only(connection, &header).await?;
        if stretches.len() > MAX_STRETCHES {
            return Err(fdo::Error::InvalidArgs(format!(
                "at most {MAX_STRETCHES} stretches fit in one report"
            )));
        }
        // Everything or nothing: a bad stretch in the batch does not leave half of it recorded.
        let mut next = self.usage.clone();
        for stretch in stretches {
            if !record_span(&mut next, &stretch) {
                return Err(fdo::Error::InvalidArgs(
                    "an app id, a start after 1970 and 1 ms to 24 hours are needed".into(),
                ));
            }
        }
        self.usage = next;
        self.store.save(&self.usage);
        Ok(())
    }

    /// The history as JSON `{today: [{id, name, ms}], week: [...], month: [...]}`, most used first.
    /// GNOME Shell's to ask, no other program's: it names every app the user ran.
    async fn get_stats(
        &self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<String> {
        crate::caller::shell_only(connection, &header).await?;
        serde_json::to_string(&self.snapshot())
            .map_err(|err| fdo::Error::Failed(format!("could not write app usage: {err}")))
    }

    /// Forgets every day, as Settings offers. GNOME Shell's to ask, no other program's.
    async fn clear(
        &mut self,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<()> {
        crate::caller::shell_only(connection, &header).await?;
        self.usage.clear();
        self.store.save(&self.usage);
        Ok(())
    }
}

/// Adds one stretch, splitting it at local midnights; false when its times are impossible.
fn record_span(usage: &mut AppUsage, stretch: &Stretch) -> bool {
    let &Stretch {
        ref id,
        ref name,
        start_ms,
        ms,
    } = stretch;
    if start_ms <= 0 || !(1..=MAX_REPORT_MS).contains(&ms) {
        return false;
    }
    let Some(start) = Local.timestamp_millis_opt(start_ms).single() else {
        return false;
    };
    let end_ms = start_ms.saturating_add(ms);
    let mut day = start.date_naive();
    let mut cursor = start_ms;
    loop {
        let Some(next_day) = day.succ_opt() else {
            return false;
        };
        let Some(midnight) = local_midnight(next_day) else {
            return false;
        };
        let until = end_ms.min(midnight);
        let part = until - cursor;
        if part > 0 && !usage.record(&day.to_string(), id, name, part) {
            return false;
        }
        if until >= end_ms {
            return true;
        }
        cursor = midnight;
        day = next_day;
    }
}

/// Drops days past the month kept as midnight passes.
pub async fn run(iface: InterfaceRef<AppUsageService>) {
    let wake = Notify::new();
    loop {
        if !wait_until(next_midnight(), &wake).await {
            continue;
        }
        let mut service = iface.get_mut().await;
        let keep = days_back(KEEP_DAYS);
        if keep
            .first()
            .is_some_and(|oldest| service.usage.prune(oldest))
        {
            service.store.save(&service.usage);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stretch(start_ms: i64, ms: i64) -> Stretch {
        Stretch {
            id: "app.desktop".into(),
            name: "App".into(),
            start_ms,
            ms,
        }
    }

    #[test]
    fn a_stretch_across_midnight_is_split_between_the_days() {
        let start = Local
            .with_ymd_and_hms(2026, 10, 20, 23, 50, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let mut usage = AppUsage::default();
        assert!(record_span(&mut usage, &stretch(start, 20 * 60 * 1000)));
        assert_eq!(usage.totals(["2026-10-20"])[0].ms, 10 * 60 * 1000);
        assert_eq!(usage.totals(["2026-10-21"])[0].ms, 10 * 60 * 1000);
    }

    #[test]
    fn a_stretch_inside_one_day_stays_on_it() {
        let start = Local
            .with_ymd_and_hms(2026, 10, 20, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let mut usage = AppUsage::default();
        assert!(record_span(&mut usage, &stretch(start, 90 * 60 * 1000)));
        assert_eq!(usage.totals(["2026-10-20"])[0].ms, 90 * 60 * 1000);
        assert!(usage.totals(["2026-10-21"]).is_empty());
    }

    #[test]
    fn impossible_stretches_are_refused() {
        let start = Local
            .with_ymd_and_hms(2026, 10, 20, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let mut usage = AppUsage::default();
        assert!(!record_span(&mut usage, &stretch(0, 1_000)));
        assert!(!record_span(&mut usage, &stretch(start, 0)));
        assert!(!record_span(&mut usage, &stretch(start, MAX_REPORT_MS + 1)));
        assert!(usage.is_empty());
    }
}
