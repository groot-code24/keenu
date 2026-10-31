// SPDX-License-Identifier: GPL-3.0-or-later

//! iCalendar (RFC 5545) as calendar servers speak it: reading events with their time zones, expanding
//! recurring ones, and writing an event back.

use chrono::{
    DateTime, Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeZone,
    Utc, Weekday,
};
use chrono_tz::Tz;

/// When something happens, as the event wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// All day.
    Date(NaiveDate),
    Utc(DateTime<Utc>),
    Zoned(NaiveDateTime, Tz),
    /// Wherever the reader is.
    Floating(NaiveDateTime),
}

impl When {
    /// The moment it is; a date is the local midnight that starts it.
    pub fn to_utc(self) -> DateTime<Utc> {
        match self {
            Self::Date(date) => local_to_utc(date.and_time(NaiveTime::MIN)),
            Self::Utc(time) => time,
            Self::Zoned(time, zone) => zoned_to_utc(time, zone),
            Self::Floating(time) => local_to_utc(time),
        }
    }

    fn naive(self) -> NaiveDateTime {
        match self {
            Self::Date(date) => date.and_time(NaiveTime::MIN),
            Self::Utc(time) => time.naive_utc(),
            Self::Zoned(time, _) | Self::Floating(time) => time,
        }
    }

    /// The same kind of time at another wall-clock time: what makes a repeat keep its local hour.
    fn with_naive(self, time: NaiveDateTime) -> Self {
        match self {
            Self::Date(_) => Self::Date(time.date()),
            Self::Utc(_) => Self::Utc(time.and_utc()),
            Self::Zoned(_, zone) => Self::Zoned(time, zone),
            Self::Floating(_) => Self::Floating(time),
        }
    }

    pub fn is_date(self) -> bool {
        matches!(self, Self::Date(_))
    }
}

/// A local time to the moment it means. In a daylight-saving gap it moves on past the gap; in an
/// overlap it is the first of the two.
fn zoned_to_utc(time: NaiveDateTime, zone: Tz) -> DateTime<Utc> {
    (0..=3)
        .find_map(|hours| {
            zone.from_local_datetime(&(time + Duration::hours(hours)))
                .earliest()
        })
        .map_or_else(|| time.and_utc(), |found| found.with_timezone(&Utc))
}

fn local_to_utc(time: NaiveDateTime) -> DateTime<Utc> {
    (0..=3)
        .find_map(|hours| {
            Local
                .from_local_datetime(&(time + Duration::hours(hours)))
                .earliest()
        })
        .map_or_else(|| time.and_utc(), |found| found.with_timezone(&Utc))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// A repeat rule. Parts Keenu does not expand mark it `unsupported`, and only its first time shows:
/// better one right event than a wrong series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub frequency: Frequency,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<When>,
    /// Weekday, with its ordinal in the month or year (`-1` is the last), as in `2MO`.
    pub by_day: Vec<(Option<i32>, Weekday)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub unsupported: bool,
}

/// One event in a calendar object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VEvent {
    pub uid: String,
    pub summary: String,
    pub description: String,
    pub location: String,
    pub start: When,
    pub end: Option<When>,
    pub rule: Option<Rule>,
    pub exceptions: Vec<When>,
    /// Which time of a series this changes, when it is a changed instance.
    pub recurrence_id: Option<When>,
    pub cancelled: bool,
}

/// One time an event happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub uid: String,
    pub title: String,
    pub location: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
}

/// Every event in iCalendar text. Damaged events are skipped; the rest still count.
pub fn parse(text: &str) -> Vec<VEvent> {
    let mut events = Vec::new();
    let mut current: Option<Parts> = None;
    // Alarms and the like sit inside an event with properties of their own.
    let mut nested = 0;
    for line in unfold(text) {
        let Some((name, params, value)) = split_property(&line) else {
            continue;
        };
        match (name.as_str(), value) {
            ("BEGIN", "VEVENT") if current.is_none() => current = Some(Parts::default()),
            ("END", "VEVENT") if nested == 0 => {
                if let Some(event) = current.take().and_then(Parts::finish) {
                    events.push(event);
                }
            }
            ("BEGIN", _) if current.is_some() => nested += 1,
            ("END", _) if current.is_some() => nested -= 1,
            _ if nested == 0 => {
                if let Some(parts) = &mut current {
                    parts.apply(&name, &params, value);
                }
            }
            _ => {}
        }
    }
    events
}

