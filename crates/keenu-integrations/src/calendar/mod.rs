// SPDX-License-Identifier: GPL-3.0-or-later

//! Calendar accounts behind one interface: CalDAV servers, Google Calendar and Microsoft Graph.

pub mod caldav;
pub mod engine;
pub mod google;
pub mod graph;
mod xml;

use std::fmt;

use crate::oauth::OAuthError;

/// One calendar of an account.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CalendarInfo {
    pub id: String,
    pub name: String,
    pub read_only: bool,
}

/// An event as a server holds it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Event {
    /// The server's name for it: a path on CalDAV, an id elsewhere.
    pub id: String,
    /// What an edit must quote, so it cannot overwrite a change it has not seen.
    pub etag: String,
    pub title: String,
    pub location: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    /// The whole iCalendar object on CalDAV, whose repeats Keenu expands itself. Others send each repeat
    /// as an event of its own.
    #[serde(default)]
    pub ics: Option<String>,
}

/// An event to create, or what to change one to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub title: String,
    pub location: String,
    pub description: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    /// IANA name of the zone it is planned in; repeats keep their local hour there.
    pub time_zone: String,
    /// An RFC 5545 rule without the `RRULE:`; `None` for one that does not repeat.
    pub recurrence: Option<String>,
}

/// What changed since the last sync.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncResult {
    pub changed: Vec<Event>,
    pub removed: Vec<String>,
    /// Pass this next time. `None` when the server has no incremental sync: everything is sent again.
    pub token: Option<String>,
    /// Everything was sent, so events not among `changed` are gone.
    pub full: bool,
}

/// Why a calendar request failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalendarError {
    /// The server does not accept these credentials: sign in again.
    Unauthorized(String),
    /// Someone else changed the event first; the edit was not made.
    Conflict,
    NotFound,
    /// The sync token is too old; a full sync is needed.
    TokenExpired,
    /// Too many requests or a server fault; worth trying again after this many seconds.
    Busy {
        retry_after: Option<u64>,
    },
    /// No answer from the server. Local changes wait for it.
    Offline(String),
    /// Anything else, in words for the user.
    Failed(String),
}

impl fmt::Display for CalendarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized(why) | Self::Offline(why) | Self::Failed(why) => f.write_str(why),
            Self::Conflict => f.write_str("The event was changed elsewhere first."),
            Self::NotFound => f.write_str("The event or calendar no longer exists."),
            Self::TokenExpired => f.write_str("The sync position has expired."),
            Self::Busy { .. } => f.write_str("The calendar server is busy."),
        }
    }
}

impl From<OAuthError> for CalendarError {
    fn from(err: OAuthError) -> Self {
        match err {
            OAuthError::Denied(why) => Self::Unauthorized(why),
            OAuthError::Offline(why) => Self::Offline(why),
            OAuthError::Failed(why) => Self::Failed(why),
        }
    }
}

/// A calendar account. Calls block on the network.
pub trait CalendarProvider: Send + Sync {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, CalendarError>;
    /// What changed since `token`; everything without one.
    fn sync(&self, calendar: &str, token: Option<&str>) -> Result<SyncResult, CalendarError>;
    fn create(&self, calendar: &str, draft: &EventDraft) -> Result<Event, CalendarError>;
    fn update(
        &self,
        calendar: &str,
        event: &str,
        etag: &str,
        draft: &EventDraft,
    ) -> Result<Event, CalendarError>;
    fn delete(&self, calendar: &str, event: &str, etag: &str) -> Result<(), CalendarError>;
}

/// Maps a status with no particular meaning to its error; `None` for success.
fn status_error(status: u16, retry_after: Option<u64>, service: &str) -> Option<CalendarError> {
    Some(match status {
        200..=299 => return None,
        401 | 403 => CalendarError::Unauthorized(format!(
            "{service} did not accept the sign-in ({status}). Sign in again."
        )),
        404 => CalendarError::NotFound,
        410 => CalendarError::TokenExpired,
        412 => CalendarError::Conflict,
        429 | 500..=599 => CalendarError::Busy { retry_after },
        other => CalendarError::Failed(format!("{service} answered {other}.")),
    })
}

/// Google's and Microsoft's date-time, to milliseconds. A time without an offset is taken as UTC, as
/// Keenu asks Microsoft to send them.
fn parse_time_ms(value: &str) -> Option<i64> {
    if let Ok(time) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(time.timestamp_millis());
    }
    let trimmed = value.split('.').next().unwrap_or(value);
    chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|time| time.and_utc().timestamp_millis())
}

/// A date's start, on this computer's clock.
fn date_to_ms(value: &str) -> Option<i64> {
    use chrono::TimeZone;
    let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    let midnight = date.and_hms_opt(0, 0, 0)?;
    chrono::Local
        .from_local_datetime(&midnight)
        .earliest()
        .map(|time| time.timestamp_millis())
}

fn ms_to_date(ms: i64) -> Option<String> {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|time| time.format("%Y-%m-%d").to_string())
}

fn ms_to_rfc3339(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[cfg(test)]
pub mod test_server {
    //! A scripted HTTP server on loopback: a mock, standing in for a calendar service in unit tests.

    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A request as the server saw it.
    #[derive(Debug, Clone)]
    pub struct Seen {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    impl Seen {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }
    }

    pub struct Server {
        pub url: String,
        pub seen: Arc<Mutex<Vec<Seen>>>,
    }

    /// `(status, extra headers, body)`.
    pub type Answer = (u16, Vec<(&'static str, String)>, String);

    /// Serves `answers` in order, one per request.
    pub fn serve(answers: Vec<Answer>) -> Server {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let own_url = url.clone();
        std::thread::spawn(move || {
            for (status, headers, body) in answers {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut parts = first.split_whitespace();
                let method = parts.next().unwrap_or_default().to_owned();
                let path = parts.next().unwrap_or_default().to_owned();
                let mut received = Vec::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let line = line.trim_end();
                    if line.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                        received.push((name.to_owned(), value.trim().to_owned()));
                    }
                }
                let mut sent = vec![0; length];
                reader.read_exact(&mut sent).unwrap();
                log.lock().unwrap().push(Seen {
                    method,
                    path,
                    headers: received,
                    body: String::from_utf8_lossy(&sent).into_owned(),
                });
                let mut stream = reader.into_inner();
                let body = body.replace("{{URL}}", &own_url);
                let mut head = format!(
                    "HTTP/1.1 {status} Whatever\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(body.as_bytes()).unwrap();
            }
        });
        Server { url, seen }
    }
}
