// SPDX-License-Identifier: GPL-3.0-or-later

//! An account's calendars kept on this computer: what the agenda shows while offline, the edits waiting to
//! go out, and the sync that brings the two together with the server.

use std::collections::BTreeMap;
use std::thread;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{CalendarError, CalendarInfo, CalendarProvider, Event, EventDraft};
use crate::http::backoff;
use crate::ical;

/// Tries of a request the server was too busy for.
const MAX_BUSY_RETRIES: u32 = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CalendarState {
    pub info: Option<CalendarInfo>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub events: BTreeMap<String, Event>,
}

/// An edit made while it could not reach the server, or that has not gone out yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingEdit {
    Create {
        calendar: String,
        draft: PendingDraft,
    },
    Update {
        calendar: String,
        event: String,
        etag: String,
        draft: PendingDraft,
    },
    Delete {
        calendar: String,
        event: String,
        etag: String,
    },
}

/// [`EventDraft`] as saved to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDraft {
    pub title: String,
    pub location: String,
    pub description: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub time_zone: String,
    pub recurrence: Option<String>,
}

impl From<&EventDraft> for PendingDraft {
    fn from(draft: &EventDraft) -> Self {
        Self {
            title: draft.title.clone(),
            location: draft.location.clone(),
            description: draft.description.clone(),
            start_ms: draft.start_ms,
            end_ms: draft.end_ms,
            all_day: draft.all_day,
            time_zone: draft.time_zone.clone(),
            recurrence: draft.recurrence.clone(),
        }
    }
}

impl From<&PendingDraft> for EventDraft {
    fn from(draft: &PendingDraft) -> Self {
        Self {
            title: draft.title.clone(),
            location: draft.location.clone(),
            description: draft.description.clone(),
            start_ms: draft.start_ms,
            end_ms: draft.end_ms,
            all_day: draft.all_day,
            time_zone: draft.time_zone.clone(),
            recurrence: draft.recurrence.clone(),
        }
    }
}

/// An edit the server turned down because someone changed the event first; the user decides what next.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    pub edit: PendingEdit,
    pub title: String,
}

/// One account's local state, saved between runs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountCache {
    pub calendars: BTreeMap<String, CalendarState>,
    pub pending: Vec<PendingEdit>,
    pub conflicts: Vec<Conflict>,
    #[serde(default)]
    pub last_sync_ms: i64,
    /// Why the last sync did not finish, for the status line; empty when it did.
    #[serde(default)]
    pub last_error: String,
}

/// One thing on the agenda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaItem {
    /// The server's name for the event, and what an edit must quote.
    pub id: String,
    pub etag: String,
    pub calendar_id: String,
    pub title: String,
    pub location: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub calendar: String,
}

impl AccountCache {
    /// Everything happening in the window, soonest first, with repeats expanded.
    pub fn agenda(&self, from_ms: i64, to_ms: i64) -> Vec<AgendaItem> {
        let (Some(from), Some(to)) = (
            DateTime::<Utc>::from_timestamp_millis(from_ms),
            DateTime::<Utc>::from_timestamp_millis(to_ms),
        ) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        for (id, calendar) in &self.calendars {
            let name = calendar
                .info
                .as_ref()
                .map_or(id.as_str(), |info| info.name.as_str());
            for event in calendar.events.values() {
                match &event.ics {
                    Some(ics) => {
                        items.extend(
                            ical::occurrences(&ical::parse(ics), from, to)
                                .into_iter()
                                .map(|occurrence| AgendaItem {
                                    id: event.id.clone(),
                                    etag: event.etag.clone(),
                                    calendar_id: id.clone(),
                                    title: occurrence.title,
                                    location: occurrence.location,
                                    start_ms: occurrence.start.timestamp_millis(),
                                    end_ms: occurrence.end.timestamp_millis(),
                                    all_day: occurrence.all_day,
                                    calendar: name.to_owned(),
                                }),
                        );
                    }
                    None if event.start_ms < to_ms && event.end_ms > from_ms => {
                        items.push(AgendaItem {
                            id: event.id.clone(),
                            etag: event.etag.clone(),
                            calendar_id: id.clone(),
                            title: event.title.clone(),
                            location: event.location.clone(),
                            start_ms: event.start_ms,
                            end_ms: event.end_ms,
                            all_day: event.all_day,
                            calendar: name.to_owned(),
                        });
                    }
                    None => {}
                }
            }
        }
        items.sort_by_key(|item| (item.start_ms, item.title.clone()));
        items
    }

    /// Queues an edit; it goes out with the next [`sync`].
    pub fn queue(&mut self, edit: PendingEdit) {
        self.pending.push(edit);
    }
}

