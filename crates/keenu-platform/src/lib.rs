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
