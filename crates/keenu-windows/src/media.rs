// SPDX-License-Identifier: GPL-3.0-or-later

//! Media sessions through Windows' Global System Media Transport Controls: every app that shows in the
//! volume flyout, Spotify and browsers among them.

use keenu_platform::{
    Availability, MediaCommand, MediaProvider, MediaSession, PlaybackState, Result,
};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime;

/// Largest cover read; a larger one is ignored rather than filling memory.
const MAX_ARTWORK: u32 = 16 * 1024 * 1024;
/// Windows times are in units of 100 ns.
const TICKS_PER_MS: i64 = 10_000;

pub struct Media;

fn manager() -> Result<Manager> {
    Manager::RequestAsync()
        .and_then(|operation| operation.join())
        .map_err(|err| format!("Windows media sessions are unavailable: {}", err.message()))
}

/// A short name from an app's identifier: `Spotify.exe`, `308046B0AF4A39CB` (a browser) or a package.
fn app_name(id: &str) -> String {
    let name = id.rsplit('!').next().unwrap_or(id);
    let name = name.strip_suffix(".exe").unwrap_or(name);
    let known = name.split('.').next().unwrap_or(name);
    if known.len() == 16 && known.chars().all(|c| c.is_ascii_hexdigit()) {
        return "Browser".to_owned();
    }
    known.to_owned()
}

/// Now, as Windows counts time.
fn now_ticks() -> i64 {
    // SAFETY: only reads the clock.
    let now = unsafe { GetSystemTimeAsFileTime() };
    (i64::from(now.dwHighDateTime) << 32) | i64::from(now.dwLowDateTime)
}

fn read_session(session: &Session) -> windows_core::Result<MediaSession> {
    let id = session.SourceAppUserModelId()?.to_string();
    let properties = session.TryGetMediaPropertiesAsync()?.join()?;
    let info = session.GetPlaybackInfo()?;
    let controls = info.Controls()?;
    let timeline = session.GetTimelineProperties()?;
    let state = match info.PlaybackStatus()? {
        Status::Playing => PlaybackState::Playing,
        Status::Paused => PlaybackState::Paused,
        _ => PlaybackState::Stopped,
    };
    let mut position = timeline.Position()?.Duration;
    // Apps report now and then; between reports a playing track has moved on by the time passed.
    if state == PlaybackState::Playing {
        let reported = timeline.LastUpdatedTime()?.UniversalTime;
        let since = now_ticks() - reported;
        if reported > 0 && since > 0 {
            position += since;
        }
    }
    let duration = timeline.EndTime()?.Duration;
    Ok(MediaSession {
        app_name: app_name(&id),
        id,
        title: properties.Title()?.to_string(),
        artist: properties.Artist()?.to_string(),
        album: properties.AlbumTitle()?.to_string(),
        state,
        position_ms: (if duration > 0 {
            position.min(duration)
        } else {
            position
        }) / TICKS_PER_MS,
        duration_ms: duration / TICKS_PER_MS,
        can_play_pause: controls.IsPauseEnabled()? || controls.IsPlayEnabled()?,
        can_next: controls.IsNextEnabled()?,
        can_previous: controls.IsPreviousEnabled()?,
        can_seek: controls.IsPlaybackPositionEnabled()?,
        has_artwork: properties.Thumbnail().is_ok(),
    })
}

fn find(manager: &Manager, id: &str) -> Result<Session> {
    let sessions = manager.GetSessions().map_err(|err| err.message())?;
    for session in sessions {
        if session
            .SourceAppUserModelId()
            .is_ok_and(|found| found == id)
        {
            return Ok(session);
        }
    }
    Err("That media session has ended.".into())
}

impl MediaProvider for Media {
    fn availability(&self) -> Availability {
        match manager() {
            Ok(_) => Availability::Available,
            Err(why) => Availability::Unsupported(why),
        }
    }

    fn sessions(&self) -> Result<Vec<MediaSession>> {
        let manager = manager()?;
        let current = manager
            .GetCurrentSession()
            .ok()
            .and_then(|session| session.SourceAppUserModelId().ok())
            .map(|id| id.to_string());
        let list = manager.GetSessions().map_err(|err| err.message())?;
        let mut sessions = Vec::new();
        for session in list {
            // A session closing while it is read is simply not there any more.
            if let Ok(read) = read_session(&session) {
                sessions.push(read);
            }
        }
        sessions.sort_by_key(|session| Some(&session.id) != current.as_ref());
        Ok(sessions)
    }

    fn command(&self, session_id: &str, command: MediaCommand) -> Result<()> {
        let session = find(&manager()?, session_id)?;
        let operation = match command {
            MediaCommand::PlayPause => session.TryTogglePlayPauseAsync(),
            MediaCommand::Next => session.TrySkipNextAsync(),
            MediaCommand::Previous => session.TrySkipPreviousAsync(),
            MediaCommand::Seek(ms) => session.TryChangePlaybackPositionAsync(ms * TICKS_PER_MS),
        };
        let accepted = operation
            .and_then(|operation| operation.join())
            .map_err(|err| err.message())?;
        if accepted {
            Ok(())
        } else {
            Err("The app did not accept that.".into())
        }
    }

    fn artwork(&self, session_id: &str) -> Result<Option<Vec<u8>>> {
        let session = find(&manager()?, session_id)?;
        let read = || -> windows_core::Result<Option<Vec<u8>>> {
            let properties = session.TryGetMediaPropertiesAsync()?.join()?;
            let Ok(thumbnail) = properties.Thumbnail() else {
                return Ok(None);
            };
            let stream = thumbnail.OpenReadAsync()?.join()?;
            let size = stream.Size()? as u32;
            if size == 0 || size > MAX_ARTWORK {
                return Ok(None);
            }
            let reader = DataReader::CreateDataReader(&stream)?;
            reader.LoadAsync(size)?.join()?;
            let mut bytes = vec![0u8; size as usize];
            reader.ReadBytes(&mut bytes)?;
            Ok(Some(bytes))
        };
        read().map_err(|err| err.message())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_apps_from_their_identifiers() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("308046B0AF4A39CB"), "Browser");
        assert_eq!(
            app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "Microsoft"
        );
    }

    /// Against the real session manager. It may or may not have sessions; it must answer either way.
    #[test]
    fn the_session_manager_answers() {
        assert!(Media.availability().is_available());
        let sessions = Media.sessions().unwrap();
        for session in sessions {
            assert!(!session.id.is_empty());
            assert!(session.position_ms >= 0);
        }
    }

    #[test]
    fn commanding_a_session_that_does_not_exist_is_an_error_not_a_crash() {
        assert!(
            Media
                .command("no.such.app", MediaCommand::PlayPause)
                .is_err()
        );
        assert!(Media.artwork("no.such.app").is_err());
    }
}
