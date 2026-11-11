// SPDX-License-Identifier: GPL-3.0-or-later

//! Microsoft Graph calendars, with `calendarView/delta` for incremental sync.
//!
//! Needs an app registration of your own; see `docs/calendar-setup.md`.

use serde_json::{Value, json};

use super::{
    CalendarError, CalendarInfo, CalendarProvider, Event, EventDraft, SyncResult, date_to_ms,
    ms_to_date, ms_to_rfc3339, parse_time_ms, status_error,
};
use crate::http;
use crate::oauth::{Provider, TokenSource};

const SERVICE: &str = "Microsoft 365";
const API: &str = "https://graph.microsoft.com/v1.0";
/// How far ahead a delta looks: Graph needs a fixed window for it.
const WINDOW_DAYS: i64 = 365;

/// The application's registration: from the environment, or compiled in by whoever builds Keenu.
pub fn provider() -> Option<Provider> {
    let client_id = std::env::var("KEENU_MICROSOFT_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("KEENU_MICROSOFT_CLIENT_ID").map(str::to_owned))
        .filter(|id| !id.is_empty())?;
    Some(Provider {
        name: SERVICE,
        auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize".into(),
        token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".into(),
        scopes: "offline_access User.Read Calendars.ReadWrite",
        client_id,
        client_secret: None,
        extra_auth_params: &[],
    })
}

/// The same registration, for the assistant to use the user's Outlook mail: reading, drafting and sending.
pub fn mail_provider() -> Option<Provider> {
    provider().map(|registered| Provider {
        name: "Outlook mail",
        scopes: "offline_access User.Read Mail.ReadWrite Mail.Send",
        ..registered
    })
}

/// One Microsoft account.
pub struct Graph {
    tokens: TokenSource,
    api: String,
}

impl Graph {
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

    /// Calls `url`; on a 401 refreshes the token and tries once more. Times come back in UTC.
    fn call(
        &self,
        method: &str,
        url: &str,
        extra: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<http::Response, CalendarError> {
        // nextLink and deltaLink come from the server: only ever followed on the same one.
        if !url.starts_with(&self.api) {
            return Err(CalendarError::Failed(format!(
                "{SERVICE} pointed somewhere else; it was not followed."
            )));
        }
        let text = body.map(Value::to_string);
        let send = |token: &str| {
            let bearer = format!("Bearer {token}");
            let mut headers = vec![
                ("Authorization", bearer.as_str()),
                ("Accept", "application/json"),
                ("Content-Type", "application/json"),
                ("Prefer", "outlook.timezone=\"UTC\""),
            ];
            headers.extend_from_slice(extra);
            http::send(method, url, &headers, text.as_deref().map(str::as_bytes))
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

fn to_event(item: &Value) -> Option<Event> {
    let all_day = item["isAllDay"].as_bool().unwrap_or(false);
    let time = |field: &Value| -> Option<i64> {
        let value = field["dateTime"].as_str()?;
        if all_day {
            date_to_ms(value.get(..10)?)
        } else {
            parse_time_ms(value)
        }
    };
    Some(Event {
        id: item["id"].as_str()?.to_owned(),
        etag: item["@odata.etag"].as_str().unwrap_or_default().to_owned(),
        title: item["subject"].as_str().unwrap_or_default().to_owned(),
        location: item["location"]["displayName"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        start_ms: time(&item["start"])?,
        end_ms: time(&item["end"])?,
        all_day,
        ics: None,
    })
}

/// Graph writes repeats as objects, not rules: the common RFC 5545 rules translated, others refused rather
/// than saved wrong.
fn recurrence_body(rule: &str, start_ms: i64) -> Result<Value, CalendarError> {
    let parsed = rule
        .split(';')
        .filter_map(|part| part.split_once('='))
        .collect::<Vec<_>>();
    let get = |key: &str| {
        parsed
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    };
    let kind = match get("FREQ") {
        Some("DAILY") => "daily",
        Some("WEEKLY") => "weekly",
        Some("MONTHLY") => "absoluteMonthly",
        Some("YEARLY") => "absoluteYearly",
        _ => {
            return Err(CalendarError::Failed(
                "Microsoft 365 cannot take that repeat.".into(),
            ));
        }
    };
    let start = ms_to_date(start_ms)
        .ok_or_else(|| CalendarError::Failed("That time is out of range.".into()))?;
    let date = chrono::NaiveDate::parse_from_str(&start, "%Y-%m-%d")
        .map_err(|_| CalendarError::Failed("That time is out of range.".into()))?;
    use chrono::Datelike;
    let mut pattern = json!({
        "type": kind,
        "interval": get("INTERVAL").and_then(|n| n.parse::<u32>().ok()).unwrap_or(1),
    });
    if kind == "weekly" {
        let names = |code: &str| match code {
            "MO" => "monday",
            "TU" => "tuesday",
            "WE" => "wednesday",
            "TH" => "thursday",
            "FR" => "friday",
            "SA" => "saturday",
            _ => "sunday",
        };
        let days: Vec<&str> = match get("BYDAY") {
            Some(days) => days.split(',').map(names).collect(),
            None => vec![names(&date.weekday().to_string().to_uppercase()[..2])],
        };
        pattern["daysOfWeek"] = json!(days);
    }
    if kind.starts_with("absolute") {
        pattern["dayOfMonth"] = json!(date.day());
    }
    if kind == "absoluteYearly" {
        pattern["month"] = json!(date.month());
    }
    let range = match get("COUNT").and_then(|n| n.parse::<u32>().ok()) {
        Some(count) => {
            json!({"type": "numbered", "startDate": start, "numberOfOccurrences": count})
        }
        None => json!({"type": "noEnd", "startDate": start}),
    };
    Ok(json!({"pattern": pattern, "range": range}))
}

fn to_body(draft: &EventDraft) -> Result<Value, CalendarError> {
    let out_of_range = || CalendarError::Failed("That time is out of range.".to_owned());
    let time = |ms: i64| -> Result<Value, CalendarError> {
        let value = if draft.all_day {
            format!("{}T00:00:00", ms_to_date(ms).ok_or_else(out_of_range)?)
        } else {
            ms_to_rfc3339(ms)
                .ok_or_else(out_of_range)?
                .trim_end_matches('Z')
                .to_owned()
        };
        Ok(json!({"dateTime": value, "timeZone": "UTC"}))
    };
    let mut body = json!({
        "subject": draft.title,
        "body": {"contentType": "text", "content": draft.description},
        "location": {"displayName": draft.location},
        "isAllDay": draft.all_day,
        "start": time(draft.start_ms)?,
        "end": time(draft.end_ms)?,
    });
    if let Some(rule) = &draft.recurrence {
        body["recurrence"] = recurrence_body(rule, draft.start_ms)?;
    }
    Ok(body)
}

impl CalendarProvider for Graph {
    fn calendars(&self) -> Result<Vec<CalendarInfo>, CalendarError> {
        let mut calendars = Vec::new();
        let mut url = format!("{}/me/calendars", self.api);
        loop {
            let reply = self.json(self.call("GET", &url, &[], None)?)?;
            for item in reply["value"].as_array().into_iter().flatten() {
                let Some(id) = item["id"].as_str() else {
                    continue;
                };
                calendars.push(CalendarInfo {
                    id: id.to_owned(),
                    name: item["name"].as_str().unwrap_or(id).to_owned(),
                    read_only: !item["canEdit"].as_bool().unwrap_or(false),
                });
            }
            match reply["@odata.nextLink"].as_str() {
                Some(next) => url = next.to_owned(),
                None => return Ok(calendars),
            }
        }
    }

    fn sync(&self, calendar: &str, token: Option<&str>) -> Result<SyncResult, CalendarError> {
        let mut result = SyncResult {
            full: token.is_none(),
            ..SyncResult::default()
        };
        let mut url = match token {
            // The token is the delta link itself.
            Some(link) => link.to_owned(),
            None => {
                let now = chrono::Utc::now();
                let format = "%Y-%m-%dT%H:%M:%SZ";
                let from = (now - chrono::Duration::days(30))
                    .format(format)
                    .to_string();
                let to = (now + chrono::Duration::days(WINDOW_DAYS))
                    .format(format)
                    .to_string();
                format!(
                    "{}/me/calendars/{}/calendarView/delta?startDateTime={}&endDateTime={}",
                    self.api,
                    http::encode_component(calendar),
                    http::encode_component(&from),
                    http::encode_component(&to),
                )
            }
        };
        loop {
            let reply = self.json(self.call("GET", &url, &[], None)?)?;
            for item in reply["value"].as_array().into_iter().flatten() {
                if item.get("@removed").is_some() {
                    result
                        .removed
                        .extend(item["id"].as_str().map(str::to_owned));
                } else {
                    result.changed.extend(to_event(item));
                }
            }
            if let Some(next) = reply["@odata.nextLink"].as_str() {
                url = next.to_owned();
                continue;
            }
            result.token = reply["@odata.deltaLink"].as_str().map(str::to_owned);
            return Ok(result);
        }
    }

    fn create(&self, calendar: &str, draft: &EventDraft) -> Result<Event, CalendarError> {
        let url = format!(
            "{}/me/calendars/{}/events",
            self.api,
            http::encode_component(calendar)
        );
        let reply = self.json(self.call("POST", &url, &[], Some(&to_body(draft)?))?)?;
        to_event(&reply)
            .ok_or_else(|| CalendarError::Failed(format!("{SERVICE} sent an unreadable event.")))
    }

    fn update(
        &self,
        _calendar: &str,
        event: &str,
        etag: &str,
        draft: &EventDraft,
    ) -> Result<Event, CalendarError> {
        let url = format!("{}/me/events/{}", self.api, http::encode_component(event));
        let reply =
            self.json(self.call("PATCH", &url, &[("If-Match", etag)], Some(&to_body(draft)?))?)?;
        to_event(&reply)
            .ok_or_else(|| CalendarError::Failed(format!("{SERVICE} sent an unreadable event.")))
    }

    fn delete(&self, _calendar: &str, event: &str, etag: &str) -> Result<(), CalendarError> {
        let url = format!("{}/me/events/{}", self.api, http::encode_component(event));
        let answer = self.call("DELETE", &url, &[("If-Match", etag)], None)?;
        match status_error(answer.status, answer.retry_after(), SERVICE) {
            None | Some(CalendarError::NotFound) => Ok(()),
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

    fn account(url: &str) -> Graph {
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
        Graph::with_api(tokens, url)
    }

    fn draft(recurrence: Option<&str>) -> EventDraft {
        EventDraft {
            title: "Lunch".into(),
            location: "Here".into(),
            description: "Notes".into(),
            start_ms: 1_793_440_800_000,
            end_ms: 1_793_444_400_000,
            all_day: false,
            time_zone: "Asia/Kolkata".into(),
            recurrence: recurrence.map(str::to_owned),
        }
    }

    #[test]
    fn a_delta_follows_next_links_and_keeps_the_delta_link() {
        let server = serve(vec![
            (200, vec![], r#"{"value":[{"id":"e1","@odata.etag":"W/\"1\"","subject":"Lunch","isAllDay":false,"start":{"dateTime":"2026-10-31T04:30:00.0000000"},"end":{"dateTime":"2026-10-31T05:30:00.0000000"},"location":{"displayName":"Here"}}],"@odata.nextLink":"{{URL}}/page2"}"#.into()),
            (200, vec![], r#"{"value":[{"id":"gone","@removed":{"reason":"deleted"}}],"@odata.deltaLink":"{{URL}}/delta2"}"#.into()),
        ]);
        let graph = account(&server.url);
        let start = format!("{}/delta1", server.url);
        let result = graph.sync("cal", Some(&start)).unwrap();
        assert_eq!(result.changed[0].title, "Lunch");
        assert_eq!(result.removed, ["gone"]);
        assert_eq!(result.token, Some(format!("{}/delta2", server.url)));
        assert!(!result.full);
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[1].path, "/page2");
        assert_eq!(seen[0].header("Prefer"), Some("outlook.timezone=\"UTC\""));
    }

    #[test]
    fn reads_events_in_utc_and_all_day_ones() {
        let event = to_event(&json!({
            "id": "e1", "@odata.etag": "W/\"1\"", "subject": "Lunch", "isAllDay": false,
            "start": {"dateTime": "2026-10-31T04:30:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-10-31T05:30:00.0000000", "timeZone": "UTC"},
            "location": {"displayName": "Here"},
        }))
        .unwrap();
        assert_eq!(event.start_ms, 1_793_421_000_000);
        assert_eq!(event.location, "Here");
        let day = to_event(&json!({
            "id": "d", "isAllDay": true,
            "start": {"dateTime": "2026-11-01T00:00:00.0000000"},
            "end": {"dateTime": "2026-11-02T00:00:00.0000000"},
        }))
        .unwrap();
        assert!(day.all_day);
    }

    #[test]
    fn a_link_pointing_at_another_server_is_not_followed() {
        let server = serve(Vec::new());
        let result = account(&server.url).sync("cal", Some("https://evil.example/steal"));
        assert!(matches!(result, Err(CalendarError::Failed(_))));
    }

    #[test]
    fn an_expired_delta_asks_for_a_full_sync() {
        let server = serve(vec![(410, vec![], "{}".into())]);
        let link = format!("{}/delta", server.url);
        assert_eq!(
            account(&server.url).sync("cal", Some(&link)),
            Err(CalendarError::TokenExpired)
        );
    }

    #[test]
    fn a_full_sync_asks_for_a_window() {
        let server = serve(vec![(
            200,
            vec![],
            r#"{"value":[],"@odata.deltaLink":"x"}"#.into(),
        )]);
        let result = account(&server.url).sync("c a l", None).unwrap();
        assert!(result.full);
        let seen = server.seen.lock().unwrap();
        assert!(
            seen[0]
                .path
                .contains("/me/calendars/c%20a%20l/calendarView/delta?startDateTime=")
        );
    }

    #[test]
    fn translates_common_repeats_and_refuses_others() {
        let weekly = recurrence_body(
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR;COUNT=4",
            1_793_440_800_000,
        )
        .unwrap();
        assert_eq!(weekly["pattern"]["type"], "weekly");
        assert_eq!(weekly["pattern"]["interval"], 2);
        assert_eq!(weekly["pattern"]["daysOfWeek"], json!(["monday", "friday"]));
        assert_eq!(weekly["range"]["numberOfOccurrences"], 4);
        let monthly = recurrence_body("FREQ=MONTHLY", 1_793_440_800_000).unwrap();
        assert_eq!(monthly["pattern"]["type"], "absoluteMonthly");
        assert_eq!(monthly["range"]["type"], "noEnd");
        assert!(recurrence_body("FREQ=SECONDLY", 0).is_err());
    }

    #[test]
    fn writing_uses_utc_and_the_etag() {
        let server = serve(vec![(412, vec![], "{}".into())]);
        let result = account(&server.url).update("", "e/1", "W/\"3\"", &draft(None));
        assert_eq!(result, Err(CalendarError::Conflict));
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen[0].method, "PATCH");
        assert_eq!(seen[0].path, "/me/events/e%2F1");
        assert_eq!(seen[0].header("If-Match"), Some("W/\"3\""));
        let body: Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["start"]["timeZone"], "UTC");
        assert_eq!(body["subject"], "Lunch");
    }
}
