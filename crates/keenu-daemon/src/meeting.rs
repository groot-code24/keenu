// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Meeting links of the desktop's calendar events (Google Meet, Zoom, Teams, Webex), read from Evolution
//! Data Server, where GNOME Shell's calendar gets them, so the island can offer to join.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use log::debug;
use zbus::Connection;

use crate::eds::{CALENDAR, close_calendar, open_calendar, timed};

/// The properties a link is looked for in, in this order.
const PROPERTIES: [&str; 4] = ["X-GOOGLE-CONFERENCE", "URL", "LOCATION", "DESCRIPTION"];
const KEEP: Duration = Duration::from_secs(6 * 60 * 60);
const CACHED: usize = 256;

type Key = (String, String);

/// The links already looked up, so an event's alert asks EDS once.
#[derive(Default)]
pub struct MeetingLinks {
    found: Mutex<HashMap<Key, (Instant, String)>>,
    /// One lookup at a time: each opens and closes the calendar for keenud.
    lookup: tokio::sync::Mutex<()>,
}

impl MeetingLinks {
    /// The event's meeting link, or "" (none, or EDS not there).
    pub async fn get(&self, bus: &Connection, source: &str, event: &str) -> String {
        let key = (source.to_owned(), event.to_owned());
        if let Some(link) = self.cached(&key, Instant::now()) {
            return link;
        }
        let _one = self.lookup.lock().await;
        if let Some(link) = self.cached(&key, Instant::now()) {
            return link;
        }
        match read_event(bus, source, event).await {
            Ok(ical) => {
                let link = find_link(&ical).unwrap_or_default();
                self.remember(key, link.clone(), Instant::now());
                link
            }
            Err(err) => {
                debug!("could not read calendar event {event}: {err}");
                String::new()
            }
        }
    }

    /// A link looked up within the last six hours.
    fn cached(&self, key: &Key, now: Instant) -> Option<String> {
        let found = self.found.lock().unwrap_or_else(PoisonError::into_inner);
        let (at, link) = found.get(key)?;
        (now.duration_since(*at) < KEEP).then(|| link.clone())
    }

    /// Keeps a found link, making room by dropping stale ones, then the oldest.
    fn remember(&self, key: Key, link: String, now: Instant) {
        let mut found = self.found.lock().unwrap_or_else(PoisonError::into_inner);
        if found.len() >= CACHED && !found.contains_key(&key) {
            found.retain(|_, (at, _)| now.duration_since(*at) < KEEP);
            let oldest = found.iter().min_by_key(|(_, (at, _))| *at);
            if let Some(oldest) = oldest.map(|(key, _)| key.clone())
                && found.len() >= CACHED
            {
                found.remove(&oldest);
            }
        }
        found.insert(key, (now, link));
    }
}

/// The event's iCalendar text, from the calendar `source` in EDS.
async fn read_event(bus: &Connection, source: &str, event: &str) -> zbus::Result<String> {
    let (path, name) = timed(open_calendar(bus, source)).await?;
    let (path, name) = (path.as_str(), name.as_str());
    let ical = timed(async {
        bus.call_method(Some(name), path, Some(CALENDAR), "Open", &())
            .await?;
        bus.call_method(Some(name), path, Some(CALENDAR), "GetObject", &(event, ""))
            .await?
            .body()
            .deserialize::<String>()
    })
    .await;
    close_calendar(bus, path, name, source).await;
    ical
}

/// The first meeting link in an event's iCalendar text, looked for property by property.
pub fn find_link(ical: &str) -> Option<String> {
    let lines = unfold(ical);
    PROPERTIES.iter().find_map(|wanted| {
        lines
            .iter()
            .filter_map(|line| split(line))
            .filter(|(name, _)| name.eq_ignore_ascii_case(wanted))
            .find_map(|(_, value)| links(&unescape(value)).find(|link| is_meeting(link)))
    })
}

/// Joins folded lines (a long line goes on in the next, after a space or tab).
fn unfold(ical: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in ical.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        match (line.strip_prefix([' ', '\t']), lines.last_mut()) {
            (Some(rest), Some(last)) => last.push_str(rest),
            _ => lines.push(line.to_owned()),
        }
    }
    lines
}

/// A content line's name and value: `DESCRIPTION;LANGUAGE=en:Join…` gives `DESCRIPTION`, `Join…`.
fn split(line: &str) -> Option<(&str, &str)> {
    let name_end = line.find([';', ':'])?;
    let mut quoted = false;
    for (at, c) in line.char_indices().skip_while(|(at, _)| *at < name_end) {
        match c {
            '"' => quoted = !quoted,
            ':' if !quoted => return Some((&line[..name_end], &line[at + 1..])),
            _ => {}
        }
    }
    None
}

/// Undoes iCalendar's escapes: `\\`, `\n`, `\,` and `\;`.
fn unescape(value: &str) -> String {
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => text.push('\n'),
            Some(other) => text.push(other),
            None => text.push('\\'),
        }
    }
    text
}

/// Each `https://` address in a text (plain, or in HTML a calendar put in a description).
fn links(text: &str) -> impl Iterator<Item = String> + '_ {
    text.match_indices("https://").map(|(at, _)| {
        let rest = &text[at..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c.is_control() || "<>\"'`|\\".contains(c))
            .unwrap_or(rest.len());
        rest[..end]
            .trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '!', '?'])
            .replace("&amp;", "&")
    })
}

