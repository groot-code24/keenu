// SPDX-License-Identifier: GPL-3.0-or-later

//! CalDAV (RFC 4791) with WebDAV sync (RFC 6578): finding calendars with PROPFIND, reading events with
//! REPORT, and writing them with PUT and DELETE, guarded by ETags.

use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;

use super::xml::{self, Node};
use super::{
    CalendarError, CalendarInfo, CalendarProvider, Event, EventDraft, SyncResult, status_error,
};
use crate::http;
use crate::ical::{self, VEvent, When};

const SERVICE: &str = "The CalDAV server";

/// One CalDAV account.
pub struct CalDav {
    /// The server's address, or a calendar home on it.
    base: String,
    authorization: String,
}

fn basic(username: &str, password: &str) -> String {
    use std::fmt::Write;
    // Base64 by hand keeps this crate free of one more dependency for a dozen lines' work.
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = format!("{username}:{password}");
    let mut out = String::from("Basic ");
    for chunk in input.as_bytes().chunks(3) {
        let bits = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        let _ = out.write_char(TABLE[(bits >> 18) as usize & 63] as char);
        let _ = out.write_char(TABLE[(bits >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(bits >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[bits as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

impl CalDav {
    /// A secure address is required: the password goes with every request.
    pub fn new(base: &str, username: &str, password: &str) -> Result<Self, CalendarError> {
        if !http::is_secure_url(base) {
            return Err(CalendarError::Failed(
                "The CalDAV address must use HTTPS, so your password is not sent in the clear."
                    .into(),
            ));
        }
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            authorization: basic(username, password),
        })
    }

    /// `path` from a server's answer, as a full address on the same server.
    fn absolute(&self, path: &str) -> String {
        if path.starts_with("http://") || path.starts_with("https://") {
            return path.to_owned();
        }
        let origin_end = self
            .base
            .find("://")
            .and_then(|at| self.base[at + 3..].find('/').map(|slash| at + 3 + slash))
            .unwrap_or(self.base.len());
        format!("{}{}", &self.base[..origin_end], path)
    }

    fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<http::Response, CalendarError> {
        if !http::is_secure_url(url) {
            return Err(CalendarError::Failed(
                "The server sent an address that is not secure; it was not followed.".into(),
            ));
        }
        let mut all = vec![("Authorization", self.authorization.as_str())];
        all.extend_from_slice(headers);
        http::send(method, url, &all, body.map(str::as_bytes)).map_err(|err| {
            CalendarError::Offline(format!("Could not reach the CalDAV server: {err}"))
        })
    }

    fn xml_request(
        &self,
        method: &str,
        url: &str,
        depth: &str,
        body: &str,
    ) -> Result<Node, CalendarError> {
        let answer = self.send(
            method,
            url,
            &[
                ("Depth", depth),
                ("Content-Type", "application/xml; charset=utf-8"),
            ],
            Some(body),
        )?;
        if let Some(error) = status_error(answer.status, answer.retry_after(), SERVICE) {
            return Err(error);
        }
        xml::parse(&answer.text()).map_err(|err| {
            CalendarError::Failed(format!("{SERVICE} sent an unreadable answer: {err}"))
        })
    }

    /// Where the user's calendars are: asks for the principal and its home, and takes the address given
    /// as the home when the server has no such discovery.
    fn calendar_home(&self) -> Result<String, CalendarError> {
        const PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:current-user-principal/></d:prop></d:propfind>"#;
        const HOME: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><c:calendar-home-set/></d:prop></d:propfind>"#;
        let Ok(root) = self.xml_request("PROPFIND", &self.base, "0", PRINCIPAL) else {
            return Ok(self.base.clone());
        };
        let principal = root
            .descendants("current-user-principal")
            .first()
            .and_then(|node| node.find_text("href"))
            .map(|href| self.absolute(href));
        let Some(principal) = principal else {
            return Ok(self.base.clone());
        };
        let root = self.xml_request("PROPFIND", &principal, "0", HOME)?;
        Ok(root
            .descendants("calendar-home-set")
            .first()
            .and_then(|node| node.find_text("href"))
            .map_or_else(|| self.base.clone(), |href| self.absolute(href)))
    }

    /// Reads events by their paths, a few at a time, for what a sync says changed.
    fn multiget(&self, calendar: &str, paths: &[String]) -> Result<Vec<Event>, CalendarError> {
        let mut events = Vec::new();
        for batch in paths.chunks(100) {
            let hrefs: String = batch
                .iter()
                .map(|path| format!("<d:href>{}</d:href>", xml::escape(path)))
                .collect();
            let body = format!(
                r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-multiget xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop>{hrefs}</c:calendar-multiget>"#
            );
            let root = self.xml_request("REPORT", calendar, "1", &body)?;
            events.extend(events_of(&root));
        }
        Ok(events)
    }

    /// Everything in the calendar.
    fn full_sync(&self, calendar: &str) -> Result<SyncResult, CalendarError> {
        const QUERY: &str = r#"<?xml version="1.0" encoding="utf-8"?><c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><c:calendar-data/></d:prop><c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT"/></c:comp-filter></c:filter></c:calendar-query>"#;
        const TOKEN: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:"><d:prop><d:sync-token/></d:prop></d:propfind>"#;
        // Asked first: a change made during the query is then seen by the next sync, not lost.
        let token = self
            .xml_request("PROPFIND", calendar, "0", TOKEN)
            .ok()
            .and_then(|root| root.find_text("sync-token").map(str::to_owned))
            .filter(|token| !token.is_empty());
        let root = self.xml_request("REPORT", calendar, "1", QUERY)?;
        Ok(SyncResult {
            changed: events_of(&root),
            removed: Vec::new(),
            token,
            full: true,
        })
    }

    /// Writes an object, under `condition` so nothing unseen is overwritten.
    fn put(
        &self,
        url: &str,
        condition: (&str, &str),
        draft: &EventDraft,
        body: String,
    ) -> Result<Event, CalendarError> {
        let answer = self.send(
            "PUT",
            url,
            &[("Content-Type", "text/calendar; charset=utf-8"), condition],
            Some(&body),
        )?;
        if let Some(error) = status_error(answer.status, answer.retry_after(), SERVICE) {
            return Err(error);
        }
        // Some servers do not send an ETag for a write; the next sync brings it.
        let etag = answer.header("etag").unwrap_or_default().to_owned();
        Ok(Event {
            id: canonical_path(calendar_path(url)),
            etag,
            title: draft.title.clone(),
            location: draft.location.clone(),
            start_ms: draft.start_ms,
            end_ms: draft.end_ms,
            all_day: draft.all_day,
            ics: Some(body),
        })
    }
}

/// One spelling of a path, whichever way a server escapes it: `@` comes back as `%40` from some and not
/// others, and an event must be recognised by it across a sync.
fn canonical_path(path: &str) -> String {
    path.split('/')
        .map(|part| http::encode_component(&http::decode_component(&part.replace('+', "%2B"))))
        .collect::<Vec<_>>()
        .join("/")
}

/// Turns a multistatus answer's items into events; items without calendar data are skipped.
fn events_of(root: &Node) -> Vec<Event> {
    root.descendants("response")
        .into_iter()
        .filter_map(|response| {
            let path = canonical_path(response.find_text("href")?);
            let ics = response.find_text("calendar-data")?;
            event_from_ics(path, response.find_text("getetag").unwrap_or_default(), ics)
        })
        .collect()
}

fn event_from_ics(id: String, etag: &str, ics: &str) -> Option<Event> {
    let parsed = ical::parse(ics);
    let main = parsed
        .iter()
        .find(|event| event.recurrence_id.is_none())
        .or_else(|| parsed.first())?;
    let start = main.start.to_utc();
    Some(Event {
        id,
        etag: etag.to_owned(),
        title: main.summary.clone(),
        location: main.location.clone(),
        start_ms: start.timestamp_millis(),
        end_ms: main.end.map_or(start, When::to_utc).timestamp_millis(),
        all_day: main.start.is_date(),
        ics: Some(ics.to_owned()),
    })
}

fn to_vevent(draft: &EventDraft, uid: &str) -> Result<VEvent, CalendarError> {
    let to_utc = |ms: i64| {
        DateTime::<Utc>::from_timestamp_millis(ms)
            .ok_or_else(|| CalendarError::Failed("That time is out of range.".into()))
    };
    let (start, end) = (to_utc(draft.start_ms)?, to_utc(draft.end_ms)?);
    let when = |time: DateTime<Utc>| -> When {
        if draft.all_day {
            return When::Date(time.with_timezone(&chrono::Local).date_naive());
        }
        match draft.time_zone.parse::<Tz>() {
            Ok(zone) => When::Zoned(time.with_timezone(&zone).naive_local(), zone),
            Err(_) => When::Utc(time),
        }
    };
    // An all-day event's end is the day after its last.
    let end_when = if draft.all_day && end.date_naive() == start.date_naive() {
        when(end + Duration::days(1))
    } else {
        when(end)
    };
    Ok(VEvent {
        uid: uid.to_owned(),
        summary: draft.title.clone(),
        description: draft.description.clone(),
        location: draft.location.clone(),
        start: when(start),
        end: Some(end_when),
        rule: None,
        exceptions: Vec::new(),
        recurrence_id: None,
        cancelled: false,
    })
}

impl CalendarProvider for CalDav {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, CalendarError> {
        const LIST: &str = r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:prop><d:resourcetype/><d:displayname/><d:current-user-privilege-set/><c:supported-calendar-component-set/></d:prop></d:propfind>"#;
        let home = self.calendar_home()?;
        let root = self.xml_request("PROPFIND", &home, "1", LIST)?;
        let mut calendars = Vec::new();
        for response in root.descendants("response") {
            if !response.has("calendar") {
                continue;
            }
            // A calendar only of tasks is not one Keenu can show.
            let components = response.descendants("comp");
            if !components.is_empty()
                && response
                    .descendants("supported-calendar-component-set")
                    .is_empty()
            {
                continue;
            }
            let Some(path) = response.find_text("href") else {
                continue;
            };
            let name = response
                .find_text("displayname")
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| {
                    path.trim_end_matches('/')
                        .rsplit('/')
                        .next()
                        .unwrap_or(path)
                });
            let read_only = response.has("current-user-privilege-set") && !response.has("write");
            calendars.push(CalendarInfo {
                id: self.absolute(path),
                name: name.to_owned(),
                read_only,
            });
        }
        Ok(calendars)
    }

    fn sync(&self, calendar: &str, token: Option<&str>) -> Result<SyncResult, CalendarError> {
        let Some(token) = token else {
            return self.full_sync(calendar);
        };
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:sync-collection xmlns:d="DAV:"><d:sync-token>{}</d:sync-token><d:sync-level>1</d:sync-level><d:prop><d:getetag/></d:prop></d:sync-collection>"#,
            xml::escape(token)
        );
        let answer = self.send(
            "REPORT",
            calendar,
            &[
                ("Depth", "1"),
                ("Content-Type", "application/xml; charset=utf-8"),
            ],
            Some(&body),
        )?;
        // RFC 6578: an expired token is 403 or 409 with DAV:valid-sync-token.
        if matches!(answer.status, 403 | 409 | 410) && answer.text().contains("valid-sync-token") {
            return Err(CalendarError::TokenExpired);
        }
        if let Some(error) = status_error(answer.status, answer.retry_after(), SERVICE) {
            return Err(error);
        }
        let root = xml::parse(&answer.text()).map_err(|err| {
            CalendarError::Failed(format!("{SERVICE} sent an unreadable answer: {err}"))
        })?;
        let mut changed = Vec::new();
        let mut removed = Vec::new();
        for response in root.descendants("response") {
            let Some(path) = response.find_text("href") else {
                continue;
            };
            let gone = response
                .find_text("status")
                .is_some_and(|status| status.contains(" 404 "));
            let own = canonical_path(calendar_path(calendar));
            let path = canonical_path(path);
            if gone {
                removed.push(path);
            } else if path.trim_end_matches('/') != own.trim_end_matches('/') {
                changed.push(path);
            }
        }
        Ok(SyncResult {
            changed: if changed.is_empty() {
                Vec::new()
            } else {
                self.multiget(calendar, &changed)?
            },
            removed,
            token: root.find_text("sync-token").map(str::to_owned),
            full: false,
        })
    }

    fn create(&self, calendar: &str, draft: &EventDraft) -> Result<Event, CalendarError> {
        let name = keenu_security::random_hex(16).map_err(CalendarError::Failed)?;
        let url = format!("{}/{name}.ics", calendar.trim_end_matches('/'));
        let event = to_vevent(draft, &format!("{name}@keenu"))?;
        let body = ical::to_ics(&event, draft.recurrence.as_deref());
        // If-None-Match: nothing already there gets overwritten.
        self.put(&url, ("If-None-Match", "*"), draft, body)
    }

    fn update(
        &self,
        _calendar: &str,
        event: &str,
        etag: &str,
        draft: &EventDraft,
    ) -> Result<Event, CalendarError> {
        if etag.is_empty() {
            return Err(CalendarError::Conflict);
        }
        let url = self.absolute(event);
        // The object as it is: its UID need not match its path, and it may hold what Keenu does not show.
        let current = self.send("GET", &url, &[("Accept", "text/calendar")], None)?;
        if let Some(error) = status_error(current.status, current.retry_after(), SERVICE) {
            return Err(error);
        }
        let original = current.text();
        let uid = ical::parse(&original)
            .first()
            .map(|found| found.uid.clone())
            .unwrap_or_default();
        let event = to_vevent(draft, &uid)?;
        let body = ical::patch_ics(&original, &event, draft.recurrence.as_deref());
        self.put(&url, ("If-Match", etag), draft, body)
    }

    fn delete(&self, _calendar: &str, event: &str, etag: &str) -> Result<(), CalendarError> {
        let url = self.absolute(event);
        let mut headers = Vec::new();
        if !etag.is_empty() {
            headers.push(("If-Match", etag));
        }
        let answer = self.send("DELETE", &url, &headers, None)?;
        match status_error(answer.status, answer.retry_after(), SERVICE) {
            // Already gone is what was wanted.
            None | Some(CalendarError::NotFound) => Ok(()),
            Some(error) => Err(error),
        }
    }
}

fn calendar_path(calendar: &str) -> &str {
    calendar
        .find("://")
        .and_then(|at| {
            calendar[at + 3..]
                .find('/')
                .map(|slash| &calendar[at + 3 + slash..])
        })
        .unwrap_or(calendar)
}

#[cfg(test)]
mod tests {
    use super::super::test_server::serve;
    use super::*;

    const ICS: &str = "BEGIN:VCALENDAR&#13;\nBEGIN:VEVENT&#13;\nUID:u1&#13;\nSUMMARY:Lunch&#13;\nDTSTART:20261031T100000Z&#13;\nDTEND:20261031T110000Z&#13;\nEND:VEVENT&#13;\nEND:VCALENDAR";

    fn draft() -> EventDraft {
        EventDraft {
            title: "Lunch".into(),
            location: String::new(),
            description: String::new(),
            start_ms: 1_793_440_800_000,
            end_ms: 1_793_444_400_000,
            all_day: false,
            time_zone: "Asia/Kolkata".into(),
            recurrence: None,
        }
    }

    #[test]
    fn refuses_to_send_a_password_in_the_clear() {
        assert!(CalDav::new("http://dav.example.org", "a", "b").is_err());
        assert!(CalDav::new("https://dav.example.org", "a", "b").is_ok());
    }

    #[test]
    fn makes_the_basic_header() {
        assert_eq!(
            basic("Aladdin", "open sesame"),
            "Basic QWxhZGRpbjpvcGVuIHNlc2FtZQ=="
        );
    }

    #[test]
    fn finds_calendars_through_the_principal_and_home() {
        let server = serve(vec![
            (207, vec![], r#"<d:multistatus xmlns:d="DAV:"><d:response><d:propstat><d:prop><d:current-user-principal><d:href>/p/me/</d:href></d:current-user-principal></d:prop></d:propstat></d:response></d:multistatus>"#.into()),
            (207, vec![], r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:propstat><d:prop><c:calendar-home-set><d:href>/home/me/</d:href></c:calendar-home-set></d:prop></d:propstat></d:response></d:multistatus>"#.into()),
            (207, vec![], r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
                <d:response><d:href>/home/me/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
                <d:response><d:href>/home/me/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Work</d:displayname></d:prop></d:propstat></d:response>
              </d:multistatus>"#.into()),
        ]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        let calendars = account.calendars().unwrap();
        assert_eq!(calendars.len(), 1);
        assert_eq!(calendars[0].name, "Work");
        assert!(calendars[0].id.ends_with("/home/me/work/"));
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].method, "PROPFIND");
        assert_eq!(seen[2].path, "/home/me/");
        assert_eq!(seen[2].header("Depth"), Some("1"));
        assert!(
            seen[0]
                .header("Authorization")
                .unwrap()
                .starts_with("Basic ")
        );
    }

    #[test]
    fn a_full_sync_reads_events_and_the_token() {
        let body = format!(
            r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/cal/u1.ics</d:href><d:propstat><d:prop><d:getetag>"e1"</d:getetag><c:calendar-data>{ICS}</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#
        );
        let server = serve(vec![
            (207, vec![], r#"<d:multistatus xmlns:d="DAV:"><d:response><d:propstat><d:prop><d:sync-token>t1</d:sync-token></d:prop></d:propstat></d:response></d:multistatus>"#.into()),
            (207, vec![], body),
        ]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        let url = format!("{}/cal/", server.url);
        let result = account.sync(&url, None).unwrap();
        assert!(result.full);
        assert_eq!(result.token.as_deref(), Some("t1"));
        assert_eq!(result.changed[0].id, "/cal/u1.ics");
        assert_eq!(result.changed[0].etag, "\"e1\"");
        assert_eq!(result.changed[0].title, "Lunch");
        assert_eq!(server.seen.lock().unwrap()[1].method, "REPORT");
    }

    #[test]
    fn an_incremental_sync_tells_changed_from_removed() {
        let changes = r#"<d:multistatus xmlns:d="DAV:"><d:sync-token>t2</d:sync-token>
            <d:response><d:href>/cal/u1.ics</d:href><d:propstat><d:prop><d:getetag>"e2"</d:getetag></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
            <d:response><d:href>/cal/old.ics</d:href><d:status>HTTP/1.1 404 Not Found</d:status></d:response>
          </d:multistatus>"#;
        let got = format!(
            r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/cal/u1.ics</d:href><d:propstat><d:prop><d:getetag>"e2"</d:getetag><c:calendar-data>{ICS}</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#
        );
        let server = serve(vec![(207, vec![], changes.into()), (207, vec![], got)]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        let result = account
            .sync(&format!("{}/cal/", server.url), Some("t1"))
            .unwrap();
        assert!(!result.full);
        assert_eq!(result.removed, ["/cal/old.ics"]);
        assert_eq!(result.changed[0].etag, "\"e2\"");
        assert_eq!(result.token.as_deref(), Some("t2"));
        let seen = server.seen.lock().unwrap();
        assert!(seen[0].body.contains("<d:sync-token>t1</d:sync-token>"));
        assert!(seen[1].body.contains("calendar-multiget"));
    }

    #[test]
    fn an_expired_token_asks_for_a_full_sync() {
        let server = serve(vec![(
            403,
            vec![],
            r#"<d:error xmlns:d="DAV:"><d:valid-sync-token/></d:error>"#.into(),
        )]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        let result = account.sync(&format!("{}/cal/", server.url), Some("old"));
        assert_eq!(result, Err(CalendarError::TokenExpired));
    }

    #[test]
    fn creating_cannot_overwrite_and_returns_the_etag() {
        let server = serve(vec![(
            201,
            vec![("ETag", "\"new\"".to_owned())],
            String::new(),
        )]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        let event = account
            .create(&format!("{}/cal/", server.url), &draft())
            .unwrap();
        assert_eq!(event.etag, "\"new\"");
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].method, "PUT");
        assert_eq!(seen[0].header("If-None-Match"), Some("*"));
        assert!(seen[0].path.starts_with("/cal/") && seen[0].path.ends_with(".ics"));
        assert!(seen[0].body.contains("SUMMARY:Lunch"));
        assert!(seen[0].body.contains("DTSTART;TZID=Asia/Kolkata:"));
    }

    #[test]
    fn changing_an_event_changed_elsewhere_is_a_conflict() {
        let existing = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:not-the-file-name\r\nSUMMARY:Old\r\nDTSTART:20261031T100000Z\r\nATTENDEE:mailto:a@example.org\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let server = serve(vec![
            (200, vec![], existing.into()),
            (412, vec![], String::new()),
        ]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        let result = account.update("", "/cal/u1.ics", "\"stale\"", &draft());
        assert_eq!(result, Err(CalendarError::Conflict));
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[1].header("If-Match"), Some("\"stale\""));
        assert!(
            seen[1].body.contains("UID:not-the-file-name"),
            "keeps the event's identity"
        );
        assert!(
            seen[1].body.contains("ATTENDEE:mailto:a@example.org"),
            "and what Keenu does not show"
        );
        assert!(seen[1].body.contains("SUMMARY:Lunch"));
    }

    #[test]
    fn a_path_is_the_same_path_however_it_is_escaped() {
        assert_eq!(
            canonical_path("/cal/a@b.ics"),
            canonical_path("/cal/a%40b.ics")
        );
        assert_eq!(
            canonical_path("/cal/a b.ics"),
            canonical_path("/cal/a%20b.ics")
        );
        assert_eq!(canonical_path("/cal/"), "/cal/");
    }

    #[test]
    fn deleting_what_is_gone_succeeds_and_a_conflict_does_not() {
        let server = serve(vec![
            (404, vec![], String::new()),
            (412, vec![], String::new()),
        ]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        assert_eq!(account.delete("", "/cal/u1.ics", "\"e\""), Ok(()));
        assert_eq!(
            account.delete("", "/cal/u1.ics", "\"e\""),
            Err(CalendarError::Conflict)
        );
    }

    #[test]
    fn wrong_credentials_say_so() {
        let server = serve(vec![(401, vec![], String::new())]);
        let account = CalDav::new(&server.url, "a", "b").unwrap();
        let result = account.sync(&format!("{}/cal/", server.url), Some("t"));
        assert!(matches!(result, Err(CalendarError::Unauthorized(_))));
    }

    #[test]
    fn a_server_down_is_offline_not_a_failure() {
        let account = CalDav::new("http://127.0.0.1:9", "a", "b").unwrap();
        let result = account.sync("http://127.0.0.1:9/cal/", Some("t"));
        assert!(matches!(result, Err(CalendarError::Offline(_))));
    }

    #[test]
    fn a_redirect_to_an_insecure_address_in_an_answer_is_not_followed() {
        let account = CalDav::new("https://dav.example.org", "a", "b").unwrap();
        let result = account.send("GET", "http://evil.example/x", &[], None);
        assert!(matches!(result, Err(CalendarError::Failed(_))));
    }

    #[test]
    fn an_all_day_event_ends_the_next_day() {
        let mut one_day = draft();
        one_day.all_day = true;
        let event = to_vevent(&one_day, "u").unwrap();
        let (start, end) = (event.start, event.end.unwrap());
        assert!(start.is_date() && end.is_date());
        assert!(end.to_utc() > start.to_utc());
    }
}
