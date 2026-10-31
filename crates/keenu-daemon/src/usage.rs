// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! AI plan usage for the island, from what is on this computer: Codex's own session logs for ChatGPT's limits, and
//! the plan's name for Claude and Grok. Sign-in tokens are never read or sent: a provider's sign-in is for its own
//! program, and each program's /usage shows what Keenu cannot. Warns once as a limit nears.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::{fdo, interface};

use crate::clock::now_ms;
use crate::store::Store;

/// Codex's local logs are read each minute while signed in. Nothing runs while locked or with usage off.
const LOCAL_EVERY: Duration = Duration::from_secs(60);
/// Without Codex signed in, the sign-in files are looked for this often.
const NOTHING_SIGNED_IN_EVERY: Duration = Duration::from_secs(30 * 60);
/// The shortest sleep, so a deadline just missed never spins.
const MIN_WAIT: Duration = Duration::from_secs(1);
/// Only the end of a Codex log is read: its newest limits are written last.
const CODEX_TAIL: u64 = 256 * 1024;
/// Only Codex's newest session logs are looked at.
const CODEX_FILES: usize = 8;
/// Said where a provider keeps its limits to itself.
const CLAUDE_ELSEWHERE: &str = "Run /usage in Claude Code to see this plan's limits";
const GROK_ELSEWHERE: &str = "Run /usage in Grok to see this plan's limits";

/// Every plan's limits, as the page draws them.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Usage {
    pub providers: Vec<Provider>,
}

/// One AI's plan and its limits.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub plan: String,
    /// Each limit the plan has, as it reports them: a 5-hour session, a week, a model's week…
    pub windows: Vec<Window>,
    /// When the numbers were read; 0 when never.
    pub updated_ms: i64,
    /// Why the numbers may be old ("As of Codex's last use"); empty when fresh.
    pub note: String,
    /// With no limits to show, why; empty for the page's own words.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub empty: String,
}

/// One limit: how much is used and when it starts over.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Window {
    pub label: String,
    pub percent: f64,
    /// 0 when unknown.
    pub resets_at_ms: i64,
}

/// What the extension sends from Settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Config {
    enabled: bool,
    claude: bool,
    codex: bool,
    /// Missing from an older extension: on, like the setting's own default.
    #[serde(default = "on")]
    grok: bool,
    /// The AI assistant turned on: off, a plan seen here starts no AI to list its models.
    #[serde(default = "on")]
    assistant: bool,
    warn: f64,
    critical: f64,
}

/// Serde default for settings an older extension leaves out.
fn on() -> bool {
    true
}

impl Default for Config {
    /// Usage off, warnings at 80% and 95%.
    fn default() -> Self {
        // Off until the extension says otherwise.
        Self {
            enabled: false,
            claude: true,
            codex: true,
            grok: true,
            assistant: true,
            warn: 80.0,
            critical: 95.0,
        }
    }
}

/// The usage page's D-Bus side: its config and the last numbers.
pub struct UsageService {
    usage: Usage,
    config: Config,
    refresh: Arc<Notify>,
}

impl UsageService {
    /// Also returns the handle that wakes the reading loop.
    pub fn new() -> (Self, Arc<Notify>) {
        let refresh = Arc::new(Notify::new());
        (
            Self {
                usage: Usage::default(),
                config: Config::default(),
                refresh: refresh.clone(),
            },
            refresh,
        )
    }
}

#[interface(name = "io.github.zickrian.isle.Usage1")]
impl UsageService {
    /// JSON `{enabled, claude, codex, grok, assistant, warn, critical}`, from Settings.
    fn set_config(&mut self, config: &str) -> fdo::Result<()> {
        self.config =
            serde_json::from_str(config).map_err(|err| fdo::Error::InvalidArgs(err.to_string()))?;
        crate::assistant::set_assistant_on(self.config.assistant);
        self.refresh.notify_one();
        Ok(())
    }

    /// Reads the usage again now.
    fn refresh(&self) {
        self.refresh.notify_one();
    }