#[derive(Default)]
struct Parts {
    uid: String,
    summary: String,
    description: String,
    location: String,
    start: Option<When>,
    end: Option<When>,
    duration: Option<Duration>,
    rule: Option<Rule>,
    exceptions: Vec<When>,
    recurrence_id: Option<When>,
    cancelled: bool,
}

impl Parts {
    fn apply(&mut self, name: &str, params: &[(String, String)], value: &str) {
        match name {
            "UID" => self.uid = value.to_owned(),
            "SUMMARY" => self.summary = unescape(value),
            "DESCRIPTION" => self.description = unescape(value),
            "LOCATION" => self.location = unescape(value),
            "DTSTART" => self.start = parse_when(params, value),
            "DTEND" => self.end = parse_when(params, value),
            "DURATION" => self.duration = parse_duration(value),
            "RRULE" => self.rule = parse_rule(value),
            "EXDATE" => self
                .exceptions
                .extend(value.split(',').filter_map(|one| parse_when(params, one))),
            "RECURRENCE-ID" => self.recurrence_id = parse_when(params, value),
            "STATUS" => self.cancelled = value.eq_ignore_ascii_case("CANCELLED"),
            _ => {}
        }
    }

    fn finish(self) -> Option<VEvent> {
        let start = self.start?;
        let end = self.end.or_else(|| {
            self.duration
                .map(|duration| start.with_naive(start.naive() + duration))
        });
        Some(VEvent {
            uid: self.uid,
            summary: self.summary,
            description: self.description,
            location: self.location,
            start,
            end,
            rule: self.rule,
            exceptions: self.exceptions,
            recurrence_id: self.recurrence_id,
            cancelled: self.cancelled,
        })
    }
}

/// Long lines continue on the next, after a space or a tab.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match line.strip_prefix([' ', '\t']) {
            Some(rest) if !lines.is_empty() => lines.last_mut().unwrap().push_str(rest),
            _ => lines.push(line.to_owned()),
        }
    }
    lines
}

/// A property's name, its parameters, and its value.
type Property<'a> = (String, Vec<(String, String)>, &'a str);

/// `NAME;PARAM=VALUE:value`. The first colon outside quotes ends the name.
fn split_property(line: &str) -> Option<Property<'_>> {
    let mut quoted = false;
    let colon = line.char_indices().find_map(|(at, c)| {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some(at),
            _ => {}
        }
        None
    })?;
    let mut parts = line[..colon].split(';');
    let name = parts.next()?.to_ascii_uppercase();
    let params = parts
        .filter_map(|part| part.split_once('='))
        .map(|(key, value)| (key.to_ascii_uppercase(), value.trim_matches('"').to_owned()))
        .collect();
    Some((name, params, &line[colon + 1..]))
}

fn parse_when(params: &[(String, String)], value: &str) -> Option<When> {
    let value = value.trim();
    let is_date = params
        .iter()
        .any(|(key, kind)| key == "VALUE" && kind.eq_ignore_ascii_case("DATE"));
    if is_date || value.len() == 8 {
        return NaiveDate::parse_from_str(value, "%Y%m%d")
            .ok()
            .map(When::Date);
    }
    if let Some(utc) = value.strip_suffix('Z') {
        let time = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(When::Utc(time.and_utc()));
    }
    let time = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S").ok()?;
    let zone = params
        .iter()
        .find(|(key, _)| key == "TZID")
        .and_then(|(_, name)| name.trim_start_matches('/').parse::<Tz>().ok());
    Some(match zone {
        Some(zone) => When::Zoned(time, zone),
        // No zone, or one we cannot name (some servers invent theirs): the reader's own.
        None => When::Floating(time),
    })
}

/// `P1DT2H30M` and the like.
fn parse_duration(value: &str) -> Option<Duration> {
    let (negative, rest) = match value.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, value.strip_prefix('+').unwrap_or(value)),
    };
    let rest = rest.strip_prefix('P')?;
    let mut total = Duration::zero();
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'T' => {}
            'W' | 'D' | 'H' | 'M' | 'S' => {
                let amount: i64 = number.parse().ok()?;
                number.clear();
                total += match c {
                    'W' => Duration::weeks(amount),
                    'D' => Duration::days(amount),
                    'H' => Duration::hours(amount),
                    'M' => Duration::minutes(amount),
                    _ => Duration::seconds(amount),
                };
            }
            _ => return None,
        }
    }
    Some(if negative { -total } else { total })
}