/// Runs `call`, waiting and trying again while the server says it is too busy.
fn patiently<T>(
    mut call: impl FnMut() -> Result<T, CalendarError>,
    sleep: &dyn Fn(std::time::Duration),
) -> Result<T, CalendarError> {
    let mut attempt = 0;
    loop {
        match call() {
            Err(CalendarError::Busy { retry_after }) if attempt < MAX_BUSY_RETRIES => {
                sleep(backoff(attempt, retry_after));
                attempt += 1;
            }
            other => return other,
        }
    }
}

/// Brings the cache and the server together: sends what is pending, then fetches what changed. On
/// failure what was done stays done, the error is kept in `last_error` and the rest waits for next time.
pub fn sync(
    provider: &dyn CalendarProvider,
    cache: &mut AccountCache,
) -> Result<(), CalendarError> {
    sync_with(provider, cache, &|wait| thread::sleep(wait))
}

fn sync_with(
    provider: &dyn CalendarProvider,
    cache: &mut AccountCache,
    sleep: &dyn Fn(std::time::Duration),
) -> Result<(), CalendarError> {
    let result = run(provider, cache, sleep);
    cache.last_error = result
        .as_ref()
        .err()
        .map_or_else(String::new, ToString::to_string);
    if result.is_ok() {
        cache.last_sync_ms = Utc::now().timestamp_millis();
    }
    result
}

fn run(
    provider: &dyn CalendarProvider,
    cache: &mut AccountCache,
    sleep: &dyn Fn(std::time::Duration),
) -> Result<(), CalendarError> {
    send_pending(provider, cache, sleep)?;
    let calendars = patiently(|| provider.calendars(), sleep)?;
    cache
        .calendars
        .retain(|id, _| calendars.iter().any(|calendar| &calendar.id == id));
    for calendar in calendars {
        let state = cache.calendars.entry(calendar.id.clone()).or_default();
        state.info = Some(calendar.clone());
        pull(provider, &calendar.id, state, sleep)?;
    }
    Ok(())
}

fn pull(
    provider: &dyn CalendarProvider,
    calendar: &str,
    state: &mut CalendarState,
    sleep: &dyn Fn(std::time::Duration),
) -> Result<(), CalendarError> {
    let token = state.token.clone();
    let result = match patiently(|| provider.sync(calendar, token.as_deref()), sleep) {
        Err(CalendarError::TokenExpired) if token.is_some() => {
            state.token = None;
            patiently(|| provider.sync(calendar, None), sleep)?
        }
        other => other?,
    };
    if result.full {
        state.events.clear();
    }
    for id in &result.removed {
        state.events.remove(id);
    }
    for event in result.changed {
        state.events.insert(event.id.clone(), event);
    }
    state.token = result.token;
    Ok(())
}