    /// JSON `{providers: [{id, name, plan, windows: [{label, percent, resets_at_ms}], updated_ms, note}]}`.
    #[zbus(property)]
    fn usage(&self) -> String {
        serde_json::to_string(&self.usage).unwrap_or_default()
    }

    /// A limit passed `percent` (the warning or the critical mark) of its window.
    #[zbus(signal)]
    async fn alert(
        emitter: &SignalEmitter<'_>,
        provider: &str,
        window: &str,
        percent: f64,
        resets_at_ms: i64,
    ) -> zbus::Result<()>;
}

/// The marks already warned about, by "provider/window/reset": 1 the warning, 2 the critical one.
type Alerted = BTreeMap<String, u8>;

/// Reads the usage now and then, publishes it, and warns as limits near.
pub async fn run(iface: InterfaceRef<UsageService>, refresh: Arc<Notify>) {
    let mut store: Store<Alerted> = Store::new("usage-alerts");
    let mut alerted = store.load();
    let mut codex = CodexReader::default();
    // Unlocking wakes the loop, to read what came meanwhile.
    let mut locked = crate::system::locked();
    // Until Settings sends its config, and while usage is off, no timer at all: `None`.
    let mut next = None;
    loop {
        let forced = match next {
            None => {
                refresh.notified().await;
                true
            }
            Some(wait) => tokio::select! {
                () = tokio::time::sleep(wait) => false,
                () = refresh.notified() => true,
                Ok(()) = locked.changed() => false,
            },
        };
        let paused = *locked.borrow_and_update();
        let config = iface.get().await.config.clone();
        if !config.enabled {
            next = None;
            publish(&iface, Usage::default()).await;
            continue;
        }
        let mut providers = Vec::new();
        if config.claude && crate::assistant::claude_signed_in() {
            let claude = claude_shell();
            // Another plan than the assistant knows: it looks at the models again.
            crate::assistant::plan_seen("claude", &claude.plan);
            providers.push(claude);
        }
        if config.grok && grok_auth().exists() {
            let grok = grok_shell();
            crate::assistant::plan_seen("grok", &grok.plan);
            providers.push(grok);
        }
        if config.codex && paused && !forced {
            // Locked, nobody sees them: the last numbers stand until unlocking wakes the loop.
            providers.extend(codex.shown.clone());
        } else if config.codex {
            let mut reader = std::mem::take(&mut codex);
            if let Ok((back, read)) = tokio::task::spawn_blocking(move || {
                let read = reader.read();
                (reader, read)
            })
            .await
            {
                codex = back;
                providers.extend(read);
            }
        }
        for provider in &mut providers {
            settle(provider, now_ms());
        }
        let warnings = marks_passed(&providers, &config, &mut alerted, now_ms());
        if !warnings.is_empty() {
            store.save(&alerted);
        }
        for (provider, window, percent, resets_at_ms) in warnings {
            let emitter = iface.signal_emitter();
            if let Err(err) =
                UsageService::alert(emitter, &provider, &window, percent, resets_at_ms).await
            {
                warn!("could not send a usage alert: {err}");
            }
        }
        next = Some(next_wait(
            config.codex && !paused && codex_signed_in(),
            next_reset(&providers, now_ms()),
        ));
        publish(&iface, Usage { providers }).await;
    }
}

/// How long to sleep: until a window resets, and at most a minute while Codex is signed in; otherwise only
/// now and then, for a sign-in to come.
fn next_wait(codex_signed_in: bool, next_reset: Option<Duration>) -> Duration {
    let longest = if codex_signed_in {
        LOCAL_EVERY
    } else {
        NOTHING_SIGNED_IN_EVERY
    };
    next_reset
        .map_or(longest, |reset| reset.min(longest))
        .max(MIN_WAIT)
}

/// How long until the next window resets, when `settle` starts it over.
fn next_reset(providers: &[Provider], now: i64) -> Option<Duration> {
    providers
        .iter()
        .flat_map(|provider| &provider.windows)
        .filter(|window| window.resets_at_ms > now)
        .map(|window| Duration::from_millis((window.resets_at_ms - now) as u64))
        .min()
}

/// Whether Codex is signed in, so its logs may yet bring limits worth a look each minute.
fn codex_signed_in() -> bool {
    crate::assistant::codex_home().join("auth.json").exists()
}

