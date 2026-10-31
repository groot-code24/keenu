// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Keenu's own calendar events, written in one line ("Dinner 8:00 pm").
//! Times are local wall-clock times; desktop calendars come from GNOME Shell.

use serde::{Deserialize, Serialize};

/// Events this long ago are forgotten.
pub const KEEP_DAYS: i64 = 400;
/// At most this many events are kept; adding more drops the oldest.
pub const CAPACITY: usize = 2000;
/// A title is cut to this many characters.
const MAX_TITLE_CHARS: usize = 200;

/// One event of Keenu's own, as saved and as sent to the extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: u64,
    pub title: String,
    /// The day, as "YYYY-MM-DD".
    pub date: String,
    /// Start time as "HH:MM"; `None` for an all-day event.
    pub start: Option<String>,
    /// End time on the same day as "HH:MM", when one was given.
    pub end: Option<String>,
    /// A desktop calendar it waits to go to (`{calendar, uid}`), kept here while that one is out of
    /// reach; the same iCalendar uid each try, so it never lands there twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send: Option<Send>,
}

/// Where an event of Keenu's waits to be sent (see [`Event::send`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Send {
    /// The desktop calendar's uid.
    pub calendar: String,
    /// The event's iCalendar uid there.
    pub uid: String,
}

/// All of Keenu's own events, kept sorted, with ids that never repeat.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Calendar {
    events: Vec<Event>,
    next_id: u64,
}

/// Why an event was not added: a day that does not exist, or nothing left but a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddError {
    BadDate,
    NoTitle,
}

impl Calendar {
    /// The events, by day and then start time (all-day events first).
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Adds an event on `date` ("YYYY-MM-DD") from a line of text, taking
    /// its time from the text (see [`parse`]). Returns the new id.
    pub fn add(&mut self, date: &str, text: &str) -> Result<u64, AddError> {
        let day = Day::parse(date).ok_or(AddError::BadDate)?;
        let parsed = parse(text).ok_or(AddError::NoTitle)?;
        self.next_id += 1;
        self.events.push(Event {
            id: self.next_id,
            title: parsed.title,
            date: day.to_string(),
            start: parsed.start.map(format_time),
            end: parsed.end.map(format_time),
            send: None,
        });
        self.sort();
        if self.events.len() > CAPACITY {
            self.events.drain(..self.events.len() - CAPACITY);
        }
        Ok(self.next_id)
    }

    /// Sets (or, with `None`, drops) where an event waits to be sent; returns whether it is there.
    pub fn set_send(&mut self, id: u64, send: Option<Send>) -> bool {
        let found = self.events.iter_mut().find(|event| event.id == id);
        found.map(|event| event.send = send).is_some()
    }

    /// The events waiting to be sent to a desktop calendar.
    pub fn waiting(&self) -> impl Iterator<Item = &Event> {
        self.events.iter().filter(|event| event.send.is_some())
    }

    /// Deletes an event; returns whether it was there.
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.events.len();
        self.events.retain(|event| event.id != id);
        self.events.len() != before
    }

    /// Forgets events more than [`KEEP_DAYS`] before `today`
    /// ("YYYY-MM-DD"). Returns whether any were dropped.
    pub fn forget_old(&mut self, today: &str) -> bool {
        let Some(today) = Day::parse(today) else {
            return false;
        };
        let before = self.events.len();
        self.events.retain(|event| {
            Day::parse(&event.date).is_none_or(|day| today.number() - day.number() <= KEEP_DAYS)
        });
        self.events.len() != before
    }

    /// Puts events in day and start order, ties by when they were added.
    fn sort(&mut self) {
        // "HH:MM" sorts as text; `None` (all day) sorts first.
        self.events
            .sort_by(|a, b| (&a.date, &a.start, a.id).cmp(&(&b.date, &b.start, b.id)));
    }
}

/// A calendar day, checked to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Day {
    year: i64,
    month: i64,
    day: i64,
}

impl Day {
    /// Reads "YYYY-MM-DD" exactly; `None` for a day the month does not have.
    fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('-');
        let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
            return None;
        }
        let (year, month, day) = (year.parse().ok()?, month.parse().ok()?, day.parse().ok()?);
        let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return None,
        };
        (1..=days)
            .contains(&day)
            .then_some(Self { year, month, day })
    }

    /// Days since 1970-01-01 (Howard Hinnant's `days_from_civil`).
    fn number(self) -> i64 {
        let year = if self.month <= 2 {
            self.year - 1
        } else {
            self.year
        };
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let month = (self.month + 9) % 12;
        let day_of_year = (153 * month + 2) / 5 + self.day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }
}