/// Whether a link joins a meeting on a service Keenu knows.
fn is_meeting(link: &str) -> bool {
    let Some(rest) = link.strip_prefix("https://") else {
        return false;
    };
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, path) = rest.split_at(host_end);
    // No `user@host`, which could pass one host off as another.
    if host.contains('@') {
        return false;
    }
    let host = host
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let on = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    let has_path = |prefixes: &[&str]| prefixes.iter().any(|prefix| path.starts_with(prefix));
    match host.as_str() {
        "meet.google.com" => path.len() > 1,
        "teams.microsoft.com" => has_path(&["/l/meetup-join/", "/meet/"]),
        "teams.live.com" => path.len() > 1,
        _ if on("zoom.us") => has_path(&["/j/", "/my/", "/w/"]),
        _ if on("webex.com") => host != "webex.com" && path.len() > 1,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_meet_comes_from_its_own_property() {
        let ical = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:abc@google.com\r\nSUMMARY:Standup\r\n\
                    LOCATION:Room 4\r\nDESCRIPTION:Notes: https://docs.google.com/x\r\n\
                    X-GOOGLE-CONFERENCE:https://meet.google.com/abc-defg-hij\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert_eq!(
            find_link(ical).as_deref(),
            Some("https://meet.google.com/abc-defg-hij")
        );
    }

    #[test]
    fn zoom_in_a_folded_description() {
        let ical = "BEGIN:VEVENT\r\nSUMMARY:Review\r\nDESCRIPTION:Hi all\\,\\nJoin Zoom Meeting\\n\
                    https://us02web.zo\r\n om.us/j/8812345678?pwd=Ab\r\n\tCd.1\\n\\nMeeting ID: 881\r\n\
                    URL:https://example.com/agenda\r\nEND:VEVENT\r\n";
        assert_eq!(
            find_link(ical).as_deref(),
            Some("https://us02web.zoom.us/j/8812345678?pwd=AbCd.1")
        );
    }

    #[test]
    fn teams_in_the_location() {
        let ical = "BEGIN:VEVENT\nLOCATION;LANGUAGE=en-US:Microsoft Teams Meeting\\; \
                    https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=%7b%7d\n\
                    DESCRIPTION:<a href=\"https://zoom.us/j/1?a=1&amp;b=2\">old link</a>\nEND:VEVENT\n";
        assert_eq!(
            find_link(ical).as_deref(),
            Some(
                "https://teams.microsoft.com/l/meetup-join/19%3ameeting_x%40thread.v2/0?context=%7b%7d"
            )
        );
        let html = "BEGIN:VEVENT\nDESCRIPTION:<a href=\"https://zoom.us/j/1?a=1&amp;b=2\">Join</a>.\nEND:VEVENT\n";
        assert_eq!(
            find_link(html).as_deref(),
            Some("https://zoom.us/j/1?a=1&b=2")
        );
    }

    #[test]
    fn no_meeting_no_link() {
        let ical = "BEGIN:VEVENT\nSUMMARY:Lunch https://meet.google.com/abc\nLOCATION:Café\n\
                    DESCRIPTION:See http://meet.google.com/abc-defg-hij and https://zoom.us/pricing\n\
                    URL:https://meet.google.com.evil.example/abc\nEND:VEVENT\n";
        assert_eq!(find_link(ical), None);
        assert_eq!(find_link(""), None);
    }

    #[test]
    fn hosts_are_checked_whole() {
        assert!(is_meeting("https://meet.google.com/abc-defg-hij"));
        assert!(is_meeting("https://acme.webex.com/meet/jo"));
        assert!(is_meeting("https://teams.live.com/meet/9312"));
        assert!(is_meeting("https://zoom.us/my/jo"));
        assert!(!is_meeting("https://meet.google.com@evil.example/abc"));
        assert!(!is_meeting("https://notzoom.us/j/1"));
        assert!(!is_meeting("https://meet.google.com/"));
        assert!(!is_meeting("https://teams.microsoft.com/v2/"));
    }

    #[test]
    fn escapes_are_undone() {
        assert_eq!(unescape(r"a\, b\; c\\d\ne\N"), "a, b; c\\d\ne\n");
        assert_eq!(
            split("DESCRIPTION;ALTREP=\"cid:x@y\":Text: more"),
            Some(("DESCRIPTION", "Text: more"))
        );
    }

    #[test]
    fn the_cache_stays_small_and_fresh() {
        let links = MeetingLinks::default();
        let start = Instant::now();
        for i in 0..CACHED + 10 {
            let at = start + Duration::from_secs(i as u64);
            links.remember(("s".into(), i.to_string()), format!("l{i}"), at);
        }
        let now = start + Duration::from_secs(CACHED as u64 + 10);
        assert_eq!(links.found.lock().unwrap().len(), CACHED);
        assert_eq!(links.cached(&("s".into(), "0".into()), now), None);
        let last = (CACHED + 9).to_string();
        assert_eq!(
            links.cached(&("s".into(), last.clone()), now).as_deref(),
            Some(format!("l{last}").as_str())
        );
        assert_eq!(links.cached(&("s".into(), last), now + KEEP), None);
    }
}
