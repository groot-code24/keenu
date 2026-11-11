// SPDX-License-Identifier: GPL-3.0-or-later

//! macOS behind Keenu's platform interfaces, using only public means. What macOS keeps private says so
//! through `availability` rather than pretending: there is no public API for every app's now-playing
//! information, so media covers the apps that can be scripted.

use std::path::{Path, PathBuf};
use std::process::Command;

use keenu_platform::{
    Adjust, AppControlProvider, AudioDevice, Availability, ClipboardContent, ClipboardProvider,
    FileShelfProvider, KeyCombo, MediaCommand, MediaProvider, MediaSession, Monitor,
    MonitorProvider, NOT_HERE, NativeNotification, NotificationProvider, PlaybackState,
    PointerAction, Power, Recording, Report, Result, SECRET_FIELD, ScreenPicture, SystemCommand,
    UiElement, UpdateProvider, UsageProvider, VoiceProvider, WindowInfo, WindowLifecycleProvider,
    holds, pick_element,
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

/// Runs JavaScript for Automation, which reaches CoreGraphics and AppKit. Scripts here are built from numbers only.
fn jxa(script: &str) -> Result<String> {
    let output = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", script])
        .output()
        .map_err(|err| format!("Could not run osascript: {err}"))?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(
            if why.contains("not allowed") || why.contains("1002") || why.contains("-1719") {
                "Keenu needs permission: System Settings > Privacy & Security > Accessibility."
                    .to_owned()
            } else {
                why
            },
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A rectangle on the screen in points, from the top left of the main display: x, y, width, height.
type Rect = (i64, i64, i64, i64);

/// The main display's area that windows may use (below the menu bar, beside the Dock), in points from its top left.
fn usable_area() -> Result<Rect> {
    let said = jxa(
        "ObjC.import('AppKit'); var s = $.NSScreen.mainScreen; var f = s.frame; var v = s.visibleFrame; \
         [v.origin.x, f.size.height - v.origin.y - v.size.height, v.size.width, v.size.height].join(' ')",
    )?;
    let numbers: Vec<i64> = said
        .split_whitespace()
        .filter_map(|n| n.parse::<f64>().ok())
        .map(|n| n.round() as i64)
        .collect();
    match numbers[..] {
        [x, y, w, h] if w > 0 && h > 0 => Ok((x, y, w, h)),
        _ => Err("macOS would not say how large the screen is.".into()),
    }
}

/// Where `part` of `area` is: `left` is its left half, `top_left` its top-left quarter, `maximize` all of it.
fn part_of(area: Rect, part: &str) -> Option<Rect> {
    let (x, y, w, h) = area;
    let (half_w, half_h) = (w / 2, h / 2);
    Some(match part {
        "maximize" => area,
        "left" => (x, y, half_w, h),
        "right" => (x + half_w, y, w - half_w, h),
        "top" => (x, y, w, half_h),
        "bottom" => (x, y + half_h, w, h - half_h),
        "top_left" => (x, y, half_w, half_h),
        "top_right" => (x + half_w, y, w - half_w, half_h),
        "bottom_left" => (x, y + half_h, half_w, h - half_h),
        "bottom_right" => (x + half_w, y + half_h, w - half_w, h - half_h),
        "center" | "unmaximize" => (x + w / 6, y + h / 6, w * 2 / 3, h * 2 / 3),
        _ => return None,
    })
}

/// `count` places side by side over `area` in `layout`: halves, rows, thirds, quarters or a grid.
fn tiles(area: Rect, layout: &str, count: usize) -> Vec<Rect> {
    let (x, y, w, h) = area;
    let (columns, rows) = match layout {
        "rows" => (1, count.clamp(1, 4)),
        "thirds" => (3, 1),
        "quarters" => (2, 2),
        "grid" => {
            let columns = (count as f64).sqrt().ceil().max(1.0) as usize;
            (columns, count.div_ceil(columns).max(1))
        }
        _ => (count.clamp(1, 4), 1),
    };
    let (columns, rows) = (columns as i64, rows as i64);
    (0..columns * rows)
        .take(count)
        .map(|index| {
            let (column, row) = (index % columns, index / columns);
            let left = x + w * column / columns;
            let top = y + h * row / rows;
            (
                left,
                top,
                x + w * (column + 1) / columns - left,
                y + h * (row + 1) / rows - top,
            )
        })
        .collect()
}

/// Puts the front window of the app behind `window` at `place`.
fn place(window: &str, place: Rect) -> Result<String> {
    let clause = app_clause(window)?;
    let (x, y, w, h) = place;
    system_events(&format!(
        "tell {clause}\nset value of attribute \"AXMinimized\" of window 1 to false\nset position of window 1 to {{{x}, {y}}}\nset size of window 1 to {{{w}, {h}}}\nend tell"
    ))?;
    Ok(format!("Moved {window}."))
}

/// The mouse at a place on the screen, in points, through CoreGraphics' events.
fn pointer(x: i32, y: i32, action: PointerAction) -> Result<String> {
    // Event types: 1/2 left down/up, 3/4 right down/up, 5 moved, 6 left dragged. Field 1 is the click count.
    let body = match action {
        PointerAction::Move => format!("post(5, {x}, {y}, 0, 0);"),
        PointerAction::Click => {
            format!("post(5, {x}, {y}, 0, 0); post(1, {x}, {y}, 0, 1); post(2, {x}, {y}, 0, 1);")
        }
        PointerAction::DoubleClick => format!(
            "post(5, {x}, {y}, 0, 0); post(1, {x}, {y}, 0, 1); post(2, {x}, {y}, 0, 1); post(1, {x}, {y}, 0, 2); post(2, {x}, {y}, 0, 2);"
        ),
        PointerAction::RightClick => {
            format!("post(5, {x}, {y}, 0, 0); post(3, {x}, {y}, 1, 1); post(4, {x}, {y}, 1, 1);")
        }
        PointerAction::Scroll(notches) => format!(
            "post(5, {x}, {y}, 0, 0); $.CGEventPost(0, $.CGEventCreateScrollWheelEvent2(null, 1, 1, {notches}, 0, 0));"
        ),
        PointerAction::DragTo(to_x, to_y) => format!(
            "post(5, {x}, {y}, 0, 0); post(1, {x}, {y}, 0, 1); for (var i = 1; i <= 10; i++) {{ post(6, {x} + ({to_x} - {x}) * i / 10, {y} + ({to_y} - {y}) * i / 10, 0, 0); }} post(2, {to_x}, {to_y}, 0, 1);"
        ),
    };
    jxa(&format!(
        "ObjC.import('CoreGraphics'); function post(t, x, y, b, clicks) {{ var e = $.CGEventCreateMouseEvent(null, t, {{x: x, y: y}}, b); if (clicks) {{ $.CGEventSetIntegerValueField(e, 1, clicks); }} $.CGEventPost(0, e); delay(0.03); }} {body}"
    ))?;
    Ok(match action {
        PointerAction::Move => format!("The pointer is at {x}, {y}."),
        PointerAction::Click => format!("Clicked at {x}, {y}."),
        PointerAction::DoubleClick => format!("Double-clicked at {x}, {y}."),
        PointerAction::RightClick => format!("Right-clicked at {x}, {y}."),
        PointerAction::Scroll(_) => format!("Scrolled at {x}, {y}."),
        PointerAction::DragTo(to_x, to_y) => format!("Dragged from {x}, {y} to {to_x}, {to_y}."),
    })
}

/// A PNG's size, from its header.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    Some((
        u32::from_be_bytes(png[16..20].try_into().ok()?),
        u32::from_be_bytes(png[20..24].try_into().ok()?),
    ))
}