impl std::fmt::Display for Day {
    /// Back to "YYYY-MM-DD", zero-padded.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// A line of text read as an event: its title, and the times in it, in
/// minutes after midnight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub title: String,
    pub start: Option<u32>,
    pub end: Option<u32>,
}

/// Words that may come before a time ("at 9", "jam 14.00") and are dropped
/// with it.
const BEFORE_TIME: &[&str] = &["at", "@", "from", "jam", "pukul", "pkl", "dari", "mulai"];
/// Words between the start and end of a range.
const RANGE_WORDS: &[&str] = &[
    "-", "–", "—", "to", "until", "till", "sampai", "hingga", "s/d",
];

/// Reads "Dinner 8:00 pm", "Standup 9:00-9:15", "Gym at 7pm" or "Holiday" (all day):
/// the first time found, the rest as the title. `None` when no title is left.
pub fn parse(text: &str) -> Option<Parsed> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut start = None;
    let mut end = None;
    let mut used = vec![false; words.len()];

    for i in 0..words.len() {
        let after_word = i
            .checked_sub(1)
            .is_some_and(|before| BEFORE_TIME.contains(&words[before].to_lowercase().as_str()));
        let Some(found) = time_at(&words, i, after_word) else {
            continue;
        };
        let mut last = found.end;
        let mut next = i + found.words;
        if last.is_none()
            && let Some(word) = words.get(next)
            && RANGE_WORDS.contains(&word.to_lowercase().as_str())
            && let Some(TimeWords {
                start: second,
                end: None,
                words,
            }) = time_at(&words, next + 1, true)
        {
            last = Some(second);
            next += 1 + words;
        }
        start = Some(found.start);
        // An end before the start is not meant as one.
        end = last.filter(|&last| last > found.start);
        let from = if after_word { i - 1 } else { i };
        used[from..next].iter_mut().for_each(|used| *used = true);
        break;
    }

    let title: String = words
        .iter()
        .zip(&used)
        .filter(|(_, used)| !**used)
        .map(|(word, _)| *word)
        .collect::<Vec<_>>()
        .join(" ");
    let title = title.trim_matches(|c: char| c == ',' || c == '-' || c.is_whitespace());
    if title.is_empty() {
        return None;
    }
    Some(Parsed {
        title: title.chars().take(MAX_TITLE_CHARS).collect(),
        start,
        end,
    })
}

/// A time found in the text, in minutes after midnight, and how many words
/// it took.
struct TimeWords {
    start: u32,
    end: Option<u32>,
    words: usize,
}

/// A time (or a range written as one word, "9-10am") starting at word `i`.
/// A bare hour counts only when `bare_ok`.
fn time_at(words: &[&str], i: usize, bare_ok: bool) -> Option<TimeWords> {
    let all = words;
    let word = words.get(i)?.to_lowercase();
    let word = word.trim_end_matches([',', ';']);
    // "pm" as a word of its own: "7 pm".
    let suffix = words
        .get(i + 1)
        .map(|next| next.to_lowercase())
        .filter(|next| next == "am" || next == "pm");
    let text = match &suffix {
        Some(suffix) => format!("{word}{suffix}"),
        None => word.to_owned(),
    };
    let words = if suffix.is_some() { 2 } else { 1 };
    // "1800" as 18:00: where a bare hour is, or last ("Dinner 1930", "Shift 1800 - 2200"), but not a
    // year ("Reunion 2015") or a number in the title ("Room 1234 meeting").
    let last = all
        .get(i + words)
        .is_none_or(|next| RANGE_WORDS.contains(&next.to_lowercase().as_str()));
    let four_ok = bare_ok
        || (last
            && text
                .parse::<u32>()
                .is_ok_and(|n| !(2000..2060).contains(&n) && n % 5 == 0));

    // A range in one word: "9:00-9:15", "9-10am", "14.00–15.30"; not "1-2".
    if let Some((first, second)) = text.split_once(['-', '–', '—']) {
        let precise = |part: &str| part.contains([':', '.']) || part.ends_with('m');
        if first.is_empty() || second.is_empty() || !(precise(first) || precise(second)) {
            return None;
        }
        let end = clock(second, true, false)?;
        // "7-9pm": the end's am/pm goes for the start too, unless that puts
        // the start after the end ("11-1pm").
        let meridiem = ["am", "pm"].into_iter().find(|m| second.ends_with(m));
        let start = match meridiem {
            Some(meridiem) if !first.ends_with('m') => {
                clock(&format!("{first}{meridiem}"), true, false)
                    .filter(|&start| start < end)
                    .or_else(|| clock(first, true, false))
            }
            _ => clock(first, true, false),
        }?;
        return Some(TimeWords {
            start,
            end: Some(end),
            words,
        });
    }
    Some(TimeWords {
        start: clock(&text, bare_ok, four_ok)?,
        end: None,
        words,
    })
}

