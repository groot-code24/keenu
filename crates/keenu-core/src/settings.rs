// SPDX-License-Identifier: GPL-3.0-or-later

//! What the user chose, on the hosts that keep their own settings (Windows and macOS; on Linux the
//! GNOME extension uses GSettings). Anything that watches, records or reaches the network starts off,
//! and no secret is ever kept here: those go to the operating system's credential store.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "settings.json";
const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub schema: u32,
    /// The first-run screens were finished or skipped.
    pub onboarded: bool,
    pub clipboard: ClipboardSettings,
    pub usage: UsageSettings,
    pub voice: VoiceSettings,
    pub lyrics: LyricsSettings,
    pub updates: UpdateSettings,
    pub notifications: NotificationSettings,
    pub window: WindowSettings,
    pub agents: AgentSettings,
    pub calendar: CalendarSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            onboarded: false,
            clipboard: ClipboardSettings::default(),
            usage: UsageSettings::default(),
            voice: VoiceSettings::default(),
            lyrics: LyricsSettings::default(),
            updates: UpdateSettings::default(),
            notifications: NotificationSettings::default(),
            window: WindowSettings::default(),
            agents: AgentSettings::default(),
            calendar: CalendarSettings::default(),
        }
    }
}

impl Settings {
    /// Brings values edited by hand, or from another version, back into range.
    pub fn sanitized(mut self) -> Self {
        self.schema = SCHEMA;
        self.clipboard.max_items = self.clipboard.max_items.clamp(1, 1000);
        self.clipboard.retention_days = self.clipboard.retention_days.clamp(1, 365);
        self.usage.retention_days = self.usage.retention_days.clamp(1, 730);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipboardSettings {
    /// Opt-in: nothing is read from the clipboard until this is on.
    pub enabled: bool,
    /// On but temporarily not recording.
    pub paused: bool,
    /// Unpinned items kept.
    pub max_items: usize,
    pub retention_days: u32,
    pub capture_images: bool,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            max_items: 100,
            retention_days: 30,
            capture_images: true,
        }
    }
}

impl ClipboardSettings {
    pub fn recording(&self) -> bool {
        self.enabled && !self.paused
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageSettings {
    /// Opt-in: the app in front is not looked at until this is on.
    pub enabled: bool,
    pub paused: bool,
    pub retention_days: u32,
    /// The user's own category per application identifier.
    pub categories: BTreeMap<String, String>,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            retention_days: 30,
            categories: BTreeMap::new(),
        }
    }
}

impl UsageSettings {
    pub fn recording(&self) -> bool {
        self.enabled && !self.paused
    }
}

/// Which speech engine to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeechEngine {
    /// Processed on this computer, by the system's own engine where it has one.
    #[default]
    Local,
    /// An OpenAI-compatible endpoint the user configured.
    Remote,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceSettings {
    /// Opt-in; even then the microphone only opens while the user holds or starts a recording.
    pub enabled: bool,
    /// The microphone's device id; empty is the system default.
    pub device: String,
    pub stt_engine: SpeechEngine,
    pub tts_engine: SpeechEngine,
    /// Base URL of an OpenAI-compatible API, such as `https://host/v1`. The key is in the credential store.
    pub endpoint: String,
    pub stt_model: String,
    pub tts_model: String,
    pub tts_voice: String,
    pub chat_model: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LyricsSettings {
    /// Opt-in: sends the playing song's title and artist to LRCLIB.
    pub online: bool,
    /// Where `Artist - Title.lrc` files are looked for; empty is the Music folder's `Lyrics`.
    pub folder: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Prerelease,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSettings {
    /// Opt-in: looks at the release manifest once a day.
    pub check: bool,
    /// Opt-in on top of `check`: installs a verified update without asking first.
    pub auto_install: bool,
    pub channel: UpdateChannel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    pub enabled: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowSettings {
    /// The display to sit on, by the system's name for it; falls back to the primary if it is gone.
    pub monitor: String,
    /// Steps aside while another application is fullscreen.
    pub hide_in_fullscreen: bool,
    pub start_at_login: bool,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            monitor: String::new(),
            hide_in_fullscreen: true,
            start_at_login: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    /// Seconds a request waits for an answer before going back to the agent's own terminal prompt.
    pub answer_timeout_secs: u32,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            answer_timeout_secs: 300,
        }
    }
}

/// One connected calendar account. Tokens and passwords are in the credential store under `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarAccount {
    pub id: String,
    /// `caldav`, `google` or `microsoft`.
    pub provider: String,
    /// What the user sees: an address, usually.
    pub label: String,
    /// CalDAV's server address, HTTPS only; unused by the others.
    #[serde(default)]
    pub server: String,
    /// CalDAV's user name.
    #[serde(default)]
    pub username: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarSettings {
    pub accounts: Vec<CalendarAccount>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_that_watches_or_reaches_out_starts_off() {
        let settings = Settings::default();
        assert!(!settings.clipboard.enabled);
        assert!(!settings.clipboard.recording());
        assert!(!settings.usage.enabled);
        assert!(!settings.voice.enabled);
        assert!(!settings.lyrics.online);
        assert!(!settings.updates.check);
        assert!(!settings.updates.auto_install);
        assert!(!settings.window.start_at_login);
    }

    #[test]
    fn a_paused_feature_is_not_recording() {
        let mut settings = Settings::default();
        settings.clipboard.enabled = true;
        assert!(settings.clipboard.recording());
        settings.clipboard.paused = true;
        assert!(!settings.clipboard.recording());
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"onboarded": true}"#).unwrap();
        assert!(settings.onboarded);
        assert_eq!(settings.clipboard.max_items, 100);
        assert!(settings.window.hide_in_fullscreen);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let settings: Settings = serde_json::from_str(r#"{"from_the_future": 1}"#).unwrap();
        assert!(!settings.onboarded);
    }

    #[test]
    fn hand_edited_values_are_brought_into_range() {
        let mut settings = Settings::default();
        settings.clipboard.max_items = 0;
        settings.clipboard.retention_days = 100_000;
        settings.usage.retention_days = 0;
        let settings = settings.sanitized();
        assert_eq!(settings.clipboard.max_items, 1);
        assert_eq!(settings.clipboard.retention_days, 365);
        assert_eq!(settings.usage.retention_days, 1);
    }
}