/// A picture of the main display, made smaller with `sips`; the file goes as soon as it is read.
fn capture() -> Result<ScreenPicture> {
    let file = std::env::temp_dir().join(format!("keenu-screen-{}.png", std::process::id()));
    let path = file.to_string_lossy().into_owned();
    let made =
        output("/usr/sbin/screencapture", &["-x", "-m", "-t", "png", &path]).and_then(|_| {
            output(
                "/usr/bin/sips",
                &["-Z", &keenu_platform::PICTURE_WIDTH.to_string(), &path],
            )
        });
    let png = made.and_then(|_| std::fs::read(&file).map_err(|err| err.to_string()));
    let _ = std::fs::remove_file(&file);
    let png = png.map_err(|why| {
        format!("macOS would not picture the screen ({why}). Keenu needs Screen Recording: System Settings > Privacy & Security.")
    })?;
    let (width, height) = png_size(&png).ok_or("The picture of the screen was not a PNG.")?;
    let bounds = jxa(
        "ObjC.import('CoreGraphics'); var b = $.CGDisplayBounds($.CGMainDisplayID()); b.size.width + ' ' + b.size.height",
    )?;
    let size: Vec<i32> = bounds
        .split_whitespace()
        .filter_map(|n| n.parse::<f64>().ok())
        .map(|n| n.round() as i32)
        .collect();
    let [wide, high] = size[..] else {
        return Err("macOS would not say how large the screen is.".into());
    };
    Ok(ScreenPicture {
        png,
        width,
        height,
        screen: (0, 0, wide, high),
    })
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
        let child = Command::new("/usr/bin/say")
            .arg("--")
            .arg(text)
            .spawn()
            .map_err(|err| format!("Could not run say: {err}"))?;
        let id = child.id();
        SPEAKING.store(id, std::sync::atomic::Ordering::Relaxed);
        let mut child = child;
        let status = child
            .wait()
            .map_err(|err| format!("Could not run say: {err}"));
        SPEAKING.store(0, std::sync::atomic::Ordering::Relaxed);
        // Stopped by the user, it ends on a signal: nothing went wrong.
        match status? {
            status if status.success() || status.code().is_none() => Ok(()),
            _ => Err("macOS could not speak.".into()),
        }
    }

    fn stop_speaking(&self) {
        let id = SPEAKING.swap(0, std::sync::atomic::Ordering::Relaxed);
        if id != 0 {
            let _ = Command::new("/bin/kill").arg(id.to_string()).status();
        }
    }
}