/// Stores and signals new numbers; the same ones again send nothing.
async fn publish(iface: &InterfaceRef<UsageService>, usage: Usage) {
    let mut service = iface.get_mut().await;
    if service.usage == usage {
        return;
    }
    service.usage = usage;
    if let Err(err) = service.usage_changed(iface.signal_emitter()).await {
        warn!("could not publish the AI usage: {err}");
    }
}

/// A window whose reset has passed starts over at 0, until the numbers are read again.
fn settle(provider: &mut Provider, now: i64) {
    for window in &mut provider.windows {
        if window.resets_at_ms > 0 && window.resets_at_ms <= now {
            window.percent = 0.0;
        }
    }
}

/// The marks newly passed, as (provider, window, percent, reset); each is warned about once per window.
fn marks_passed(
    providers: &[Provider],
    config: &Config,
    alerted: &mut Alerted,
    now: i64,
) -> Vec<(String, String, f64, i64)> {
    // Windows that have reset are forgotten; one with no known reset (0) only once it is low again.
    alerted.retain(|key, _| {
        key.rsplit('/')
            .next()
            .and_then(|r| r.parse::<i64>().ok())
            .is_none_or(|r| r == 0 || r > now)
    });
    let mut passed = Vec::new();
    for provider in providers {
        for window in &provider.windows {
            let level = if window.percent >= config.critical {
                2
            } else if window.percent >= config.warn {
                1
            } else {
                0
            };
            // A reset read again a moment off (its fraction of a second changes) is the same window.
            let prefix = format!("{}/{}/", provider.id, window.label);
            let key = alerted
                .keys()
                .find(|key| {
                    key.strip_prefix(&prefix)
                        .and_then(|r| r.parse::<i64>().ok())
                        .is_some_and(|r| (r - window.resets_at_ms).abs() < 60_000)
                })
                .cloned()
                .unwrap_or_else(|| format!("{prefix}{}", window.resets_at_ms));
            if level == 0 && window.resets_at_ms == 0 {
                alerted.remove(&key);
            }
            if level > alerted.get(&key).copied().unwrap_or(0) {
                alerted.insert(key, level);
                passed.push((
                    provider.name.clone(),
                    window.label.clone(),
                    window.percent,
                    window.resets_at_ms,
                ));
            }
        }
    }
    passed
}

/// Claude with its plan's name. Its limits stay with Claude Code: reading them would take its sign-in.
fn claude_shell() -> Provider {
    Provider {
        id: "claude".into(),
        name: "Claude".into(),
        // As the assistant's Settings and Claude Code name it ("Team").
        plan: crate::assistant::claude_plan(),
        empty: CLAUDE_ELSEWHERE.into(),
        ..Provider::default()
    }
}

/// Where Grok keeps its sign-in, which is only looked for, never read.
fn grok_auth() -> std::path::PathBuf {
    crate::assistant::grok_home().join("auth.json")
}

/// Grok with its plan's name, as Grok last cached it.
fn grok_shell() -> Provider {
    Provider {
        id: "grok".into(),
        name: "Grok".into(),
        plan: crate::assistant::grok_plan(),
        empty: GROK_ELSEWHERE.into(),
        ..Provider::default()
    }
}

/// ChatGPT's plan and limits as Codex last saw them in its logs (read again only when its newest log changed).
#[derive(Default)]
struct CodexReader {
    seen: Option<(PathBuf, std::time::SystemTime)>,
    last: Option<Provider>,
    /// What `read` gave last, shown again while the screen is locked.
    shown: Option<Provider>,
}

impl CodexReader {
    /// What Codex logged; `None` without Codex.
    fn read(&mut self) -> Option<Provider> {
        self.shown = self.read_logs();
        if let Some(shown) = &self.shown {
            // Another plan than the assistant knows: it looks at the models again.
            crate::assistant::plan_seen("codex", &shown.plan);
        }
        self.shown.clone()
    }