fn send_pending(
    provider: &dyn CalendarProvider,
    cache: &mut AccountCache,
    sleep: &dyn Fn(std::time::Duration),
) -> Result<(), CalendarError> {
    while let Some(edit) = cache.pending.first().cloned() {
        let done = match &edit {
            PendingEdit::Create { calendar, draft } => {
                patiently(|| provider.create(calendar, &draft.into()), sleep).map(drop)
            }
            PendingEdit::Update {
                calendar,
                event,
                etag,
                draft,
            } => patiently(
                || provider.update(calendar, event, etag, &draft.into()),
                sleep,
            )
            .map(drop),
            PendingEdit::Delete {
                calendar,
                event,
                etag,
            } => patiently(|| provider.delete(calendar, event, etag), sleep),
        };
        match done {
            Ok(()) => {
                cache.pending.remove(0);
            }
            Err(CalendarError::Conflict) => {
                let title = match &edit {
                    PendingEdit::Create { draft, .. } | PendingEdit::Update { draft, .. } => {
                        draft.title.clone()
                    }
                    PendingEdit::Delete { event, .. } => event.clone(),
                };
                cache.pending.remove(0);
                cache.conflicts.push(Conflict { edit, title });
            }
            // An event gone from the server has nothing left to edit.
            Err(CalendarError::NotFound) => {
                cache.pending.remove(0);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::super::SyncResult;
    use super::*;

    /// A calendar server in memory: a mock, for the rules here.
    #[derive(Default)]
    struct Fake {
        calendars: Vec<CalendarInfo>,
        sync_answers: Mutex<Vec<Result<SyncResult, CalendarError>>>,
        write_answers: Mutex<Vec<Result<(), CalendarError>>>,
        writes: Mutex<Vec<String>>,
        tokens_seen: Mutex<Vec<Option<String>>>,
    }

    fn event(id: &str, start_ms: i64) -> Event {
        Event {
            id: id.into(),
            etag: "e".into(),
            title: id.into(),
            location: String::new(),
            start_ms,
            end_ms: start_ms + 1000,
            all_day: false,
            ics: None,
        }
    }

    impl Fake {
        fn write(&self, what: &str) -> Result<Event, CalendarError> {
            self.writes.lock().unwrap().push(what.to_owned());
            let mut answers = self.write_answers.lock().unwrap();
            if !answers.is_empty() {
                answers.remove(0)?;
            }
            Ok(event("written", 0))
        }
    }

    impl CalendarProvider for Fake {
        fn calendars(&self) -> Result<Vec<CalendarInfo>, CalendarError> {
            Ok(self.calendars.clone())
        }
        fn sync(&self, _: &str, token: Option<&str>) -> Result<SyncResult, CalendarError> {
            self.tokens_seen
                .lock()
                .unwrap()
                .push(token.map(str::to_owned));
            self.sync_answers.lock().unwrap().remove(0)
        }
        fn create(&self, _: &str, draft: &EventDraft) -> Result<Event, CalendarError> {
            self.write(&format!("create {}", draft.title))
        }
        fn update(
            &self,
            _: &str,
            event: &str,
            _: &str,
            _: &EventDraft,
        ) -> Result<Event, CalendarError> {
            self.write(&format!("update {event}"))
        }
        fn delete(&self, _: &str, event: &str, _: &str) -> Result<(), CalendarError> {
            self.write(&format!("delete {event}")).map(drop)
        }
    }

    fn one_calendar() -> Vec<CalendarInfo> {
        vec![CalendarInfo {
            id: "c".into(),
            name: "Work".into(),
            read_only: false,
        }]
    }

    fn draft(title: &str) -> PendingDraft {
        PendingDraft {
            title: title.into(),
            location: String::new(),
            description: String::new(),
            start_ms: 0,
            end_ms: 1,
            all_day: false,
            time_zone: "UTC".into(),
            recurrence: None,
        }
    }

    fn no_wait(_: std::time::Duration) {}

    #[test]
    fn a_first_sync_fills_the_cache_and_keeps_the_token() {
        let fake = Fake {
            calendars: one_calendar(),
            sync_answers: Mutex::new(vec![Ok(SyncResult {
                changed: vec![event("a", 5), event("b", 1)],
                token: Some("t1".into()),
                full: true,
                ..SyncResult::default()
            })]),
            ..Fake::default()
        };
        let mut cache = AccountCache::default();
        sync_with(&fake, &mut cache, &no_wait).unwrap();
        assert_eq!(cache.calendars["c"].token.as_deref(), Some("t1"));
        let agenda = cache.agenda(0, 100);
        assert_eq!(agenda[0].title, "b", "soonest first");
        assert_eq!(agenda[1].calendar, "Work");
        assert!(cache.last_sync_ms > 0);
    }

    #[test]
    fn an_incremental_sync_changes_only_what_it_says() {
        let fake = Fake {
            calendars: one_calendar(),
            sync_answers: Mutex::new(vec![Ok(SyncResult {
                changed: vec![event("new", 1)],
                removed: vec!["old".into()],
                token: Some("t2".into()),
                full: false,
            })]),
            ..Fake::default()
        };
        let mut cache = AccountCache::default();
        let state = cache.calendars.entry("c".into()).or_default();
        state.token = Some("t1".into());
        state.events.insert("old".into(), event("old", 0));
        state.events.insert("kept".into(), event("kept", 0));
        sync_with(&fake, &mut cache, &no_wait).unwrap();
        let events = &cache.calendars["c"].events;
        assert!(events.contains_key("new") && events.contains_key("kept"));
        assert!(!events.contains_key("old"));
        assert_eq!(fake.tokens_seen.lock().unwrap()[0].as_deref(), Some("t1"));
    }

    #[test]
    fn an_expired_token_falls_back_to_a_full_sync_that_replaces_all() {
        let fake = Fake {
            calendars: one_calendar(),
            sync_answers: Mutex::new(vec![
                Err(CalendarError::TokenExpired),
                Ok(SyncResult {
                    changed: vec![event("fresh", 1)],
                    token: Some("t9".into()),
                    full: true,
                    ..SyncResult::default()
                }),
            ]),
            ..Fake::default()
        };
        let mut cache = AccountCache::default();
        let state = cache.calendars.entry("c".into()).or_default();
        state.token = Some("old".into());
        state.events.insert("stale".into(), event("stale", 0));
        sync_with(&fake, &mut cache, &no_wait).unwrap();
        let state = &cache.calendars["c"];
        assert!(!state.events.contains_key("stale"));
        assert_eq!(state.token.as_deref(), Some("t9"));
        assert_eq!(
            *fake.tokens_seen.lock().unwrap(),
            [Some("old".to_owned()), None]
        );
    }

    #[test]
    fn offline_edits_wait_and_what_is_cached_stays_readable() {
        let fake = Fake {
            calendars: one_calendar(),
            write_answers: Mutex::new(vec![Err(CalendarError::Offline("no network".into()))]),
            ..Fake::default()
        };
        let mut cache = AccountCache::default();
        cache
            .calendars
            .entry("c".into())
            .or_default()
            .events
            .insert("a".into(), event("a", 1));
        cache.queue(PendingEdit::Create {
            calendar: "c".into(),
            draft: draft("Lunch"),
        });
        assert!(sync_with(&fake, &mut cache, &no_wait).is_err());
        assert_eq!(cache.pending.len(), 1, "kept for next time");
        assert_eq!(cache.agenda(0, 100).len(), 1);
        assert!(cache.last_error.contains("no network"));
    }

    #[test]
    fn pending_edits_go_out_in_order_before_fetching() {
        let fake = Fake {
            calendars: one_calendar(),
            sync_answers: Mutex::new(vec![Ok(SyncResult::default())]),
            ..Fake::default()
        };
        let mut cache = AccountCache::default();
        cache.queue(PendingEdit::Create {
            calendar: "c".into(),
            draft: draft("One"),
        });
        cache.queue(PendingEdit::Delete {
            calendar: "c".into(),
            event: "e2".into(),
            etag: "x".into(),
        });
        sync_with(&fake, &mut cache, &no_wait).unwrap();
        assert_eq!(*fake.writes.lock().unwrap(), ["create One", "delete e2"]);
        assert!(cache.pending.is_empty());
    }

    #[test]
    fn a_conflict_is_set_aside_for_the_user_and_the_rest_goes_on() {
        let fake = Fake {
            calendars: one_calendar(),
            sync_answers: Mutex::new(vec![Ok(SyncResult::default())]),
            write_answers: Mutex::new(vec![Err(CalendarError::Conflict), Ok(())]),
            ..Fake::default()
        };
        let mut cache = AccountCache::default();
        cache.queue(PendingEdit::Update {
            calendar: "c".into(),
            event: "e1".into(),
            etag: "stale".into(),
            draft: draft("Changed"),
        });
        cache.queue(PendingEdit::Create {
            calendar: "c".into(),
            draft: draft("Other"),
        });
        sync_with(&fake, &mut cache, &no_wait).unwrap();
        assert_eq!(cache.conflicts.len(), 1);
        assert_eq!(cache.conflicts[0].title, "Changed");
        assert!(cache.pending.is_empty());
        assert_eq!(fake.writes.lock().unwrap().len(), 2);
    }

    #[test]
    fn a_busy_server_is_given_time_then_given_up_on() {
        let waited = Mutex::new(Vec::new());
        let sleep = |wait: std::time::Duration| waited.lock().unwrap().push(wait.as_secs());
        let mut calls = 0;
        let result: Result<(), CalendarError> = patiently(
            || {
                calls += 1;
                Err(CalendarError::Busy { retry_after: None })
            },
            &sleep,
        );
        assert_eq!(result, Err(CalendarError::Busy { retry_after: None }));
        assert_eq!(calls, MAX_BUSY_RETRIES + 1);
        assert_eq!(*waited.lock().unwrap(), [1, 2, 4]);
    }

    #[test]
    fn a_calendar_removed_from_the_account_goes() {
        let fake = Fake::default();
        let mut cache = AccountCache::default();
        cache.calendars.entry("gone".into()).or_default();
        sync_with(&fake, &mut cache, &no_wait).unwrap();
        assert!(cache.calendars.is_empty());
    }

    #[test]
    fn caldav_events_are_expanded_on_the_agenda() {
        let mut cache = AccountCache::default();
        let mut repeating = event("r", 0);
        repeating.ics = Some(
            "BEGIN:VEVENT\nUID:r\nSUMMARY:Standup\nDTSTART:20261001T100000Z\nDTEND:20261001T101500Z\nRRULE:FREQ=DAILY;COUNT=3\nEND:VEVENT\n".into(),
        );
        cache
            .calendars
            .entry("c".into())
            .or_default()
            .events
            .insert("r".into(), repeating);
        let from = DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        let to = DateTime::parse_from_rfc3339("2026-10-10T00:00:00Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(cache.agenda(from, to).len(), 3);
    }
}