/// The `say` under way, so it can be stopped; 0 for none.
static SPEAKING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

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

/// Using other apps, through System Events: macOS's accessibility interface as it is open to scripts. It needs the user's
/// permission in System Settings > Privacy & Security > Accessibility, which macOS asks for the first time.
pub struct AppControl;

/// The window a script should work on, in AppleScript: by the app it is in, or what is in front.
fn app_clause(window: &str) -> Result<String> {
    let window = window.trim();
    if window.is_empty() {
        return Ok("first application process whose frontmost is true".to_owned());
    }
    let found = AppControl.windows()?;
    let apps: Vec<&WindowInfo> = found
        .iter()
        .filter(|w| holds(&w.title, window) || holds(&w.app, window))
        .collect();
    match apps.first() {
        Some(info) => Ok(format!("application process {}", quoted(&info.app))),
        None => Err(format!("No window called “{window}” is open.")),
    }
}

/// `tell System Events`, explaining the one failure people can fix.
fn system_events(script: &str) -> Result<String> {
    osascript(&format!(
        "tell application \"System Events\"\n{script}\nend tell"
    ))
    .map_err(|why| {
        if why.contains("not allowed") || why.contains("1002") || why.contains("-1719") {
            "Keenu needs permission: System Settings > Privacy & Security > Accessibility."
                .to_owned()
        } else {
            why
        }
    })
}

/// Fields between windows, lines and controls in what the scripts below print.
const FIELD: &str = "\u{1f}";
const RECORD: &str = "\u{1e}";

/// The controls of a window as records. A secure field's value is never asked for: AppleScript gets its role first.
fn read_controls(window: &str, limit: usize) -> Result<Vec<UiElement>> {
    let clause = app_clause(window)?;
    let script = format!(
        r#"set out to ""
set fieldMark to (ASCII character 31)
set recordMark to (ASCII character 30)
tell {clause}
  set found to entire contents of front window
  set counted to 0
  repeat with item_ in found
    if counted is greater than or equal to {limit} then exit repeat
    try
      set r to role of item_
      set n to ""
      try
        set n to name of item_ as text
      end try
      if n is "missing value" or n is "" then
        try
          set n to description of item_ as text
        end try
      end if
      set v to ""
      if r is not "AXSecureTextField" then
        try
          set v to value of item_ as text
        end try
      end if
      set e to "true"
      try
        set e to enabled of item_ as text
      end try
      set out to out & r & fieldMark & n & fieldMark & v & fieldMark & e & recordMark
      set counted to counted + 1
    end try
  end repeat
end tell
return out"#
    );
    let text = system_events(&script)?;
    Ok(parse_controls(&text))
}

