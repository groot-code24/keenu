// SPDX-License-Identifier: GPL-3.0-or-later

//! What Keenu needs from an operating system, and nothing about how any one of them does it.
//!
//! Each host implements these against its native APIs (WinRT and Win32 in `keenu-windows`, AppKit in
//! `keenu-macos`); Linux's are the D-Bus services of `keenu-daemon`. Calls block, so a host runs them off
//! its UI thread. A capability a system lacks is reported with [`Availability`], never faked.

use std::path::{Path, PathBuf};

/// Whether something works here, and if not, the reason a person should read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// Turned off by the user's choice or a system setting; it could work.
    NeedsPermission(String),
    /// Needs credentials, an account or a configuration that is not there yet.
    NotConfigured(String),
    /// This system or build cannot do it.
    Unsupported(String),
}

impl Availability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    /// What to tell the person; `None` when it simply works.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Available => None,
            Self::NeedsPermission(why) | Self::NotConfigured(why) | Self::Unsupported(why) => {
                Some(why)
            }
        }
    }
}

/// A failure, in words meant for the person using Keenu.
pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Paused,
    Playing,
}

/// One application's media session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaSession {
    /// The system's identifier for the session's application.
    pub id: String,
    pub app_name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub state: PlaybackState,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub can_play_pause: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    /// Whether the session offers a cover; fetch it with [`MediaProvider::artwork`].
    pub has_artwork: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaCommand {
    PlayPause,
    Next,
    Previous,
    Seek(i64),
}

pub trait MediaProvider: Send + Sync {
    fn availability(&self) -> Availability;
    /// Every active session; the one the system considers current first.
    fn sessions(&self) -> Result<Vec<MediaSession>>;
    fn command(&self, session_id: &str, command: MediaCommand) -> Result<()>;
    /// Encoded image bytes of a session's cover, if it has one.
    fn artwork(&self, session_id: &str) -> Result<Option<Vec<u8>>>;
}

/// A notification ready for the system to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeNotification {
    pub tag: String,
    pub title: String,
    pub body: String,
    /// `(action id, label)`; a system that has no buttons shows the text alone.
    pub actions: Vec<(String, String)>,
}

pub trait NotificationProvider: Send + Sync {
    fn availability(&self) -> Availability;
    fn show(&self, notification: &NativeNotification) -> Result<()>;
}

/// What is on the clipboard right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardContent {
    Text(String),
    Rich {
        text: String,
        rtf: String,
    },
    /// PNG-encoded.
    Image {
        png: Vec<u8>,
        width: u32,
        height: u32,
    },
    Files(Vec<PathBuf>),
}

pub trait ClipboardProvider: Send + Sync {
    fn availability(&self) -> Availability;
    /// Reads it. Only ever called on a change while the user has history switched on.
    fn read(&self) -> Result<Option<ClipboardContent>>;
    fn write(&self, content: &ClipboardContent) -> Result<()>;
}