/// Minutes after midnight for "13:30", "13.30", "1:30pm", "1pm", "12am",
/// a bare hour ("13") when `bare_ok`, or four digits ("1330") when `four_ok`.
fn clock(text: &str, bare_ok: bool, four_ok: bool) -> Option<u32> {
    let (digits, meridiem) = if let Some(rest) = text.strip_suffix("am") {
        (rest, Some(false))
    } else if let Some(rest) = text.strip_suffix("pm") {
        (rest, Some(true))
    } else {
        (text, None)
    };
    let (hour, minute) = match digits.split_once([':', '.']) {
        Some((hour, minute)) if minute.len() == 2 => (hour, Some(minute)),
        Some(_) => return None,
        None if four_ok && digits.len() == 4 && digits.bytes().all(|b| b.is_ascii_digit()) => {
            (&digits[..2], Some(&digits[2..]))
        }
        None => (digits, None),
    };
    if hour.is_empty() || hour.len() > 2 || !hour.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if minute.is_none() && meridiem.is_none() && !bare_ok {
        return None;
    }
    let minute: u32 = match minute {
        Some(minute) if minute.bytes().all(|b| b.is_ascii_digit()) => minute.parse().ok()?,
        Some(_) => return None,
        None => 0,
    };
    let mut hour: u32 = hour.parse().ok()?;
    match meridiem {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour = hour % 12 + if pm { 12 } else { 0 };
        }
        None if hour > 23 => return None,
        None => {}
    }
    (minute < 60).then_some(hour * 60 + minute)
}