fn parse_controls(text: &str) -> Vec<UiElement> {
    text.split(RECORD)
        .filter_map(|record| {
            let mut parts = record.split(FIELD);
            let role = parts.next()?.trim();
            if role.is_empty() {
                return None;
            }
            let name = parts.next().unwrap_or_default();
            let value = parts.next().unwrap_or_default();
            let enabled = parts.next().unwrap_or("true").trim() != "false";
            let secret = role == "AXSecureTextField";
            let shown = |text: &str| {
                if text == "missing value" {
                    String::new()
                } else {
                    text.to_owned()
                }
            };
            let name = shown(name);
            let value = if secret { String::new() } else { shown(value) };
            if name.is_empty()
                && value.is_empty()
                && !matches!(role, "AXTextField" | "AXTextArea" | "AXSecureTextField")
            {
                return None;
            }
            Some(UiElement {
                role: role.trim_start_matches("AX").to_lowercase(),
                name,
                value,
                enabled,
                secret,
            })
        })
        .collect()
}

/// AppleScript's key code for a named key.
fn key_code(key: &str) -> Option<u32> {
    Some(match key {
        "enter" | "return" => 36,
        "tab" => 48,
        "space" => 49,
        "backspace" | "delete" => 51,
        "esc" | "escape" => 53,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        "home" => 115,
        "end" => 119,
        "pageup" => 116,
        "pagedown" => 121,
        _ => return None,
    })
}

impl AppControlProvider for AppControl {
    fn availability(&self) -> Availability {
        match system_events("return name of first application process whose frontmost is true") {
            Ok(_) => Availability::Available,
            Err(why) => Availability::NeedsPermission(why),
        }
    }

    fn windows(&self) -> Result<Vec<WindowInfo>> {
        let script = r#"set out to ""
set fieldMark to (ASCII character 31)
set recordMark to (ASCII character 30)
repeat with p in (application processes whose background only is false)
  set appName to name of p
  set isFront to frontmost of p as text
  repeat with w in windows of p
    set out to out & appName & fieldMark & (name of w as text) & fieldMark & isFront & recordMark
  end repeat
end repeat
return out"#;
        let text = system_events(script)?;
        Ok(text
            .split(RECORD)
            .filter_map(|record| {
                let mut parts = record.split(FIELD);
                let app = parts.next()?.trim().to_owned();
                if app.is_empty() {
                    return None;
                }
                let title = parts.next().unwrap_or_default().to_owned();
                let focused = parts.next().unwrap_or_default().trim() == "true";
                Some(WindowInfo {
                    id: format!("{app}/{title}"),
                    app,
                    title,
                    focused,
                })
            })
            .collect())
    }

    fn read_window(&self, window: &str, limit: usize) -> Result<Vec<UiElement>> {
        read_controls(window, limit)
    }

    fn focus_window(&self, window: &str) -> Result<()> {
        let clause = app_clause(window)?;
        system_events(&format!("set frontmost of {clause} to true")).map(drop)
    }

    fn close_window(&self, window: &str) -> Result<()> {
        let clause = app_clause(window)?;
        system_events(&format!("tell {clause} to click (first button of front window whose subrole is \"AXCloseButton\")")).map(drop)
    }

    fn click(&self, window: &str, target: &str) -> Result<String> {
        let elements = read_controls(window, 2000)?;
        let control = pick_element(&elements, target, "control")?;
        if control.secret {
            return Err(SECRET_FIELD.into());
        }
        if !control.enabled {
            return Err(format!("“{}” is greyed out right now.", control.name));
        }
        let clause = app_clause(window)?;
        system_events(&format!(
            "tell {clause}\nrepeat with item_ in (entire contents of front window)\ntry\nif name of item_ is {} or description of item_ is {} then\nclick item_\nexit repeat\nend if\nend try\nend repeat\nend tell",
            quoted(&control.name),
            quoted(&control.name)
        ))?;
        Ok(format!("Pressed “{}”.", control.name))
    }

