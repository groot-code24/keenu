// SPDX-License-Identifier: GPL-3.0-or-later

//! Google Calendar API v3, with its `syncToken` for incremental sync.
//!
//! Needs an OAuth client of type "Desktop app" of your own; see `docs/calendar-setup.md`.

use serde_json::{Value, json};

use super::{
    CalendarError, CalendarInfo, CalendarProvider, Event, EventDraft, SyncResult, date_to_ms,
    ms_to_date, ms_to_rfc3339, parse_time_ms, status_error,
};
use crate::http;
use crate::oauth::{Provider, TokenSource};

const SERVICE: &str = "Google Calendar";
const API: &str = "https://www.googleapis.com/calendar/v3";

/// The application's registration: from the environment, or compiled in by whoever builds Keenu.
pub fn provider() -> Option<Provider> {
    let client_id = std::env::var("KEENU_GOOGLE_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("KEENU_GOOGLE_CLIENT_ID").map(str::to_owned))
        .filter(|id| !id.is_empty())?;
    let client_secret = std::env::var("KEENU_GOOGLE_CLIENT_SECRET")
        .ok()
        .or_else(|| option_env!("KEENU_GOOGLE_CLIENT_SECRET").map(str::to_owned))
        .filter(|secret| !secret.is_empty());
    Some(Provider {
        name: SERVICE,
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        scopes: "https://www.googleapis.com/auth/calendar",
        client_id,
        client_secret,
        // Needed for Google to give a refresh token at all.
        extra_auth_params: &[("access_type", "offline"), ("prompt", "consent")],
    })
}

/// The same registration, for the assistant to use the user's Gmail: reading, drafting and sending, never deleting
/// for good. Google treats this as a restricted scope: an OAuth client of one's own, in testing with oneself as a test
/// user, works; see `docs/calendar-setup.md`.
pub fn mail_provider() -> Option<Provider> {
    provider().map(|registered| Provider {
        name: "Gmail",
        scopes: "https://www.googleapis.com/auth/gmail.modify",
        ..registered
    })
}

/// Tells Google to withdraw the access, for when the user removes the account.
pub fn revoke(token: &str) -> Result<(), CalendarError> {
    let body = http::form(&[("token", token)]);
    let answer = http::send(
        "POST",
        "https://oauth2.googleapis.com/revoke",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        Some(body.as_bytes()),
    )
    .map_err(CalendarError::Offline)?;
    // 400 means it was not valid any more, which is the point.
    if answer.is_success() || answer.status == 400 {
        Ok(())
    } else {
        Err(CalendarError::Failed(format!(
            "{SERVICE} answered {}.",
            answer.status
        )))
    }
}

/// One Google account.
pub struct Google {
    tokens: TokenSource,
    api: String,
}

impl Google {
    pub fn new(tokens: TokenSource) -> Self {
        Self {
            tokens,
            api: API.to_owned(),
        }
    }

    #[cfg(test)]
    fn with_api(tokens: TokenSource, api: &str) -> Self {
        Self {
            tokens,
            api: api.to_owned(),
        }
    }

    /// Calls the API; on a 401 refreshes the token and tries once more.
    fn call(
        &self,
        method: &str,
        path: &str,
        extra: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<http::Response, CalendarError> {
        let url = format!("{}{path}", self.api);
        let text = body.map(Value::to_string);
        let send = |token: &str| {
            let bearer = format!("Bearer {token}");
            let mut headers = vec![
                ("Authorization", bearer.as_str()),
                ("Accept", "application/json"),
                ("Content-Type", "application/json"),
            ];
            headers.extend_from_slice(extra);
            http::send(method, &url, &headers, text.as_deref().map(str::as_bytes))
                .map_err(|err| CalendarError::Offline(format!("Could not reach {SERVICE}: {err}")))
        };
        let answer = send(&self.tokens.access_token()?)?;
        if answer.status == 401 {
            return send(&self.tokens.force_refresh()?);
        }
        Ok(answer)
    }

    fn json(&self, answer: http::Response) -> Result<Value, CalendarError> {
        if let Some(error) = status_error(answer.status, answer.retry_after(), SERVICE) {
            return Err(error);
        }
        serde_json::from_slice(&answer.body)
            .map_err(|_| CalendarError::Failed(format!("{SERVICE} sent an unreadable answer.")))
    }
}

fn encoded(id: &str) -> String {
    http::encode_component(id)
}

fn to_event(item: &Value) -> Option<Event> {
    let time = |field: &Value| -> Option<(i64, bool)> {
        if let Some(value) = field["dateTime"].as_str() {
            return Some((parse_time_ms(value)?, false));
        }
        Some((date_to_ms(field["date"].as_str()?)?, true))
    };
    let (start_ms, all_day) = time(&item["start"])?;
    let (end_ms, _) = time(&item["end"])?;
    Some(Event {
        id: item["id"].as_str()?.to_owned(),
        etag: item["etag"].as_str().unwrap_or_default().to_owned(),
        title: item["summary"].as_str().unwrap_or_default().to_owned(),
        location: item["location"].as_str().unwrap_or_default().to_owned(),
        start_ms,
        end_ms,
        all_day,
        ics: None,
    })
}

fn to_body(draft: &EventDraft) -> Result<Value, CalendarError> {
    let out_of_range = || CalendarError::Failed("That time is out of range.".to_owned());
    let time = |ms: i64| -> Result<Value, CalendarError> {
        if draft.all_day {
            Ok(json!({"date": ms_to_date(ms).ok_or_else(out_of_range)?}))
        } else {
            Ok(json!({
                "dateTime": ms_to_rfc3339(ms).ok_or_else(out_of_range)?,
                "timeZone": draft.time_zone,
            }))
        }
    };
    let mut body = json!({
        "summary": draft.title,
        "location": draft.location,
        "description": draft.description,
        "start": time(draft.start_ms)?,
        "end": time(draft.end_ms)?,
    });
    if let Some(rule) = &draft.recurrence {
        body["recurrence"] = json!([format!("RRULE:{rule}")]);
    }
    Ok(body)
}

impl CalendarProvider for Google {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, CalendarError> {
        let mut calendars = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let path = match &page {
                Some(token) => format!("/users/me/calendarList?pageToken={}", encoded(token)),
                None => "/users/me/calendarList".to_owned(),
            };
            let reply = self.json(self.call("GET", &path, &[], None)?)?;
            for item in reply["items"].as_array().into_iter().flatten() {
                let Some(id) = item["id"].as_str() else {
                    continue;
                };
                calendars.push(CalendarInfo {
                    id: id.to_owned(),
                    name: item["summaryOverride"]
                        .as_str()
                        .or_else(|| item["summary"].as_str())
                        .unwrap_or(id)
                        .to_owned(),
                    read_only: !matches!(item["accessRole"].as_str(), Some("owner" | "writer")),
                });
            }
            page = reply["nextPageToken"].as_str().map(str::to_owned);
            if page.is_none() {
                return Ok(calendars);
            }
        }
    }

    fn sync(&self, calendar: &str, token: Option<&str>) -> Result<SyncResult, CalendarError> {
        let mut result = SyncResult {
            full: token.is_none(),
            ..SyncResult::default()
        };
        let mut query = match token {
            Some(token) => format!(
                "singleEvents=true&showDeleted=true&syncToken={}",
                encoded(token)
            ),
            None => {
                // Past events matter little; a full sync would otherwise read years of them.
                let since = chrono::Utc::now() - chrono::Duration::days(30);
                format!(
                    "singleEvents=true&timeMin={}",
                    encoded(&since.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                )
            }
        };
        loop {
            let path = format!("/calendars/{}/events?{query}", encoded(calendar));
            let reply = self.json(self.call("GET", &path, &[], None)?)?;
            for item in reply["items"].as_array().into_iter().flatten() {
                if item["status"] == "cancelled" {
                    result
                        .removed
                        .extend(item["id"].as_str().map(str::to_owned));
                } else {
                    result.changed.extend(to_event(item));
                }
            }
            if let Some(next) = reply["nextPageToken"].as_str() {
                let base = query
                    .split("&pageToken=")
                    .next()
                    .unwrap_or_default()
                    .to_owned();
                query = format!("{base}&pageToken={}", encoded(next));
                continue;
            }
            result.token = reply["nextSyncToken"].as_str().map(str::to_owned);
            return Ok(result);
        }
    }

    fn create(&self, calendar: &str, draft: &EventDraft) -> Result<Event, CalendarError> {
        let path = format!("/calendars/{}/events", encoded(calendar));
        let reply = self.json(self.call("POST", &path, &[], Some(&to_body(draft)?))?)?;
        to_event(&reply)
            .ok_or_else(|| CalendarError::Failed(format!("{SERVICE} sent an unreadable event.")))
    }

    fn update(
        &self,
        calendar: &str,
        event: &str,
        etag: &str,
        draft: &EventDraft,
    ) -> Result<Event, CalendarError> {
        let path = format!("/calendars/{}/events/{}", encoded(calendar), encoded(event));
        let reply =
            self.json(self.call("PUT", &path, &[("If-Match", etag)], Some(&to_body(draft)?))?)?;
        to_event(&reply)
            .ok_or_else(|| CalendarError::Failed(format!("{SERVICE} sent an unreadable event.")))
    }

    fn delete(&self, calendar: &str, event: &str, etag: &str) -> Result<(), CalendarError> {
        let path = format!("/calendars/{}/events/{}", encoded(calendar), encoded(event));
        let answer = self.call("DELETE", &path, &[("If-Match", etag)], None)?;
        match status_error(answer.status, answer.retry_after(), SERVICE) {
            // 410 here is an event already deleted, not an old sync token.
            None | Some(CalendarError::NotFound | CalendarError::TokenExpired) => Ok(()),
            Some(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_server::serve;
    use super::*;
    use crate::oauth::Tokens;
    use keenu_security::credentials::{CredentialStore, MemoryStore};
    use std::sync::Arc;

    fn account(url: &str) -> Google {
        let store: Arc<dyn CredentialStore> = Arc::new(MemoryStore::default());
        let provider = Provider {
            name: SERVICE,
            auth_url: format!("{url}/auth"),
            token_url: format!("{url}/token"),
            scopes: "x",
            client_id: "id".into(),
            client_secret: None,
            extra_auth_params: &[],
        };
        let tokens = TokenSource::new(provider, store, "k".into());
        tokens
            .save(&Tokens {
                access_token: "good".into(),
                refresh_token: Some("r".into()),
                expires_at_ms: chrono::Utc::now().timestamp_millis() + 3_600_000,
            })
            .unwrap();
        Google::with_api(tokens, url)
    }

    fn draft() -> EventDraft {
        EventDraft {
            title: "Lunch".into(),
            location: "Here".into(),
            description: String::new(),
            start_ms: 1_793_440_800_000,
            end_ms: 1_793_444_400_000,
            all_day: false,
            time_zone: "Asia/Kolkata".into(),
            recurrence: Some("FREQ=WEEKLY".into()),
        }
    }

    #[test]
    fn lists_calendars_over_pages_with_their_access() {
        let server = serve(vec![
            (200, vec![], r#"{"items":[{"id":"a@b","summary":"Mine","accessRole":"owner"}],"nextPageToken":"p2"}"#.into()),
            (200, vec![], r#"{"items":[{"id":"h","summary":"Holidays","accessRole":"reader"}]}"#.into()),
        ]);
        let calendars = account(&server.url).calendars().unwrap();
        assert_eq!(calendars.len(), 2);
        assert!(!calendars[0].read_only);
        assert!(calendars[1].read_only);
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].header("Authorization"), Some("Bearer good"));
        assert!(seen[1].path.contains("pageToken=p2"));
    }

    #[test]
    fn sync_follows_pages_and_tells_changed_from_cancelled() {
        let server = serve(vec![
            (200, vec![], r#"{"items":[{"id":"e1","etag":"\"1\"","summary":"Lunch","start":{"dateTime":"2026-10-31T10:00:00+05:30"},"end":{"dateTime":"2026-10-31T11:00:00+05:30"}}],"nextPageToken":"p2"}"#.into()),
            (200, vec![], r#"{"items":[{"id":"e2","status":"cancelled"},{"id":"e3","summary":"Day off","start":{"date":"2026-11-01"},"end":{"date":"2026-11-02"}}],"nextSyncToken":"next"}"#.into()),
        ]);
        let result = account(&server.url).sync("a@b", Some("old")).unwrap();
        assert!(!result.full);
        assert_eq!(result.token.as_deref(), Some("next"));
        assert_eq!(result.removed, ["e2"]);
        assert_eq!(result.changed[0].title, "Lunch");
        assert_eq!(result.changed[0].start_ms, 1_793_421_000_000);
        assert!(result.changed[1].all_day);
        let seen = server.seen.lock().unwrap();
        assert!(seen[0].path.contains("/calendars/a%40b/events?"));
        assert!(seen[0].path.contains("syncToken=old"));
    }

    #[test]
    fn an_old_token_asks_for_a_full_sync() {
        let server = serve(vec![(410, vec![], "{}".into())]);
        assert_eq!(
            account(&server.url).sync("c", Some("old")),
            Err(CalendarError::TokenExpired)
        );
    }

    #[test]
    fn a_rate_limit_passes_on_how_long_to_wait() {
        let server = serve(vec![(
            429,
            vec![("Retry-After", "12".to_owned())],
            "{}".into(),
        )]);
        assert_eq!(
            account(&server.url).sync("c", Some("t")),
            Err(CalendarError::Busy {
                retry_after: Some(12)
            })
        );
    }

    #[test]
    fn creating_sends_the_zone_and_the_repeat() {
        let server = serve(vec![(200, vec![], r#"{"id":"new","etag":"\"1\"","summary":"Lunch","start":{"dateTime":"2026-10-31T10:00:00Z"},"end":{"dateTime":"2026-10-31T11:00:00Z"}}"#.into())]);
        let event = account(&server.url).create("c", &draft()).unwrap();
        assert_eq!(event.id, "new");
        let seen = server.seen.lock().unwrap();
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["start"]["timeZone"], "Asia/Kolkata");
        assert_eq!(body["recurrence"][0], "RRULE:FREQ=WEEKLY");
    }

    #[test]
    fn changing_quotes_the_etag_and_a_clash_is_a_conflict() {
        let server = serve(vec![(412, vec![], "{}".into())]);
        let result = account(&server.url).update("c", "e1", "\"7\"", &draft());
        assert_eq!(result, Err(CalendarError::Conflict));
        assert_eq!(
            server.seen.lock().unwrap()[0].header("If-Match"),
            Some("\"7\"")
        );
    }

    #[test]
    fn deleting_what_is_gone_succeeds() {
        let server = serve(vec![
            (410, vec![], "{}".into()),
            (204, vec![], String::new()),
        ]);
        let google = account(&server.url);
        assert_eq!(google.delete("c", "e1", "\"7\""), Ok(()));
        assert_eq!(google.delete("c", "e1", "\"7\""), Ok(()));
    }

    #[test]
    fn an_expired_token_is_refreshed_and_the_call_repeated() {
        let server = serve(vec![
            (401, vec![], "{}".into()),
            (
                200,
                vec![],
                r#"{"access_token":"fresh","expires_in":3600}"#.into(),
            ),
            (200, vec![], r#"{"items":[]}"#.into()),
        ]);
        assert_eq!(account(&server.url).calendars(), Ok(Vec::new()));
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[1].path, "/token");
        assert_eq!(seen[2].header("Authorization"), Some("Bearer fresh"));
    }

    #[test]
    fn without_a_client_id_it_is_not_configured() {
        // Reads the environment, set by nobody in tests: nothing is made up instead.
        if std::env::var_os("KEENU_GOOGLE_CLIENT_ID").is_none()
            && option_env!("KEENU_GOOGLE_CLIENT_ID").is_none()
        {
            assert!(provider().is_none());
        }
    }
}