/// Minutes after midnight as "HH:MM".
pub fn format_time(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> (String, Option<String>, Option<String>) {
        let parsed = parse(text).expect("a title");
        (
            parsed.title,
            parsed.start.map(format_time),
            parsed.end.map(format_time),
        )
    }

    fn event(
        title: &str,
        start: Option<&str>,
        end: Option<&str>,
    ) -> (String, Option<String>, Option<String>) {
        (title.into(), start.map(Into::into), end.map(Into::into))
    }

    #[test]
    fn reads_times_in_many_forms() {
        assert_eq!(
            read("Lunch with Sara 12:30"),
            event("Lunch with Sara", Some("12:30"), None)
        );
        assert_eq!(read("12.30 lunch"), event("lunch", Some("12:30"), None));
        assert_eq!(read("Dinner 8:00 pm"), event("Dinner", Some("20:00"), None));
        assert_eq!(
            read("DINNER 08.00 PM"),
            event("DINNER", Some("20:00"), None)
        );
        assert_eq!(read("Dinner 8pm"), event("Dinner", Some("20:00"), None));
        assert_eq!(read("Gym at 7pm"), event("Gym", Some("19:00"), None));
        assert_eq!(read("Gym 7 pm"), event("Gym", Some("19:00"), None));
        assert_eq!(read("Call 9:45am"), event("Call", Some("09:45"), None));
        assert_eq!(
            read("Midnight snack 12am"),
            event("Midnight snack", Some("00:00"), None)
        );
        assert_eq!(read("Noon 12pm"), event("Noon", Some("12:00"), None));
        assert_eq!(read("Rapat jam 14"), event("Rapat", Some("14:00"), None));
        assert_eq!(read("testing 1800"), event("testing", Some("18:00"), None));
        assert_eq!(read("Dinner 1930"), event("Dinner", Some("19:30"), None));
        assert_eq!(
            read("Futsal jam 2000"),
            event("Futsal", Some("20:00"), None)
        );
        assert_eq!(
            read("Rapat pukul 0915"),
            event("Rapat", Some("09:15"), None)
        );
        assert_eq!(
            read("Rapat pukul 09.15"),
            event("Rapat", Some("09:15"), None)
        );
    }

    #[test]
    fn reads_ranges() {
        assert_eq!(
            read("Standup 9:00-9:15"),
            event("Standup", Some("09:00"), Some("09:15"))
        );
        assert_eq!(
            read("Standup 9:00 - 9:15"),
            event("Standup", Some("09:00"), Some("09:15"))
        );
        assert_eq!(
            read("Class 9-10am"),
            event("Class", Some("09:00"), Some("10:00"))
        );
        assert_eq!(
            read("Dinner 7-9pm"),
            event("Dinner", Some("19:00"), Some("21:00"))
        );
        assert_eq!(
            read("Lunch 11-1pm"),
            event("Lunch", Some("11:00"), Some("13:00"))
        );
        assert_eq!(
            read("Rapat jam 14.00 sampai 15.30"),
            event("Rapat", Some("14:00"), Some("15:30"))
        );
        assert_eq!(
            read("Workshop 13:00 to 17:00"),
            event("Workshop", Some("13:00"), Some("17:00"))
        );
        // An end before the start is dropped, not swapped.
        assert_eq!(read("Late 22:00-01:00"), event("Late", Some("22:00"), None));
        assert_eq!(
            read("Shift 1800 - 2200"),
            event("Shift", Some("18:00"), Some("22:00"))
        );
    }

    #[test]
    fn leaves_other_numbers_in_the_title() {
        assert_eq!(read("Holiday"), event("Holiday", None, None));
        assert_eq!(read("Buy 3 apples"), event("Buy 3 apples", None, None));
        assert_eq!(read("Buy 1-2 apples"), event("Buy 1-2 apples", None, None));
        assert_eq!(
            read("Season 2023-2024"),
            event("Season 2023-2024", None, None)
        );
        assert_eq!(
            read("Room 101 meeting"),
            event("Room 101 meeting", None, None)
        );
        assert_eq!(read("Score 25:99"), event("Score 25:99", None, None));
        assert_eq!(read("Release v1.2"), event("Release v1.2", None, None));
        // Four digits read as a time only last, and never as a year or an odd minute.
        assert_eq!(read("Reunion 2015"), event("Reunion 2015", None, None));
        assert_eq!(read("Expo 2030"), event("Expo 2030", None, None));
        assert_eq!(
            read("Room 1234 meeting"),
            event("Room 1234 meeting", None, None)
        );
        assert_eq!(read("Code 1234"), event("Code 1234", None, None));
        assert_eq!(read("Score 2575"), event("Score 2575", None, None));
        // Only the first time is read.
        assert_eq!(
            read("Meet 10:00 then 11:00"),
            event("Meet then 11:00", Some("10:00"), None)
        );
    }

    #[test]
    fn needs_a_title() {
        assert_eq!(parse("  "), None);
        assert_eq!(parse("10:00"), None);
        assert_eq!(parse("at 9pm"), None);
    }

    #[test]
    fn events_wait_to_be_sent_and_old_saves_still_load() {
        let mut calendar = Calendar::default();
        let id = calendar.add("2026-10-27", "Dinner 19:00").unwrap();
        assert_eq!(calendar.waiting().count(), 0);
        let send = Send {
            calendar: "google".into(),
            uid: "keenu-1".into(),
        };
        assert!(calendar.set_send(id, Some(send.clone())));
        assert_eq!(calendar.waiting().next().unwrap().send, Some(send));
        assert!(calendar.set_send(id, None));
        assert!(!calendar.set_send(id + 1, None));
        assert_eq!(calendar.waiting().count(), 0);

        let saved = r#"{"events":[{"id":1,"title":"Trip","date":"2026-10-21","start":null,"end":null}],"next_id":1}"#;
        let loaded: Calendar = serde_json::from_str(saved).unwrap();
        assert_eq!(loaded.events()[0].send, None);
        assert!(!serde_json::to_string(&loaded).unwrap().contains("send"));
    }

    #[test]
    fn checks_dates() {
        assert!(Day::parse("2024-02-29").is_some());
        assert!(Day::parse("2026-02-29").is_none());
        assert!(Day::parse("2026-13-01").is_none());
        assert!(Day::parse("2026-1-01").is_none());
        assert!(Day::parse("26-01-01").is_none());
        assert_eq!(Day::parse("1970-01-01").unwrap().number(), 0);
        assert_eq!(Day::parse("2026-10-21").unwrap().number(), 20_747);
    }

    #[test]
    fn adds_sorted_and_forgets_old_ones() {
        let mut calendar = Calendar::default();
        let late = calendar.add("2026-10-21", "Dinner 19:00").unwrap();
        calendar.add("2026-10-21", "Trip").unwrap();
        calendar.add("2026-10-20", "Call 9:00").unwrap();
        calendar.add("2025-01-01", "Long ago").unwrap();
        let order: Vec<_> = calendar.events().iter().map(|e| e.title.as_str()).collect();
        assert_eq!(order, ["Long ago", "Call", "Trip", "Dinner"]);
        assert_eq!(calendar.add("2026-02-30", "Nope"), Err(AddError::BadDate));
        assert_eq!(calendar.add("2026-10-21", "9pm"), Err(AddError::NoTitle));

        assert!(calendar.forget_old("2026-10-21"));
        assert_eq!(calendar.events().len(), 3);
        assert!(calendar.remove(late));
        assert!(!calendar.remove(late));
    }
}