    fn set_text(&self, window: &str, target: &str, text: &str) -> Result<String> {
        let elements = read_controls(window, 2000)?;
        let fields: Vec<UiElement> = elements
            .into_iter()
            .filter(|e| {
                e.secret || matches!(e.role.as_str(), "textfield" | "textarea" | "combobox")
            })
            .collect();
        let control = if target.trim().is_empty() {
            match fields.as_slice() {
                [only] => only,
                [] => return Err("That window has no field to type in.".into()),
                _ => return Err("That window has several fields: say which.".into()),
            }
        } else {
            pick_element(&fields, target, "field")?
        };
        if control.secret {
            return Err(SECRET_FIELD.into());
        }
        let clause = app_clause(window)?;
        system_events(&format!(
            "tell {clause}\nrepeat with item_ in (entire contents of front window)\ntry\nif role of item_ is not \"AXSecureTextField\" and (name of item_ is {name} or description of item_ is {name}) then\nset value of item_ to {value}\nexit repeat\nend if\nend try\nend repeat\nend tell",
            name = quoted(&control.name),
            value = quoted(text)
        ))?;
        Ok(format!("Typed into “{}”.", control.name))
    }

    fn press_keys(&self, window: &str, keys: &str) -> Result<()> {
        let combo =
            KeyCombo::parse(keys).ok_or_else(|| format!("“{keys}” is not a key combination."))?;
        if !window.trim().is_empty() {
            self.focus_window(window)?;
        }
        let mut held = Vec::new();
        for (on, name) in [
            (combo.ctrl, "control down"),
            (combo.shift, "shift down"),
            (combo.alt, "option down"),
            (combo.meta, "command down"),
        ] {
            if on {
                held.push(name);
            }
        }
        let using = if held.is_empty() {
            String::new()
        } else {
            format!(" using {{{}}}", held.join(", "))
        };
        let press = match key_code(&combo.key) {
            Some(code) => format!("key code {code}{using}"),
            None if combo.key.chars().count() == 1 => {
                format!("keystroke {}{using}", quoted(&combo.key))
            }
            None => return Err(format!("“{}” is not a key.", combo.key)),
        };
        system_events(&press).map(drop)
    }

    fn open_app(&self, name: &str) -> Result<String> {
        if name.trim().is_empty() {
            return Err("Say which app.".into());
        }
        let status = Command::new("/usr/bin/open")
            .args(["-a", name])
            .status()
            .map_err(|err| format!("Could not run open: {err}"))?;
        if status.success() {
            Ok(format!("Opened {name}."))
        } else {
            Err(format!("No app called “{name}” was found."))
        }
    }

