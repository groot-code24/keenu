// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS behind Keenu's platform interfaces, using only public means. What macOS keeps private says so
//! through `availability` rather than pretending: there is no public API for every app's now-playing
//! information, so media covers the apps that can be scripted.

use std::path::{Path, PathBuf};
use std::process::Command;

use keenu_platform::{
    AudioDevice, Availability, ClipboardContent, ClipboardProvider, FileShelfProvider,
    MediaCommand, MediaProvider, MediaSession, Monitor, MonitorProvider, NativeNotification,
    NotificationProvider, PlaybackState, Recording, Result, UpdateProvider, UsageProvider,
    VoiceProvider, WindowLifecycleProvider,
};
use objc2::rc::Retained;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSScreen, NSWorkspace};
use objc2_foundation::{MainThreadMarker, NSString};

/// Runs AppleScript. Scripts here are constants, or quote what they are given with [`quoted`].
fn osascript(script: &str) -> Result<String> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-e", script])
        .output()
        .map_err(|err| format!("Could not run osascript: {err}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// `text` as an AppleScript string.
fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The apps with a scripting interface for what is playing.
const SCRIPTABLE: [&str; 2] = ["Music", "Spotify"];

pub struct Media;

fn running(app: &str) -> bool {
    osascript(&format!("application {} is running", quoted(app)))
        .is_ok_and(|answer| answer == "true")
}

impl MediaProvider for Media {
    fn availability(&self) -> Availability {
        Availability::Unsupported(
            "macOS only lets Keenu see Music and Spotify: it has no public way to read every app's playback.".into(),
        )
    }

    fn sessions(&self) -> Result<Vec<MediaSession>> {
        let mut sessions = Vec::new();
        for app in SCRIPTABLE {
            if !running(app) {
                continue;
            }
            let script = format!(
                "tell application {} to get {{name of current track, artist of current track, album of current track, player state as text, player position, duration of current track}}",
                quoted(app)
            );
            // A running app with nothing loaded has no current track to ask about.
            let Ok(answer) = osascript(&script) else {
                continue;
            };
            let parts: Vec<&str> = answer.split(", ").collect();
            if parts.len() < 6 {
                continue;
            }
            let position_ms =
                (parts[parts.len() - 2].parse::<f64>().unwrap_or(0.0) * 1000.0) as i64;
            let raw_duration = parts[parts.len() - 1].parse::<f64>().unwrap_or(0.0);
            // Spotify reports milliseconds, Music seconds.
            let duration_ms = if app == "Spotify" {
                raw_duration as i64
            } else {
                (raw_duration * 1000.0) as i64
            };
            sessions.push(MediaSession {
                id: app.to_owned(),
                app_name: app.to_owned(),
                title: parts[0].to_owned(),
                artist: parts[1].to_owned(),
                album: parts[2].to_owned(),
                state: match parts[parts.len() - 3] {
                    "playing" => PlaybackState::Playing,
                    "paused" => PlaybackState::Paused,
                    _ => PlaybackState::Stopped,
                },
                position_ms,
                duration_ms,
                can_play_pause: true,
                can_next: true,
                can_previous: true,
                can_seek: true,
                has_artwork: false,
            });
        }
        Ok(sessions)
    }

    fn command(&self, session_id: &str, command: MediaCommand) -> Result<()> {
        if !SCRIPTABLE.contains(&session_id) {
            return Err("That app cannot be controlled.".into());
        }
        let verb = match command {
            MediaCommand::PlayPause => "playpause".to_owned(),
            MediaCommand::Next => "next track".to_owned(),
            MediaCommand::Previous => "previous track".to_owned(),
            MediaCommand::Seek(ms) => format!("set player position to {}", ms as f64 / 1000.0),
        };
        osascript(&format!(
            "tell application {} to {verb}",
            quoted(session_id)
        ))
        .map(drop)
    }

    fn artwork(&self, _session_id: &str) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

pub struct Notifier;

impl NotificationProvider for Notifier {
    fn availability(&self) -> Availability {
        Availability::Available
    }

    /// Through Script Editor's identity, with no buttons: a signed bundle is needed for UserNotifications,
    /// which is not implemented yet.
    fn show(&self, notification: &NativeNotification) -> Result<()> {
        osascript(&format!(
            "display notification {} with title {}",
            quoted(&notification.body),
            quoted(&notification.title)
        ))
        .map(drop)
    }
}

pub struct Clipboard;

impl ClipboardProvider for Clipboard {
    fn availability(&self) -> Availability {
        Availability::Unsupported(
            "macOS has no notice of clipboard changes for an app to act on; history is not implemented here yet.".into(),
        )
    }

    fn read(&self) -> Result<Option<ClipboardContent>> {
        // SAFETY: the general pasteboard is always there; the type name is a constant from AppKit.
        unsafe {
            let board = NSPasteboard::generalPasteboard();
            let text: Option<Retained<NSString>> = board.stringForType(NSPasteboardTypeString);
            Ok(text.map(|text| ClipboardContent::Text(text.to_string())))
        }
    }

    fn write(&self, content: &ClipboardContent) -> Result<()> {
        let ClipboardContent::Text(text) = content else {
            return Err("Only text can go back on the clipboard here so far.".into());
        };
        // SAFETY: as above.
        unsafe {
            let board = NSPasteboard::generalPasteboard();
            board.clearContents();
            board.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
        }
        Ok(())
    }
}

pub struct Shell;

impl FileShelfProvider for Shell {
    fn open(&self, path: &Path) -> Result<()> {
        run_open(&[path.as_os_str().to_string_lossy().as_ref()])
    }

    fn reveal(&self, path: &Path) -> Result<()> {
        run_open(&["-R", path.as_os_str().to_string_lossy().as_ref()])
    }

    fn thumbnail(&self, _path: &Path, _size: u32) -> Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

fn run_open(args: &[&str]) -> Result<()> {
    let status = Command::new("/usr/bin/open")
        .args(args)
        .status()
        .map_err(|err| format!("Could not run open: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("macOS could not open that.".into())
    }
}

pub fn open_url(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        return Err("Only secure addresses are opened.".into());
    }
    run_open(&[url])
}

/// Plays through `afplay`, from a temporary file only this user can read.
pub fn play_wav(wav: &[u8]) -> Result<()> {
    let path: PathBuf =
        std::env::temp_dir().join(format!("keenu-speech-{}.wav", std::process::id()));
    keenu_core::json_file::write_private(&path, wav).map_err(|err| err.to_string())?;
    let status = Command::new("/usr/bin/afplay").arg(&path).status();
    let _ = std::fs::remove_file(&path);
    match status {
        Ok(status) if status.success() => Ok(()),
        _ => Err("macOS could not play the audio.".into()),
    }
}

pub struct Voice;

impl VoiceProvider for Voice {
    fn availability(&self) -> Availability {
        Availability::Unsupported(
            "Recording from the microphone is not implemented on macOS yet.".into(),
        )
    }
    fn devices(&self) -> Result<Vec<AudioDevice>> {
        Ok(Vec::new())
    }
    fn start(&self, _device: &str) -> Result<()> {
        Err("Recording is not implemented on macOS yet.".into())
    }
    fn stop(&self) -> Result<Recording> {
        Err("Not recording.".into())
    }
    fn cancel(&self) -> Result<()> {
        Ok(())
    }
    fn recording(&self) -> bool {
        false
    }
    fn listen_local(&self) -> Result<String> {
        Err("Speech recognition is not implemented on macOS yet.".into())
    }
    fn speak_local(&self, text: &str) -> Result<()> {
        let status = Command::new("/usr/bin/say")
            .arg("--")
            .arg(text)
            .status()
            .map_err(|err| format!("Could not run say: {err}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("macOS could not speak.".into())
        }
    }
}

pub struct Displays;

impl MonitorProvider for Displays {
    fn monitors(&self) -> Result<Vec<Monitor>> {
        let main =
            MainThreadMarker::new().ok_or("Displays can only be listed from the main thread.")?;
        let screens = NSScreen::screens(main);
        let mut list = Vec::new();
        for (index, screen) in screens.iter().enumerate() {
            let frame = screen.frame();
            list.push(Monitor {
                name: format!("Display {}", index + 1),
                is_primary: index == 0,
                x: frame.origin.x as i32,
                y: frame.origin.y as i32,
                width: frame.size.width as i32,
                height: frame.size.height as i32,
                scale_percent: (screen.backingScaleFactor() * 100.0) as u32,
            });
        }
        Ok(list)
    }
}

pub struct Foreground;

impl UsageProvider for Foreground {
    fn availability(&self) -> Availability {
        Availability::Available
    }

    /// The bundle identifier of the app in front, and nothing else.
    fn foreground_app(&self) -> Option<String> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication()?;
        app.bundleIdentifier().map(|id| id.to_string())
    }
}

pub struct Lifecycle;

impl WindowLifecycleProvider for Lifecycle {
    fn set_start_at_login(&self, _enabled: bool) -> Result<()> {
        Err("Starting at login needs a signed app bundle, and is not implemented yet.".into())
    }
    fn start_at_login(&self) -> Result<bool> {
        Ok(false)
    }
    fn fullscreen_active(&self) -> bool {
        false
    }
}

pub struct Updates;

impl UpdateProvider for Updates {
    fn availability(&self) -> Availability {
        Availability::NotConfigured(
            "Updating in place needs Developer ID signing and notarization, which this build does not have.".into(),
        )
    }
    fn installable_kinds(&self) -> Vec<&'static str> {
        Vec::new()
    }
    fn verify_platform_signature(&self, _file: &Path) -> Result<()> {
        Err("Signature checks are not implemented on macOS yet.".into())
    }
    fn install(&self, _file: &Path) -> Result<()> {
        Err("Updating in place is not implemented on macOS yet.".into())
    }
}