pub trait FileShelfProvider: Send + Sync {
    /// Opens the file with its default application.
    fn open(&self, path: &Path) -> Result<()>;
    /// Shows the file selected in the system's file manager.
    fn reveal(&self, path: &Path) -> Result<()>;
    /// A PNG thumbnail no larger than `size` pixels, if the system can make one.
    fn thumbnail(&self, path: &Path, size: u32) -> Result<Option<Vec<u8>>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// 16-bit mono PCM.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recording {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

pub trait VoiceProvider: Send + Sync {
    fn availability(&self) -> Availability;
    fn devices(&self) -> Result<Vec<AudioDevice>>;
    /// Starts recording from `device` (`""` for the default). Called only on the user's action.
    fn start(&self, device: &str) -> Result<()>;
    /// Stops and returns what was recorded.
    fn stop(&self) -> Result<Recording>;
    /// Stops and throws it away.
    fn cancel(&self) -> Result<()>;
    /// Whether a recording is under way, so the UI always tells the truth about the microphone.
    fn recording(&self) -> bool;
    /// Listens once and returns the words, on this computer, where the system can do that.
    fn listen_local(&self) -> Result<String>;
    /// Says `text` with the system's voice.
    fn speak_local(&self, text: &str) -> Result<()>;
    /// Stops what `speak_local` is saying, so the user can talk over it.
    fn stop_speaking(&self) {}
    /// Listens on this computer until one of `phrases` is heard, or `stop` is set (`None`). What was said after the
    /// phrase in the same breath comes back ("Keenu, open Spotify" gives "open Spotify"), and is empty when there was
    /// nothing. The microphone is open for as long as this runs, so it runs only while the user has asked for it.
    fn wait_for_wake(
        &self,
        _phrases: &[&str],
        _stop: &std::sync::atomic::AtomicBool,
    ) -> Result<Option<String>> {
        Err("Listening for a wake word is not available on this system yet.".into())
    }
    /// Whether `wait_for_wake` can work here.
    fn hears_phrases(&self) -> bool {
        false
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// Stays the same across restarts, for remembering the user's choice.
    pub name: String,
    pub is_primary: bool,
    /// In physical pixels; `x` and `y` may be negative.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Percent: 100 at normal scale.
    pub scale_percent: u32,
}

pub trait MonitorProvider: Send + Sync {
    fn monitors(&self) -> Result<Vec<Monitor>>;
}

pub trait UsageProvider: Send + Sync {
    fn availability(&self) -> Availability;
    /// The identifier of the application in front, and nothing else about it.
    fn foreground_app(&self) -> Option<String>;
    /// How long since the user last touched the keyboard or mouse, where the system says.
    fn idle_ms(&self) -> Option<u64> {
        None
    }
}

pub trait WindowLifecycleProvider: Send + Sync {
    fn set_start_at_login(&self, enabled: bool) -> Result<()>;
    fn start_at_login(&self) -> Result<bool>;
    /// Whether another application covers the monitor in fullscreen.
    fn fullscreen_active(&self) -> bool;
}

/// Puts a downloaded update in place; verified before this is ever called.
pub trait UpdateProvider: Send + Sync {
    fn availability(&self) -> Availability;
    /// The artifact kinds this installation can take, best first.
    fn installable_kinds(&self) -> Vec<&'static str>;
    /// Checks the platform's own signature on the file.
    fn verify_platform_signature(&self, file: &Path) -> Result<()>;
    /// Installs it and restarts Keenu.
    fn install(&self, file: &Path) -> Result<()>;
}

/// An open window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowInfo {
    /// What the system calls it, good until it closes.
    pub id: String,
    pub app: String,
    pub title: String,
    pub focused: bool,
}

/// One control in a window, as the system's accessibility interface reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiElement {
    pub role: String,
    pub name: String,
    /// What it holds; always empty for a password field.
    pub value: String,
    pub enabled: bool,
    /// A password field: Keenu neither reads it nor types into it.
    pub secret: bool,
}

/// Said when an action reaches a password field. Providers check this themselves, so it holds whatever the field is
/// called and whatever the AI asks.
pub const SECRET_FIELD: &str = "That is a password field, so Keenu leaves it to you. Type it yourself, then tell Keenu to go on.";

/// Said for a system command this system has no way to carry out yet.
pub const NOT_HERE: &str = "Keenu cannot do that on this system yet.";

/// Turning a level up, down, or to a value from 0 to 100.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adjust {
    Set(u8),
    Up,
    Down,
    Mute,
    Unmute,
}

/// Ending or pausing the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Power {
    Sleep,
    Restart,
    ShutDown,
    SignOut,
}

/// Something the assistant asks of the system itself rather than of one app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemCommand {
    /// A window, by its id or title or app (the one in front when empty): `focus`, `close`, `minimize`, `maximize`,
    /// `unmaximize`, or a part of the screen: `left`, `right`, `top`, `bottom`, `top_left`, `top_right`,
    /// `bottom_left`, `bottom_right`, `center`.
    Window {
        window: String,
        command: String,
    },
    /// Windows side by side over the screen: `halves`, `rows`, `quarters`, `thirds` or `grid`, of `apps` in order,
    /// or of the windows in front when it is empty.
    Arrange {
        layout: String,
        apps: Vec<String>,
    },
    Volume(Adjust),
    Brightness(Adjust),
    Wifi(bool),
    Bluetooth(bool),
    DarkMode(bool),
    /// The system's settings at a page: `wifi`, `bluetooth`, `display`, `sound`, `notifications`, `power`,
    /// `privacy`, `apps`, `update`, `network`, `personalization`, `mouse`, `keyboard`, `printers`, `system`.
    Settings(String),
    /// The whole screen, saved as a picture. Where, in words.
    Screenshot,
    Lock,
    Power(Power),
    /// Types text where the cursor is, in whatever app has it.
    Write(String),
    /// Puts text on the clipboard.
    Copy(String),
}