    fn find_apps(&self, terms: &[String]) -> Result<Vec<String>> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let mut names = Vec::new();
        for folder in [
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
            home.join("Applications"),
        ] {
            let Ok(entries) = std::fs::read_dir(folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(app) = name.strip_suffix(".app")
                    && (terms.is_empty() || terms.iter().any(|term| holds(app, term)))
                {
                    names.push(app.to_owned());
                }
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn open_path(&self, target: &str) -> Result<()> {
        // `open` would read it as one of its options.
        if target.starts_with('-') {
            return Err("macOS could not open that.".into());
        }
        let status = Command::new("/usr/bin/open")
            .arg(target)
            .status()
            .map_err(|err| format!("Could not run open: {err}"))?;
        if status.success() {
            Ok(())
        } else {
            Err("macOS could not open that.".into())
        }
    }

    fn system(&self, command: &SystemCommand) -> Result<String> {
        match command {
            SystemCommand::Volume(adjust) => {
                let script = match adjust {
                    Adjust::Set(level) => {
                        format!("set volume output volume {level} without output muted")
                    }
                    Adjust::Up => "set volume output volume ((output volume of (get volume settings)) + 10) without output muted".into(),
                    Adjust::Down => "set volume output volume ((output volume of (get volume settings)) - 10)".into(),
                    Adjust::Mute => "set volume with output muted".into(),
                    Adjust::Unmute => "set volume without output muted".into(),
                };
                osascript(&script).map(|_| "Done.".to_owned())
            }
            SystemCommand::DarkMode(on) => system_events(&format!(
                "tell appearance preferences to set dark mode to {on}"
            ))
            .map(|_| {
                if *on {
                    "Dark mode on."
                } else {
                    "Dark mode off."
                }
                .to_owned()
            }),
            SystemCommand::Wifi(on) => {
                let ports = output("/usr/sbin/networksetup", &["-listallhardwareports"])?;
                let device = wifi_device(&ports).ok_or("This Mac has no Wi-Fi.")?;
                output(
                    "/usr/sbin/networksetup",
                    &["-setairportpower", &device, if *on { "on" } else { "off" }],
                )
                .map(|_| if *on { "Wi-Fi on." } else { "Wi-Fi off." }.to_owned())
            }
            SystemCommand::Settings(panel) => {
                let page = settings_page(panel);
                self.open_path(&format!("x-apple.systempreferences:{page}"))
                    .map(|()| "Opened System Settings.".to_owned())
            }
            SystemCommand::Screenshot => {
                let file = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join("Desktop")
                    .join(format!(
                        "Keenu {}.png",
                        chrono::Local::now().format("%Y-%m-%d at %H.%M.%S")
                    ));
                output("/usr/sbin/screencapture", &["-x", &file.to_string_lossy()])
                    .map(|_| format!("Saved a picture of the screen as {}.", file.display()))
            }
            // The Lock Screen shortcut, as macOS has no command for it.
            SystemCommand::Lock => {
                system_events("keystroke \"q\" using {control down, command down}")
                    .map(|_| "Locked.".to_owned())
            }
            SystemCommand::Power(Power::Sleep) => {
                output("/usr/bin/pmset", &["sleepnow"]).map(|_| "Sleeping.".to_owned())
            }
            SystemCommand::Power(what) => {
                let verb = match what {
                    Power::Restart => "restart",
                    Power::ShutDown => "shut down",
                    _ => "log out",
                };
                system_events(verb).map(|_| format!("Asked macOS to {verb}."))
            }
            SystemCommand::Write(text) => system_events(&format!("keystroke {}", quoted(text)))
                .map(|_| "Typed it.".to_owned()),
            SystemCommand::Copy(text) => Clipboard
                .write(&ClipboardContent::Text(text.clone()))
                .map(|()| "Copied.".to_owned()),
            SystemCommand::Window { window, command } if command == "focus" => {
                self.focus_window(window).map(|()| "In front.".to_owned())
            }
            SystemCommand::Window { window, command } if command == "close" => self
                .close_window(window)
                .map(|()| "Asked it to close.".to_owned()),
            SystemCommand::Window { window, command } if command == "minimize" => {
                let clause = app_clause(window)?;
                system_events(&format!(
                    "tell {clause} to set value of attribute \"AXMinimized\" of window 1 to true"
                ))
                .map(|_| "Minimized.".to_owned())
            }
            SystemCommand::Window { window, command } => {
                let area = part_of(usable_area()?, command).ok_or(NOT_HERE)?;
                place(window, area)
            }
            SystemCommand::Arrange { layout, apps } => {
                let names: Vec<String> = if apps.is_empty() {
                    self.windows()?
                        .into_iter()
                        .map(|w| w.app)
                        .filter(|app| !app.to_lowercase().contains("keenu"))
                        .fold(Vec::new(), |mut seen, app| {
                            if !seen.contains(&app) {
                                seen.push(app);
                            }
                            seen
                        })
                        .into_iter()
                        .take(4)
                        .collect()
                } else {
                    apps.clone()
                };
                if names.is_empty() {
                    return Err("There are no windows to arrange.".into());
                }
                let places = tiles(usable_area()?, layout, names.len());
                let mut moved = Vec::new();
                for (name, area) in names.iter().zip(places) {
                    match place(name, area) {
                        Ok(_) => moved.push(name.clone()),
                        Err(why) => {
                            return Err(format!("Arranged {} of them; {name}: {why}", moved.len()));
                        }
                    }
                }
                Ok(format!("Arranged {}.", moved.join(", ")))
            }
            SystemCommand::Pointer { x, y, action } => pointer(*x, *y, *action),
            // No public way to set them from another program: brightness and Bluetooth.
            _ => Err(NOT_HERE.into()),
        }
    }

    fn capture_screen(&self) -> Result<ScreenPicture> {
        capture()
    }

    fn report(&self, report: Report) -> Result<String> {
        match report {
            Report::TopProcesses => {
                let by_processor = output("/bin/ps", &["-Aceo", "pcpu,pmem,comm", "-r"])?;
                let by_memory = output("/bin/ps", &["-Aceo", "pcpu,pmem,comm", "-m"])?;
                let first = |text: &str| text.lines().take(9).collect::<Vec<_>>().join("\n");
                Ok(format!(
                    "Using the processor most:\n{}\nUsing the most memory:\n{}",
                    first(&by_processor),
                    first(&by_memory)
                ))
            }
            Report::SystemInfo => {
                let mut said = Vec::new();
                for (program, args) in [
                    ("/usr/bin/sw_vers", &[][..]),
                    ("/usr/bin/uptime", &[][..]),
                    ("/usr/bin/pmset", &["-g", "batt"][..]),
                    ("/bin/df", &["-h", "/"][..]),
                ] {
                    if let Ok(text) = output(program, args) {
                        said.push(text);
                    }
                }
                Ok(said.join("\n"))
            }
            Report::Clipboard => Ok(match Clipboard.read()? {
                Some(ClipboardContent::Text(text) | ClipboardContent::Rich { text, .. }) => text,
                Some(_) => "Something that is not text.".into(),
                None => "Nothing.".into(),
            }),
        }
    }
}

/// What `program` printed, run with `args`.
fn output(program: &str, args: &[&str]) -> Result<String> {
    let done = Command::new(program)
        .args(args)
        .output()
        .map_err(|err| format!("Could not run {program}: {err}"))?;
    if done.status.success() {
        Ok(String::from_utf8_lossy(&done.stdout).trim().to_owned())
    } else {
        Err(String::from_utf8_lossy(&done.stderr).trim().to_owned())
    }
}

/// The Wi-Fi device in `networksetup -listallhardwareports`: the "Device:" line after "Hardware Port: Wi-Fi".
fn wifi_device(ports: &str) -> Option<String> {
    let mut lines = ports.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "Hardware Port: Wi-Fi" {
            return lines
                .next()?
                .trim()
                .strip_prefix("Device: ")
                .map(str::to_owned);
        }
    }
    None
}

/// The System Settings page for `panel`, as macOS 13 and later name them.
fn settings_page(panel: &str) -> &'static str {
    match panel {
        "wifi" => "com.apple.wifi-settings-extension",
        "bluetooth" => "com.apple.BluetoothSettings",
        "display" => "com.apple.Displays-Settings.extension",
        "sound" => "com.apple.Sound-Settings.extension",
        "notifications" => "com.apple.Notifications-Settings.extension",
        "power" | "battery" => "com.apple.Battery-Settings.extension",
        "privacy" => "com.apple.settings.PrivacySecurity.extension",
        "network" => "com.apple.Network-Settings.extension",
        "keyboard" => "com.apple.Keyboard-Settings.extension",
        "update" => "com.apple.Software-Update-Settings.extension",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = (0, 25, 1440, 875);

    #[test]
    fn a_part_of_the_screen_is_where_people_mean() {
        assert_eq!(part_of(SCREEN, "left"), Some((0, 25, 720, 875)));
        assert_eq!(part_of(SCREEN, "bottom_right"), Some((720, 462, 720, 438)));
        assert_eq!(part_of(SCREEN, "maximize"), Some(SCREEN));
        assert_eq!(part_of(SCREEN, "upside"), None);
    }

    #[test]
    fn tiles_cover_the_screen_without_gaps() {
        let quarters = tiles(SCREEN, "quarters", 4);
        assert_eq!(quarters.len(), 4);
        assert_eq!(quarters[3], (720, 462, 720, 438));
        let halves = tiles(SCREEN, "halves", 2);
        assert_eq!(halves[0].2 + halves[1].2, 1440);
        let grid = tiles(SCREEN, "grid", 5);
        assert_eq!(grid.len(), 5);
        assert_eq!(grid[0].2, 480);
    }

    #[test]
    fn a_png_says_its_size() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&1600u32.to_be_bytes());
        png.extend_from_slice(&1000u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((1600, 1000)));
        assert_eq!(png_size(b"GIF89a"), None);
    }
}
