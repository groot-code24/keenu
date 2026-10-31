// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Wall-clock time and waiting for wall-clock deadlines.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Local, NaiveDate, TimeZone};

use tokio::sync::Notify;

/// Longest single sleep. Tokio sleeps on the monotonic clock, which stops during suspend: waits are
/// re-checked on waking from it (`resumed`), and at least this often in case that was missed.
const MAX_NAP: Duration = Duration::from_secs(60 * 60);

/// Wakes every wait to re-check the wall clock.
static RESUMED: Notify = Notify::const_new();

/// The computer woke from suspend: deadlines passed meanwhile are due now.
pub fn resumed() {
    RESUMED.notify_waiters();
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as i64)
}

/// Waits until the wall-clock `deadline` passes or `wake` is notified.
/// Returns whether it passed; on `false`, re-read the state and call again.
pub async fn wait_until(deadline: Option<i64>, wake: &Notify) -> bool {
    let notified = wake.notified();
    let Some(deadline) = deadline else {
        notified.await;
        return false;
    };
    let remaining = deadline - now_ms();
    if remaining <= 0 {
        return true;
    }
    let nap = Duration::from_millis(remaining as u64).min(MAX_NAP);
    tokio::select! {
        () = tokio::time::sleep(nap) => now_ms() >= deadline,
        () = RESUMED.notified() => now_ms() >= deadline,
        () = notified => false,
    }
}

/// Today's date in local time, e.g. "2026-10-20".
pub fn today() -> String {
    Local::now().date_naive().to_string()
}

/// The last `count` days in local time, oldest first and today last.
pub fn days_back(count: u32) -> Vec<String> {
    let today = Local::now().date_naive();
    (0..count)
        .rev()
        .map(|back| (today - chrono::Duration::days(i64::from(back))).to_string())
        .collect()
}

/// Start of today in local time, in milliseconds since the Unix epoch.
pub fn start_of_today() -> i64 {
    local_midnight(Local::now().date_naive()).unwrap_or(0)
}

/// The coming midnight in local time, for refreshing "today" figures.
pub fn next_midnight() -> Option<i64> {
    local_midnight(Local::now().date_naive().succ_opt()?)
}

/// Local midnight starting `date`, in milliseconds since the Unix epoch.
pub(crate) fn local_midnight(date: NaiveDate) -> Option<i64> {
    // `earliest` resolves times made ambiguous by daylight saving changes; where the clock skips midnight
    // (Chile, Cuba, Lebanon), the day starts at its first time that exists.
    let start = date.and_hms_opt(0, 0, 0)?;
    (0..=8).find_map(|quarter| {
        Local
            .from_local_datetime(&(start + chrono::Duration::minutes(15 * quarter)))
            .earliest()
            .map(|time| time.timestamp_millis())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_whose_midnight_is_skipped_still_starts() {
        // Chile skips 00:00-01:00 on 6 September 2026; run with TZ=America/Santiago to see the gap.
        let day = NaiveDate::from_ymd_opt(2026, 9, 6).unwrap();
        let start = local_midnight(day).unwrap();
        let before = local_midnight(day.pred_opt().unwrap()).unwrap();
        assert!(start > before);
    }
}