    /// The limits in Codex's newest logs. Both the user's Codex and Keenu's are looked at.
    fn read_logs(&mut self) -> Option<Provider> {
        // Each folder's newest, so a long history of the user's own never hides Keenu's fresh ones.
        let mut files = Vec::new();
        for sessions in [
            crate::assistant::codex_home().join("sessions"),
            crate::store::data_dir().join("codex/sessions"),
        ] {
            let mut newest = Vec::new();
            newest_logs(&sessions, &mut newest);
            files.extend(newest);
        }
        let modified = |file: &PathBuf| std::fs::metadata(file).and_then(|m| m.modified()).ok();
        files.sort_by_key(|file| std::cmp::Reverse(modified(file)));
        let newest = files
            .first()
            .and_then(|file| Some((file.clone(), modified(file)?)));
        if newest.is_some() && newest == self.seen {
            return self.last.clone();
        }
        self.seen = newest;
        self.last = files
            .iter()
            .take(CODEX_FILES)
            .find_map(|file| codex_limits(file));
        self.last.clone()
    }
}

/// The newest limits in one Codex log, from its last part.
fn codex_limits(file: &Path) -> Option<Provider> {
    use std::io::{Read, Seek, SeekFrom};
    let mut log = std::fs::File::open(file).ok()?;
    let length = log.metadata().ok()?.len();
    log.seek(SeekFrom::Start(length.saturating_sub(CODEX_TAIL)))
        .ok()?;
    let mut tail = Vec::new();
    log.read_to_end(&mut tail).ok()?;
    let text = String::from_utf8_lossy(&tail);
    let (limits, at) = text.lines().rev().find_map(|line| {
        // Most lines are messages; only parse the ones that can hold limits.
        if !line.contains("rate_limits") {
            return None;
        }
        let event: Value = serde_json::from_str(line).ok()?;
        let limits = find_key(&event, "rate_limits")?.clone();
        Some((limits, event["timestamp"].as_str().map_or(0, rfc3339_ms)))
    })?;
    let modified = std::fs::metadata(file)
        .and_then(|m| m.modified())
        .map(|t| chrono::DateTime::<chrono::Local>::from(t).timestamp_millis())
        .unwrap_or(0);
    Some(Provider {
        id: "codex".into(),
        name: "ChatGPT".into(),
        plan: crate::assistant::codex_plan(limits["plan_type"].as_str().unwrap_or_default()),
        windows: codex_windows(&limits),
        updated_ms: if at > 0 { at } else { modified },
        note: "As of Codex’s last use".into(),
        ..Provider::default()
    })
}

/// Codex's session logs, newest first: sessions/<year>/<month>/<day>/*.jsonl.
fn newest_logs(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort_by(|a, b| b.cmp(a));
    for path in entries {
        if files.len() >= CODEX_FILES {
            return;
        }
        if path.is_dir() {
            newest_logs(&path, files);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            files.push(path);
        }
    }
}

/// The primary and secondary limits ChatGPT reports.
fn codex_windows(limits: &Value) -> Vec<Window> {
    ["primary", "secondary"]
        .iter()
        .filter_map(|key| {
            let window = limits.get(*key).filter(|w| !w.is_null())?;
            Some(Window {
                label: window_label(window["window_minutes"].as_i64().unwrap_or(0)),
                percent: window["used_percent"].as_f64()?,
                resets_at_ms: window["resets_at"].as_i64().map_or(0, |s| s * 1000),
            })
        })
        .collect()
}

/// A window of `minutes` as people say it: "5 hours", "Week", "30 days".
fn window_label(minutes: i64) -> String {
    match minutes {
        300 => "5 hours".to_owned(),
        10080 => "Week".to_owned(),
        m if m > 0 && m % 1440 == 0 => format!("{} days", m / 1440),
        m if m > 0 => format!("{} hours", (m + 30) / 60),
        _ => "Limit".to_owned(),
    }
}

/// The first value under `key`, at any depth.
fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => map
            .get(key)
            .filter(|v| !v.is_null())
            .or_else(|| map.values().find_map(|v| find_key(v, key))),
        Value::Array(items) => items.iter().find_map(|v| find_key(v, key)),
        _ => None,
    }
}