/// Something the assistant asks to know about the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    /// The programs using the most processor time and memory now.
    TopProcesses,
    /// Battery, memory, disk space, how long it has been on, and the system's version.
    SystemInfo,
    /// The text on the clipboard. Never what a password manager marked as private.
    Clipboard,
}

/// Using other apps, through the system's accessibility interface.
pub trait AppControlProvider: Send + Sync {
    fn availability(&self) -> Availability;
    fn windows(&self) -> Result<Vec<WindowInfo>>;
    /// The controls of the window whose id or title holds `window`; the one in front when it is empty.
    fn read_window(&self, window: &str, limit: usize) -> Result<Vec<UiElement>>;
    fn focus_window(&self, window: &str) -> Result<()>;
    /// Asks the window to close, as its close button does, so it can ask to save.
    fn close_window(&self, window: &str) -> Result<()>;
    /// Presses the control called `target`. What it did, in words.
    fn click(&self, window: &str, target: &str) -> Result<String>;
    /// Puts `text` in the field called `target`; the field with focus when `target` is empty.
    fn set_text(&self, window: &str, target: &str, text: &str) -> Result<String>;
    /// Presses keys such as `ctrl+s` or `enter` in the window.
    fn press_keys(&self, window: &str, keys: &str) -> Result<()>;
    /// Starts an installed app by name.
    fn open_app(&self, name: &str) -> Result<String>;
    /// Installed apps whose name holds one of `terms`.
    fn find_apps(&self, terms: &[String]) -> Result<Vec<String>>;
    /// Opens a file, folder or address with what the system uses for it.
    fn open_path(&self, target: &str) -> Result<()>;
    /// Carries out a system command. What it did, in words.
    fn system(&self, command: &SystemCommand) -> Result<String> {
        let _ = command;
        Err(NOT_HERE.into())
    }
    /// What the system says about itself, as text for the assistant.
    fn report(&self, report: Report) -> Result<String> {
        let _ = report;
        Err(NOT_HERE.into())
    }
}

/// A key combination as people write it: `ctrl+shift+s`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyCombo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The Windows key, or Command on a Mac.
    pub meta: bool,
    /// The key itself, in lower case: `s`, `enter`, `f5`.
    pub key: String,
}

impl KeyCombo {
    /// `None` when it names no key, or more than one.
    pub fn parse(text: &str) -> Option<Self> {
        let mut combo = Self {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            key: String::new(),
        };
        for part in text.split('+').map(|part| part.trim().to_lowercase()) {
            match part.as_str() {
                "ctrl" | "control" => combo.ctrl = true,
                "shift" => combo.shift = true,
                "alt" | "option" => combo.alt = true,
                "win" | "windows" | "cmd" | "command" | "meta" | "super" => combo.meta = true,
                "" => return None,
                key if combo.key.is_empty() => combo.key = key.to_owned(),
                _ => return None,
            }
        }
        (!combo.key.is_empty()).then_some(combo)
    }
}

/// What people call the assistant to wake it, longest first so "hey keenu" is not taken for "keenu".
pub const WAKE_PHRASES: &[&str] = &[
    "good afternoon keenu",
    "good morning keenu",
    "good evening keenu",
    "hello keenu",
    "okay keenu",
    "hey keenu",
    "hi keenu",
    "ok keenu",
    "keenu",
];

