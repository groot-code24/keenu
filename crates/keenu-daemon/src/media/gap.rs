// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Browsers report nothing for a moment between videos; a short gap after a playing
//! song is shown as still playing, so the island stays steady. Real pauses show at once.

use std::time::Duration;

use tokio::time::Instant;

use super::NowPlaying;

/// The longest gap bridged; after it, whatever is there shows as it is.
pub const GRACE: Duration = Duration::from_millis(1500);

/// `P` is the player behind a song, so controls go to the one shown.
#[derive(Debug)]
pub struct GapBridge<P> {
    /// The song last seen playing, and its player.
    playing: Option<(NowPlaying, P)>,
    /// When the gap after it began.
    since: Option<Instant>,
}

impl<P> Default for GapBridge<P> {
    /// Written out, since a derive would require `P: Default`.
    fn default() -> Self {
        Self {
            playing: None,
            since: None,
        }
    }
}

impl<P: Clone> GapBridge<P> {
    /// What to show for `actual` (played by `player`), and when the bridged gap ends.
    pub fn show(
        &mut self,
        actual: NowPlaying,
        player: P,
        now: Instant,
    ) -> (NowPlaying, P, Option<Instant>) {
        if is_playing(&actual) {
            self.playing = Some((actual.clone(), player.clone()));
            self.since = None;
            return (actual, player, None);
        }
        let Some((last, last_player)) = &self.playing else {
            return (actual, player, None);
        };
        let same_song = !actual.title.is_empty()
            && actual.title == last.title
            && actual.artist == last.artist
            && actual.identity == last.identity;
        let since = *self.since.get_or_insert(now);
        if same_song || now.duration_since(since) >= GRACE {
            self.playing = None;
            self.since = None;
            return (actual, player, None);
        }
        let until = Some(since + GRACE);
        if actual.title.is_empty() || actual.identity != last.identity {
            return (last.clone(), last_player.clone(), until);
        }
        // The next song, not playing yet: shown as it will be.
        let shown = NowPlaying {
            status: "Playing".into(),
            ..actual
        };
        (shown, player, until)
    }
}

/// Whether a song is really playing, with a title to show.
fn is_playing(now_playing: &NowPlaying) -> bool {
    now_playing.available && now_playing.status == "Playing" && !now_playing.title.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, status: &str) -> NowPlaying {
        NowPlaying {
            available: true,
            identity: "Chrome".into(),
            status: status.into(),
            title: title.into(),
            artist: "Artist".into(),
            ..NowPlaying::default()
        }
    }

    #[test]
    fn bridges_a_short_gap_between_songs() {
        let mut bridge = GapBridge::<u8>::default();
        let start = Instant::now();
        assert_eq!(bridge.show(song("One", "Playing"), 0, start).0.title, "One");

        // Nothing for a moment: still the first song, playing.
        let (shown, _, until) = bridge.show(song("", "Paused"), 0, start);
        assert_eq!(
            (shown.title.as_str(), shown.status.as_str()),
            ("One", "Playing")
        );
        assert_eq!(until, Some(start + GRACE));

        // The next one's details, not playing yet: shown as playing.
        let later = start + Duration::from_millis(200);
        let (shown, _, _) = bridge.show(song("Two", "Paused"), 0, later);
        assert_eq!(
            (shown.title.as_str(), shown.status.as_str()),
            ("Two", "Playing")
        );

        let (shown, _, until) = bridge.show(song("Two", "Playing"), 0, later);
        assert_eq!((shown.title.as_str(), until), ("Two", None));
    }

    #[test]
    fn shows_a_pause_at_once() {
        let mut bridge = GapBridge::<u8>::default();
        let start = Instant::now();
        bridge.show(song("One", "Playing"), 0, start);
        let (shown, _, until) = bridge.show(song("One", "Paused"), 0, start);
        assert_eq!((shown.status.as_str(), until), ("Paused", None));
        // Resuming later is not a gap either.
        bridge.show(song("One", "Playing"), 0, start);
    }

    #[test]
    fn a_long_gap_shows_as_it_is() {
        let mut bridge = GapBridge::<u8>::default();
        let start = Instant::now();
        bridge.show(song("One", "Playing"), 0, start);
        bridge.show(NowPlaying::default(), 0, start);
        let (shown, _, until) = bridge.show(NowPlaying::default(), 0, start + GRACE);
        assert_eq!((shown.available, until), (false, None));
        // And nothing more is bridged until a song plays again.
        let (shown, _, _) = bridge.show(song("Two", "Paused"), 0, start + GRACE * 2);
        assert_eq!(shown.status, "Paused");
    }

    #[test]
    fn another_paused_player_does_not_show_during_the_gap() {
        let mut bridge = GapBridge::<u8>::default();
        let start = Instant::now();
        bridge.show(song("One", "Playing"), 0, start);
        let spotify = NowPlaying {
            identity: "Spotify".into(),
            ..song("Elsewhere", "Paused")
        };
        let (shown, player, _) = bridge.show(spotify, 2, start);
        // Shown with its own player, so the buttons control that one.
        assert_eq!((shown.title.as_str(), player), ("One", 0));
    }
}