fn parse_weekday(value: &str) -> Option<Weekday> {
    Some(match value {
        "MO" => Weekday::Mon,
        "TU" => Weekday::Tue,
        "WE" => Weekday::Wed,
        "TH" => Weekday::Thu,
        "FR" => Weekday::Fri,
        "SA" => Weekday::Sat,
        "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn parse_rule(value: &str) -> Option<Rule> {
    let mut rule = Rule {
        frequency: Frequency::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        unsupported: false,
    };
    let mut frequency = None;
    for part in value.split(';') {
        let (key, value) = part.split_once('=')?;
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => {
                frequency = match value.to_ascii_uppercase().as_str() {
                    "DAILY" => Some(Frequency::Daily),
                    "WEEKLY" => Some(Frequency::Weekly),
                    "MONTHLY" => Some(Frequency::Monthly),
                    "YEARLY" => Some(Frequency::Yearly),
                    _ => {
                        rule.unsupported = true;
                        Some(Frequency::Daily)
                    }
                };
            }
            "INTERVAL" => rule.interval = value.parse().ok().filter(|&n| n > 0)?,
            "COUNT" => rule.count = value.parse().ok(),
            "UNTIL" => rule.until = parse_when(&[], value),
            "BYDAY" => {
                for day in value.split(',') {
                    let split = day.len().checked_sub(2)?;
                    let weekday = parse_weekday(&day[split..].to_ascii_uppercase())?;
                    let ordinal = match &day[..split] {
                        "" => None,
                        ordinal => Some(ordinal.parse().ok()?),
                    };
                    rule.by_day.push((ordinal, weekday));
                }
            }
            "BYMONTHDAY" => {
                rule.by_month_day = value
                    .split(',')
                    .filter_map(|day| day.parse().ok())
                    .collect();
            }
            "BYMONTH" => {
                rule.by_month = value
                    .split(',')
                    .filter_map(|month| month.parse().ok())
                    .collect();
            }
            // Weeks starting on another day only matter for an INTERVAL above one with BYDAY.
            "WKST" => {
                if !value.eq_ignore_ascii_case("MO") && rule.interval > 1 {
                    rule.unsupported = true;
                }
            }
            _ => rule.unsupported = true,
        }
    }
    rule.frequency = frequency?;
    Some(rule)
}

/// The most candidate times looked at for one event: a safety stop for rules that hardly ever match.
const MAX_STEPS: u32 = 20_000;

/// The start of each time `event` happens that overlaps `from`..`to`, in order.
pub fn starts_between(event: &VEvent, from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<When> {
    let length = duration_of(event);
    let overlaps = |start: DateTime<Utc>| start < to && start + length > from;
    let Some(rule) = event.rule.as_ref().filter(|rule| !rule.unsupported) else {
        return if overlaps(event.start.to_utc()) {
            vec![event.start]
        } else {
            Vec::new()
        };
    };
    let excluded: Vec<DateTime<Utc>> = event.exceptions.iter().map(|when| when.to_utc()).collect();
    let until = rule.until.map(When::to_utc);
    let mut found = Vec::new();
    let mut emitted = 0;
    let first = event.start.naive();
    let mut period = 0;
    let mut steps = 0;
    'periods: loop {
        let mut times = candidates(rule, first, period);
        times.sort_unstable();
        times.dedup();
        if times.is_empty() && period > 0 && !may_have_more(rule, first, period) {
            break;
        }
        for time in times {
            steps += 1;
            if steps > MAX_STEPS {
                break 'periods;
            }
            if time < first {
                continue;
            }
            let when = event.start.with_naive(time);
            let start = when.to_utc();
            if until.is_some_and(|until| start > until) || start >= to {
                break 'periods;
            }
            if rule.count.is_some_and(|count| emitted >= count) {
                break 'periods;
            }
            emitted += 1;
            if !excluded.contains(&start) && overlaps(start) {
                found.push(when);
            }
        }
        period += 1;
        // A series whose periods have long passed `to` has nothing left to say.
        if period > MAX_STEPS {
            break;
        }
    }
    found
}

fn may_have_more(rule: &Rule, first: NaiveDateTime, period: u32) -> bool {
    // A rule matching nothing this period may match later (31st of a month, leap days), up to its limits.
    let years = match rule.frequency {
        Frequency::Yearly => period * rule.interval,
        Frequency::Monthly => period * rule.interval / 12,
        Frequency::Weekly => period * rule.interval / 52,
        Frequency::Daily => period * rule.interval / 365,
    };
    first.year() + (years as i32) < 2200
}

fn duration_of(event: &VEvent) -> Duration {
    match event.end {
        Some(end) => end.to_utc() - event.start.to_utc(),
        None if event.start.is_date() => Duration::days(1),
        None => Duration::zero(),
    }
}

/// The wall-clock times period number `period` of the rule offers, unfiltered by count or end.
fn candidates(rule: &Rule, first: NaiveDateTime, period: u32) -> Vec<NaiveDateTime> {
    let steps = period.saturating_mul(rule.interval);
    let time = first.time();
    let date = first.date();
    let dates: Vec<NaiveDate> = match rule.frequency {
        Frequency::Daily => {
            let day = date + Duration::days(i64::from(steps));
            vec![day]
        }
        Frequency::Weekly => {
            let week = date.week(Weekday::Mon).first_day() + Duration::weeks(i64::from(steps));
            let days: Vec<Weekday> = if rule.by_day.is_empty() {
                vec![date.weekday()]
            } else {
                rule.by_day.iter().map(|(_, day)| *day).collect()
            };
            days.iter()
                .map(|day| week + Duration::days(i64::from(day.num_days_from_monday())))
                .collect()
        }
        Frequency::Monthly => month_days(
            rule,
            date.checked_add_months(Months::new(0)).unwrap_or(date),
            steps,
        ),
        Frequency::Yearly => {
            let year = date.year() + steps as i32;
            let months: Vec<u32> = if rule.by_month.is_empty() {
                vec![date.month()]
            } else {
                rule.by_month.clone()
            };
            months
                .iter()
                .filter_map(|&month| NaiveDate::from_ymd_opt(year, month, 1))
                .flat_map(|month_start| days_in_month(rule, month_start, date.day()))
                .collect()
        }
    };
    dates
        .into_iter()
        .filter(|day| rule.by_month.is_empty() || rule.by_month.contains(&day.month()))
        .filter(|day| match rule.frequency {
            // Other frequencies chose their days by these already.
            Frequency::Daily => {
                (rule.by_day.is_empty() || rule.by_day.iter().any(|(_, wd)| *wd == day.weekday()))
                    && (rule.by_month_day.is_empty()
                        || rule.by_month_day.contains(&(day.day() as i32)))
            }
            _ => true,
        })
        .map(|day| day.and_time(time))
        .collect()
}

fn month_days(rule: &Rule, date: NaiveDate, steps: u32) -> Vec<NaiveDate> {
    let Some(month_start) = date
        .with_day(1)
        .and_then(|start| start.checked_add_months(Months::new(steps)))
    else {
        return Vec::new();
    };
    days_in_month(rule, month_start, date.day())
}

/// The days of one month the rule picks; with no BYDAY or BYMONTHDAY, the same day as the first
/// time, and none in a month too short to have it.
fn days_in_month(rule: &Rule, month_start: NaiveDate, default_day: u32) -> Vec<NaiveDate> {
    let last = last_day_of_month(month_start);
    let mut days = Vec::new();
    for &day in &rule.by_month_day {
        let day = if day < 0 {
            i64::from(last) + 1 + i64::from(day)
        } else {
            i64::from(day)
        };
        if (1..=i64::from(last)).contains(&day) {
            days.extend(month_start.with_day(day as u32));
        }
    }
    for &(ordinal, weekday) in &rule.by_day {
        let all: Vec<NaiveDate> = (1..=last)
            .filter_map(|day| month_start.with_day(day))
            .filter(|day| day.weekday() == weekday)
            .collect();
        match ordinal {
            None => days.extend(&all),
            Some(ordinal) if ordinal > 0 => days.extend(all.get(ordinal as usize - 1)),
            Some(ordinal) => days.extend(
                all.len()
                    .checked_sub(ordinal.unsigned_abs() as usize)
                    .and_then(|at| all.get(at)),
            ),
        }
    }
    if rule.by_month_day.is_empty() && rule.by_day.is_empty() {
        days.extend(month_start.with_day(default_day));
    }
    days
}

fn last_day_of_month(month_start: NaiveDate) -> u32 {
    month_start
        .checked_add_months(Months::new(1))
        .and_then(|next| next.pred_opt())
        .map_or(28, |last| last.day())
}

/// Every time `events` (from one calendar object or many) happen in `from`..`to`, in order. A changed
/// instance replaces the time it changes; a cancelled event or instance does not happen.
pub fn occurrences(events: &[VEvent], from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<Occurrence> {
    let mut found = Vec::new();
    for event in events {
        if event.cancelled {
            continue;
        }
        let length = duration_of(event);
        let replaced: Vec<DateTime<Utc>> = events
            .iter()
            .filter(|other| other.uid == event.uid)
            .filter_map(|other| other.recurrence_id)
            .map(When::to_utc)
            .collect();
        for when in starts_between(event, from, to) {
            let start = when.to_utc();
            if event.recurrence_id.is_none() && replaced.contains(&start) {
                continue;
            }
            found.push(Occurrence {
                uid: event.uid.clone(),
                title: event.summary.clone(),
                location: event.location.clone(),
                start,
                end: start + length,
                all_day: event.start.is_date(),
            });
        }
    }
    found.sort_by_key(|occurrence| occurrence.start);
    found
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

/// Fold at 75 bytes, never inside a character, as the standard asks.
fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 8);
    let mut width = 0;
    for c in line.chars() {
        if width + c.len_utf8() > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += c.len_utf8();
    }
    out.push_str("\r\n");
    out
}

fn when_property(name: &str, when: When) -> String {
    match when {
        When::Date(date) => format!("{name};VALUE=DATE:{}", date.format("%Y%m%d")),
        When::Utc(time) => format!("{name}:{}", time.format("%Y%m%dT%H%M%SZ")),
        When::Zoned(time, zone) => format!(
            "{name};TZID={}:{}",
            zone.name(),
            time.format("%Y%m%dT%H%M%S")
        ),
        When::Floating(time) => format!("{name}:{}", time.format("%Y%m%dT%H%M%S")),
    }
}

/// `event` as a calendar object to `PUT`. `rrule` is written as given; it comes from Keenu's own form.
pub fn to_ics(event: &VEvent, rrule: Option<&str>) -> String {
    let mut out = String::from(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Keenu//Calendar//EN\r\nBEGIN:VEVENT\r\n",
    );
    let mut line = |text: String| out.push_str(&fold(&text));
    line(format!("UID:{}", event.uid));
    line(format!("DTSTAMP:{}", Utc::now().format("%Y%m%dT%H%M%SZ")));
    line(when_property("DTSTART", event.start));
    if let Some(end) = event.end {
        line(when_property("DTEND", end));
    }
    line(format!("SUMMARY:{}", escape(&event.summary)));
    if !event.location.is_empty() {
        line(format!("LOCATION:{}", escape(&event.location)));
    }
    if !event.description.is_empty() {
        line(format!("DESCRIPTION:{}", escape(&event.description)));
    }
    if let Some(rrule) = rrule {
        line(format!("RRULE:{rrule}"));
    }
    out.push_str("END:VEVENT\r\nEND:VCALENDAR\r\n");
    out
}

/// `original` with its main event changed to `event`, keeping whatever else the object holds: its UID,
/// attendees, alarms and changed instances, none of which Keenu shows but all of which are someone's data.
pub fn patch_ics(original: &str, event: &VEvent, rrule: Option<&str>) -> String {
    const REPLACED: [&str; 7] = [
        "SUMMARY",
        "DTSTART",
        "DTEND",
        "DURATION",
        "LOCATION",
        "DESCRIPTION",
        "RRULE",
    ];
    let lines = unfold(original);
    let mut out = String::with_capacity(original.len() + 128);
    let mut depth = 0;
    let mut in_event = false;
    // Each line of the event, and whether it is inside something nested, such as an alarm.
    let mut buffer: Vec<(String, bool)> = Vec::new();
    let new_lines = |out: &mut String| {
        let mut line = |text: String| out.push_str(&fold(&text));
        line(when_property("DTSTART", event.start));
        if let Some(end) = event.end {
            line(when_property("DTEND", end));
        }
        line(format!("SUMMARY:{}", escape(&event.summary)));
        if !event.location.is_empty() {
            line(format!("LOCATION:{}", escape(&event.location)));
        }
        if !event.description.is_empty() {
            line(format!("DESCRIPTION:{}", escape(&event.description)));
        }
        if let Some(rrule) = rrule {
            line(format!("RRULE:{rrule}"));
        }
    };
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let name = line
            .split([':', ';'])
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if !in_event {
            if line.eq_ignore_ascii_case("BEGIN:VEVENT") {
                in_event = true;
                depth = 0;
                buffer.clear();
                buffer.push((line, false));
            } else {
                out.push_str(&fold(&line));
            }
            continue;
        }
        let mut nested = depth > 0;
        if name == "BEGIN" {
            depth += 1;
            nested = true;
        } else if name == "END" && depth > 0 {
            depth -= 1;
        } else if line.eq_ignore_ascii_case("END:VEVENT") {
            // A changed instance of a series stays as it was; only the main event is rewritten.
            let main = !buffer
                .iter()
                .any(|(kept, _)| kept.to_ascii_uppercase().starts_with("RECURRENCE-ID"));
            for (kept, inside) in &buffer {
                let kept_name = kept
                    .split([':', ';'])
                    .next()
                    .unwrap_or_default()
                    .to_ascii_uppercase();
                if !main || *inside || !REPLACED.contains(&kept_name.as_str()) {
                    out.push_str(&fold(kept));
                }
            }
            if main {
                new_lines(&mut out);
            }
            out.push_str("END:VEVENT\r\n");
            in_event = false;
            continue;
        }
        buffer.push((line, nested));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn wrap(body: &str) -> String {
        format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:u1\r\nSUMMARY:Thing\r\n{body}END:VEVENT\r\nEND:VCALENDAR\r\n"
        )
    }

    fn event(body: &str) -> VEvent {
        parse(&wrap(body)).remove(0)
    }

    fn days(event: &VEvent, from: &str, to: &str) -> Vec<String> {
        starts_between(event, utc(from), utc(to))
            .iter()
            .map(|when| when.to_utc().format("%Y-%m-%d").to_string())
            .collect()
    }

    const WIDE: (&str, &str) = ("2020-01-01T00:00:00Z", "2030-01-01T00:00:00Z");

    #[test]
    fn reads_folded_escaped_lines_and_skips_alarms() {
        let events = parse(
            "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nSUMMARY:Long\r\n  title\\, with\\;marks\r\nDESCRIPTION:one\\ntwo\r\n\
             DTSTART:20261031T100000Z\r\nBEGIN:VALARM\r\nSUMMARY:Not the title\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        );
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].summary, "Long title, with;marks");
        assert_eq!(events[0].description, "one\ntwo");
    }

    #[test]
    fn a_damaged_event_does_not_lose_the_rest() {
        let text = "BEGIN:VEVENT\nUID:bad\nDTSTART:garbage\nEND:VEVENT\nBEGIN:VEVENT\nUID:good\nDTSTART:20261031T100000Z\nEND:VEVENT\n";
        let events = parse(text);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].uid, "good");
    }

    #[test]
    fn reads_each_kind_of_time() {
        let date = event("DTSTART;VALUE=DATE:20261031\r\n");
        assert!(date.start.is_date());
        let zoned = event("DTSTART;TZID=Asia/Kolkata:20261031T100000\r\n");
        assert_eq!(zoned.start.to_utc(), utc("2026-10-31T04:30:00Z"));
        let quoted = event("DTSTART;TZID=\"America/New_York\":20260701T090000\r\n");
        assert_eq!(quoted.start.to_utc(), utc("2026-07-01T13:00:00Z"));
        let unknown = event("DTSTART;TZID=Custom Zone:20261031T100000\r\n");
        assert!(matches!(unknown.start, When::Floating(_)));
    }

    #[test]
    fn duration_gives_an_end() {
        let with = event("DTSTART:20261031T100000Z\r\nDURATION:PT1H30M\r\n");
        assert_eq!(with.end.unwrap().to_utc(), utc("2026-10-31T11:30:00Z"));
    }

    #[test]
    fn a_daily_rule_with_count_and_exceptions() {
        let daily = event(
            "DTSTART:20261001T100000Z\r\nRRULE:FREQ=DAILY;COUNT=4\r\nEXDATE:20261002T100000Z\r\n",
        );
        assert_eq!(
            days(&daily, WIDE.0, WIDE.1),
            ["2026-10-01", "2026-10-03", "2026-10-04"],
            "an exception still counts towards COUNT"
        );
    }

    #[test]
    fn a_weekly_rule_on_chosen_days_with_interval() {
        // A Thursday; every second week on Mondays and Thursdays.
        let weekly = event(
            "DTSTART:20261001T100000Z\r\nRRULE:FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH;COUNT=4\r\n",
        );
        assert_eq!(
            days(&weekly, WIDE.0, WIDE.1),
            ["2026-10-01", "2026-10-12", "2026-10-15", "2026-10-26"]
        );
    }

    #[test]
    fn a_monthly_rule_skips_months_too_short() {
        let monthly = event("DTSTART:20260131T100000Z\r\nRRULE:FREQ=MONTHLY;COUNT=3\r\n");
        assert_eq!(
            days(&monthly, WIDE.0, WIDE.1),
            ["2026-01-31", "2026-03-31", "2026-05-31"]
        );
    }

    #[test]
    fn a_monthly_rule_by_weekday_and_from_the_end() {
        let second_tuesday =
            event("DTSTART:20261013T100000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=2TU;COUNT=3\r\n");
        assert_eq!(
            days(&second_tuesday, WIDE.0, WIDE.1),
            ["2026-10-13", "2026-11-10", "2026-12-08"]
        );
        let last_friday =
            event("DTSTART:20261030T100000Z\r\nRRULE:FREQ=MONTHLY;BYDAY=-1FR;COUNT=2\r\n");
        assert_eq!(
            days(&last_friday, WIDE.0, WIDE.1),
            ["2026-10-30", "2026-11-27"]
        );
        let last_day =
            event("DTSTART:20261031T100000Z\r\nRRULE:FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=2\r\n");
        assert_eq!(
            days(&last_day, WIDE.0, WIDE.1),
            ["2026-10-31", "2026-11-30"]
        );
    }

    #[test]
    fn a_yearly_rule_keeps_a_birthday_on_leap_days_only() {
        let yearly = event("DTSTART;VALUE=DATE:20280229\r\nRRULE:FREQ=YEARLY;COUNT=2\r\n");
        let found: Vec<When> = starts_between(
            &yearly,
            utc("2028-01-01T00:00:00Z"),
            utc("2040-01-01T00:00:00Z"),
        );
        assert_eq!(
            found,
            [
                When::Date(NaiveDate::from_ymd_opt(2028, 2, 29).unwrap()),
                When::Date(NaiveDate::from_ymd_opt(2032, 2, 29).unwrap())
            ]
        );
    }

    #[test]
    fn until_ends_a_series() {
        let until =
            event("DTSTART:20261001T100000Z\r\nRRULE:FREQ=DAILY;UNTIL=20261003T100000Z\r\n");
        assert_eq!(days(&until, WIDE.0, WIDE.1).len(), 3);
    }

    #[test]
    fn a_repeat_keeps_its_local_hour_across_daylight_saving() {
        // 09:00 in New York: 13:00 UTC in summer, 14:00 UTC after the clocks change on 1 November 2026.
        let weekly =
            event("DTSTART;TZID=America/New_York:20261026T090000\r\nRRULE:FREQ=WEEKLY;COUNT=2\r\n");
        let found = starts_between(&weekly, utc(WIDE.0), utc(WIDE.1));
        assert_eq!(found[0].to_utc(), utc("2026-10-26T13:00:00Z"));
        assert_eq!(found[1].to_utc(), utc("2026-11-02T14:00:00Z"));
    }

    #[test]
    fn a_time_inside_a_daylight_saving_gap_moves_past_it() {
        // 02:30 does not exist in New York on 8 March 2026.
        let gap = event("DTSTART;TZID=America/New_York:20260308T023000\r\n");
        assert_eq!(gap.start.to_utc(), utc("2026-03-08T07:30:00Z"));
    }

    #[test]
    fn only_times_in_the_window_come_back_and_an_endless_rule_ends() {
        let daily = event("DTSTART:20200101T100000Z\r\nRRULE:FREQ=DAILY\r\n");
        assert_eq!(
            days(&daily, "2026-10-31T00:00:00Z", "2026-11-02T00:00:00Z"),
            ["2026-10-31", "2026-11-01"]
        );
    }

    #[test]
    fn an_event_still_running_at_the_window_counts() {
        let long = event("DTSTART:20261030T100000Z\r\nDTEND:20261101T100000Z\r\n");
        assert_eq!(
            days(&long, "2026-10-31T00:00:00Z", "2026-10-31T12:00:00Z").len(),
            1
        );
    }

    #[test]
    fn a_rule_it_cannot_expand_shows_only_its_first_time() {
        let hard =
            event("DTSTART:20261001T100000Z\r\nRRULE:FREQ=MONTHLY;BYSETPOS=-1;BYDAY=MO,TU\r\n");
        assert!(hard.rule.as_ref().unwrap().unsupported);
        assert_eq!(days(&hard, WIDE.0, WIDE.1), ["2026-10-01"]);
    }

    #[test]
    fn a_changed_instance_replaces_its_time_and_cancelled_ones_are_gone() {
        let text = "BEGIN:VEVENT\nUID:s\nSUMMARY:Series\nDTSTART:20261001T100000Z\nDTEND:20261001T110000Z\nRRULE:FREQ=DAILY;COUNT=3\nEND:VEVENT\n\
                    BEGIN:VEVENT\nUID:s\nSUMMARY:Moved\nRECURRENCE-ID:20261002T100000Z\nDTSTART:20261002T150000Z\nDTEND:20261002T160000Z\nEND:VEVENT\n\
                    BEGIN:VEVENT\nUID:gone\nSUMMARY:Off\nSTATUS:CANCELLED\nDTSTART:20261001T100000Z\nEND:VEVENT\n";
        let found = occurrences(&parse(text), utc(WIDE.0), utc(WIDE.1));
        let summary: Vec<(String, String)> = found
            .iter()
            .map(|o| (o.title.clone(), o.start.format("%d %H").to_string()))
            .collect();
        assert_eq!(
            summary,
            [
                ("Series".to_owned(), "01 10".to_owned()),
                ("Moved".to_owned(), "02 15".to_owned()),
                ("Series".to_owned(), "03 10".to_owned()),
            ]
        );
    }

    #[test]
    fn changing_an_event_keeps_what_keenu_does_not_show() {
        let original = "BEGIN:VCALENDAR\r\nPRODID:-//Other//EN\r\nBEGIN:VEVENT\r\nUID:server-uid\r\nSUMMARY:Old\r\nDTSTART:20261001T100000Z\r\nDTEND:20261001T110000Z\r\nATTENDEE;CN=Ana:mailto:ana@example.org\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nDESCRIPTION:Alarm text\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:server-uid\r\nRECURRENCE-ID:20261002T100000Z\r\nSUMMARY:Moved one\r\nDTSTART:20261002T150000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let mut changed = parse(original).remove(0);
        changed.summary = "New title".into();
        let patched = patch_ics(original, &changed, Some("FREQ=DAILY;COUNT=2"));
        assert!(patched.contains("UID:server-uid"));
        assert!(
            patched.contains("ATTENDEE;CN=Ana:mailto:ana@example.org"),
            "{patched}"
        );
        assert!(
            patched.contains("BEGIN:VALARM") && patched.contains("Alarm text"),
            "{patched}"
        );
        assert!(patched.contains("PRODID:-//Other//EN"));
        assert!(patched.contains("SUMMARY:New title"));
        assert!(!patched.contains("SUMMARY:Old"));
        assert!(
            patched.contains("SUMMARY:Moved one"),
            "a changed instance stays"
        );
        let events = parse(&patched);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].summary, "New title");
        assert_eq!(events[0].rule.as_ref().unwrap().count, Some(2));
        assert_eq!(events[1].summary, "Moved one");
    }

    #[test]
    fn what_it_writes_it_can_read_back() {
        let original = event(
            "DTSTART;TZID=Asia/Kolkata:20261031T100000\r\nDTEND;TZID=Asia/Kolkata:20261031T110000\r\nLOCATION:Room\\, 4\r\n",
        );
        let mut written = original.clone();
        written.summary = "A very long title that goes on and on, past the seventy-five bytes one line may hold; é".into();
        let text = to_ics(&written, Some("FREQ=WEEKLY;COUNT=2"));
        for line in text.split("\r\n") {
            assert!(line.len() <= 75, "{line}");
        }
        let back = parse(&text).remove(0);
        assert_eq!(back.summary, written.summary);
        assert_eq!(back.location, "Room, 4");
        assert_eq!(back.start, written.start);
        assert_eq!(back.end, written.end);
        assert_eq!(back.rule.unwrap().count, Some(2));
    }
}