/// Lower case, with only letters, digits and single spaces.
fn plain_words(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// What was said after a wake phrase, when `heard` starts with one: "Keenu, open Spotify" gives "open Spotify", and
/// "Hey Keenu" gives "". `None` when it starts with none of them.
pub fn after_wake(heard: &str, phrases: &[&str]) -> Option<String> {
    let plain = plain_words(heard);
    let mut phrases: Vec<String> = phrases.iter().map(|p| plain_words(p)).collect();
    phrases.sort_by_key(|p| std::cmp::Reverse(p.len()));
    let phrase = phrases
        .iter()
        .find(|p| !p.is_empty() && (plain == **p || plain.starts_with(&format!("{p} "))))?;
    let skip = phrase.split(' ').count();
    let rest: Vec<&str> = heard.split_whitespace().skip(skip).collect();
    Some(
        rest.join(" ")
            .trim_start_matches(|c: char| !c.is_alphanumeric())
            .to_owned(),
    )
}

/// `text` holds `part`, in any case; an empty `part` holds for everything.
pub fn holds(text: &str, part: &str) -> bool {
    text.to_lowercase().contains(&part.trim().to_lowercase())
}

/// Which control is meant by `target`: the one named exactly, else the only one whose name holds it. Several that could
/// be meant is an error, so a request that is not clear is never guessed at.
pub fn pick_element<'a>(
    elements: &'a [UiElement],
    target: &str,
    what: &str,
) -> Result<&'a UiElement> {
    let target = target.trim();
    if target.is_empty() {
        return Err(format!("Say which {what}."));
    }
    let lower = target.to_lowercase();
    if let Some(exact) = elements.iter().find(|e| e.name.to_lowercase() == lower) {
        return Ok(exact);
    }
    let close: Vec<&UiElement> = elements.iter().filter(|e| holds(&e.name, target)).collect();
    match close.as_slice() {
        [] => Err(format!("No {what} called “{target}” is in that window.")),
        [only] => Ok(only),
        several => {
            let names: Vec<&str> = several.iter().take(6).map(|e| e.name.as_str()).collect();
            Err(format!(
                "More than one {what} could be “{target}”: {}. Say which.",
                names.join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_after_a_wake_phrase_is_kept() {
        assert_eq!(
            after_wake("Keenu, open Spotify", WAKE_PHRASES).as_deref(),
            Some("open Spotify")
        );
        assert_eq!(after_wake("Hey Keenu", WAKE_PHRASES).as_deref(), Some(""));
        assert_eq!(
            after_wake("good morning keenu what is on today", WAKE_PHRASES).as_deref(),
            Some("what is on today")
        );
        assert_eq!(
            after_wake("Hey, Keenu! Lock the screen", WAKE_PHRASES).as_deref(),
            Some("Lock the screen")
        );
        assert_eq!(after_wake("open Spotify", WAKE_PHRASES), None);
        assert_eq!(after_wake("Keenuish", WAKE_PHRASES), None);
    }

    fn element(name: &str) -> UiElement {
        UiElement {
            name: name.to_owned(),
            ..UiElement::default()
        }
    }

    #[test]
    fn reads_key_combinations() {
        let combo = KeyCombo::parse("Ctrl+Shift+S").unwrap();
        assert!(combo.ctrl && combo.shift && !combo.alt && !combo.meta);
        assert_eq!(combo.key, "s");
        assert_eq!(KeyCombo::parse("enter").unwrap().key, "enter");
        assert!(KeyCombo::parse("cmd+q").unwrap().meta);
        assert_eq!(KeyCombo::parse("ctrl+"), None);
        assert_eq!(KeyCombo::parse("ctrl"), None);
        assert_eq!(KeyCombo::parse("a+b"), None);
    }

    #[test]
    fn a_control_is_found_only_when_it_is_clear_which() {
        let elements = [element("Save"), element("Save as"), element("Open")];
        assert_eq!(
            pick_element(&elements, "save", "button").unwrap().name,
            "Save"
        );
        assert_eq!(
            pick_element(&elements, "ope", "button").unwrap().name,
            "Open"
        );
        let both = [element("Save as"), element("Save all")];
        assert!(
            pick_element(&both, "save", "button")
                .unwrap_err()
                .contains("More than one")
        );
        assert!(
            pick_element(&elements, "print", "button")
                .unwrap_err()
                .contains("No button")
        );
        assert!(pick_element(&elements, " ", "button").is_err());
    }
}