/// 0 when the time does not parse.
fn rfc3339_ms(text: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(text).map_or(0, |t| t.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Sign-ins belong to each provider's own program: nothing here may read one or call home with one.
    #[test]
    fn no_sign_in_token_is_read_or_sent() {
        let source = include_str!("usage.rs");
        // Built up, so this test's own words do not match.
        for word in [
            concat!("access", "Token"),
            concat!("access", "_token"),
            concat!("Bea", "rer "),
            concat!(".creden", "tials.json"),
            concat!("api/oauth", "/usage"),
            concat!("backend-api", "/wham"),
            concat!("billing", "?format"),
        ] {
            assert!(!source.contains(word), "usage.rs mentions {word}");
        }
    }

    #[test]
    fn says_where_a_plans_limits_can_be_seen() {
        assert!(claude_shell().empty.contains("/usage"));
        assert!(grok_shell().empty.contains("/usage"));
        assert!(claude_shell().windows.is_empty() && grok_shell().windows.is_empty());
    }

    #[test]
    fn sleeps_until_something_is_due() {
        let minutes = |m: u64| Duration::from_secs(m * 60);
        // Codex signed in: its logs each minute.
        assert_eq!(next_wait(true, None), LOCAL_EVERY);
        // A window that resets sooner wakes it, to start that window over.
        assert_eq!(next_wait(false, Some(minutes(3))), minutes(3));
        // Nothing to watch: only for a sign-in.
        assert_eq!(next_wait(false, None), NOTHING_SIGNED_IN_EVERY);
        // Overdue never spins.
        assert_eq!(next_wait(true, Some(Duration::ZERO)), MIN_WAIT);
    }

    #[test]
    fn finds_the_next_reset() {
        let provider = |resets: &[i64]| Provider {
            windows: resets
                .iter()
                .map(|&resets_at_ms| Window {
                    resets_at_ms,
                    ..Window::default()
                })
                .collect(),
            ..Provider::default()
        };
        let providers = [provider(&[0, 5_000]), provider(&[900, 3_000])];
        assert_eq!(
            next_reset(&providers, 1_000),
            Some(Duration::from_millis(2_000))
        );
        assert_eq!(next_reset(&providers, 6_000), None);
    }

    #[test]
    fn reads_what_codex_saw() {
        let pro = json!({"primary": {"used_percent": 9.0, "window_minutes": 10080, "resets_at": 1793084579},
            "secondary": null, "plan_type": "prolite"});
        let windows = codex_windows(&pro);
        assert_eq!(windows.len(), 1);
        assert_eq!(
            (windows[0].label.as_str(), windows[0].percent),
            ("Week", 9.0)
        );
        assert_eq!(windows[0].resets_at_ms, 1_793_084_579_000);
        assert_eq!(crate::assistant::codex_plan("prolite"), "Pro");
        let plus = json!({"primary": {"used_percent": 50.0, "window_minutes": 300}, "secondary":
            {"used_percent": 20.0, "window_minutes": 10080}});
        let labels: Vec<_> = codex_windows(&plus).into_iter().map(|w| w.label).collect();
        assert_eq!(labels, ["5 hours", "Week"]);
        let line = json!({"timestamp": "2026-10-21T08:59:00Z", "payload": {"info": {}, "rate_limits": pro}});
        assert_eq!(
            find_key(&line, "rate_limits").unwrap()["plan_type"],
            "prolite"
        );
    }

    #[test]
    fn warns_once_per_mark_and_window() {
        let config = Config {
            enabled: true,
            ..Config::default()
        };
        let mut alerted = Alerted::new();
        let at = |percent: f64| {
            vec![Provider {
                id: "claude".into(),
                name: "Claude".into(),
                windows: vec![Window {
                    label: "5 hours".into(),
                    percent,
                    resets_at_ms: 2_000,
                }],
                ..Provider::default()
            }]
        };
        assert!(marks_passed(&at(50.0), &config, &mut alerted, 1_000).is_empty());
        assert_eq!(
            marks_passed(&at(82.0), &config, &mut alerted, 1_000).len(),
            1
        );
        assert!(marks_passed(&at(85.0), &config, &mut alerted, 1_000).is_empty());
        assert_eq!(
            marks_passed(&at(96.0), &config, &mut alerted, 1_000).len(),
            1
        );
        assert!(marks_passed(&at(99.0), &config, &mut alerted, 1_000).is_empty());
        // Once the window has reset, it may warn again.
        assert!(marks_passed(&[], &config, &mut alerted, 3_000).is_empty() && alerted.is_empty());
    }

    #[test]
    fn a_reset_read_a_moment_off_is_the_same_window() {
        let config = Config {
            enabled: true,
            ..Config::default()
        };
        let mut alerted = Alerted::new();
        let at = |percent: f64, resets_at_ms: i64| {
            vec![Provider {
                id: "claude".into(),
                name: "Claude".into(),
                windows: vec![Window {
                    label: "5 hours".into(),
                    percent,
                    resets_at_ms,
                }],
                ..Provider::default()
            }]
        };
        let reset = 1_793_084_579_000;
        assert_eq!(
            marks_passed(&at(82.0, reset + 123), &config, &mut alerted, 1_000).len(),
            1
        );
        assert!(marks_passed(&at(83.0, reset + 456), &config, &mut alerted, 2_000).is_empty());
        assert!(marks_passed(&at(84.0, reset - 80), &config, &mut alerted, 3_000).is_empty());
        assert_eq!(
            marks_passed(&at(96.0, reset + 7), &config, &mut alerted, 4_000).len(),
            1
        );
        // The next window, hours later, warns again.
        let next = reset + 5 * 3600 * 1000;
        assert_eq!(
            marks_passed(&at(82.0, next), &config, &mut alerted, 5_000).len(),
            1
        );
    }

    #[test]
    fn a_limit_with_no_known_reset_warns_once_until_it_is_low_again() {
        let config = Config {
            enabled: true,
            ..Config::default()
        };
        let mut alerted = Alerted::new();
        let at = |percent: f64| {
            vec![Provider {
                id: "codex".into(),
                name: "ChatGPT".into(),
                windows: vec![Window {
                    label: "Week".into(),
                    percent,
                    resets_at_ms: 0,
                }],
                ..Provider::default()
            }]
        };
        assert_eq!(
            marks_passed(&at(85.0), &config, &mut alerted, 1_000).len(),
            1
        );
        // A minute later, and later still: no repeat.
        assert!(marks_passed(&at(85.0), &config, &mut alerted, 61_000).is_empty());
        assert!(marks_passed(&at(86.0), &config, &mut alerted, 121_000).is_empty());
        // Low again (a reset nobody announced), then high: a new warning.
        assert!(marks_passed(&at(5.0), &config, &mut alerted, 181_000).is_empty());
        assert_eq!(
            marks_passed(&at(83.0), &config, &mut alerted, 241_000).len(),
            1
        );
    }

    #[test]
    fn reads_the_newest_limits_from_the_end_of_a_codex_log() {
        let file = std::env::temp_dir().join(format!("keenu-codex-{}.jsonl", std::process::id()));
        let line = |percent: f64| {
            json!({"timestamp": "2026-10-21T08:00:00Z", "payload": {"rate_limits": {"plan_type": "plus",
                "primary": {"used_percent": percent, "window_minutes": 300, "resets_at": 1793084579}}}})
            .to_string()
        };
        let filler = "x".repeat(300 * 1024);
        std::fs::write(
            &file,
            format!(
                "{}\n{filler}\n{}\n{{\"other\":1}}\n",
                line(10.0),
                line(42.0)
            ),
        )
        .unwrap();
        let codex = codex_limits(&file).unwrap();
        assert_eq!(
            (codex.plan.as_str(), codex.windows[0].percent),
            ("Plus", 42.0)
        );
        std::fs::remove_file(file).unwrap();
    }

    #[test]
    fn a_window_past_its_reset_starts_over() {
        let mut provider = Provider {
            windows: vec![Window {
                label: "Week".into(),
                percent: 70.0,
                resets_at_ms: 1_000,
            }],
            ..Provider::default()
        };
        settle(&mut provider, 2_000);
        assert_eq!(provider.windows[0].percent, 0.0);
    }
}
