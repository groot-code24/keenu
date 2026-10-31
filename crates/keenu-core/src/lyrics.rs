// SPDX-License-Identifier: GPL-3.0-or-later

//! Following a song with its lyrics: which line is being sung, how a file's bytes become text, and the
//! file name lyrics are looked for under. The LRC format itself is in [`crate::lrc`].

use crate::lrc::{self, LyricLine};

/// A song's lyrics, ready to follow its playback.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lyrics {
    lines: Vec<LyricLine>,
}

impl Lyrics {
    /// Reads an LRC file's bytes, whatever its encoding. `None` when no line has a time: an invalid
    /// file, or plain text that cannot be followed.
    pub fn from_lrc_bytes(bytes: &[u8]) -> Option<Self> {
        let lines = lrc::parse(&decode(bytes));
        (!lines.is_empty()).then_some(Self { lines })
    }

    pub fn from_lines(lines: Vec<LyricLine>) -> Self {
        Self { lines }
    }

    pub fn lines(&self) -> &[LyricLine] {
        &self.lines
    }

    /// The line being sung at `position_ms`; `None` before the first. Seeking works by asking again.
    pub fn active(&self, position_ms: i64) -> Option<usize> {
        self.lines
            .partition_point(|line| line.time_ms <= position_ms)
            .checked_sub(1)
    }

    /// The lines to show around `active`, as a range: what scrolling comes to on the island.
    pub fn window(
        &self,
        active: Option<usize>,
        before: usize,
        after: usize,
    ) -> std::ops::Range<usize> {
        let centre = active.unwrap_or(0);
        let start = centre.saturating_sub(before);
        let end = (centre + after + 1).min(self.lines.len());
        start..end
    }

    /// When line `index` starts, for clicking a line to seek to it.
    pub fn time_of(&self, index: usize) -> Option<i64> {
        self.lines.get(index).map(|line| line.time_ms)
    }
}

/// Text from a lyrics file: UTF-8 or UTF-16 by their byte-order marks, UTF-8 without one, and Latin-1
/// (where every byte is a character) for the legacy files that are neither.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => bytes.iter().map(|&byte| byte as char).collect(),
    }
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| unit(pair))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Makes a song's title or artist safe in a file name on every system, and unable to leave its folder.
pub fn safe_file_part(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    cleaned.trim().trim_matches('.').trim().to_owned()
}

/// The file name a song's lyrics are looked for under: `Artist - Title.lrc`.
pub fn file_name(artist: &str, title: &str) -> Option<String> {
    let (artist, title) = (safe_file_part(artist), safe_file_part(title));
    if title.is_empty() {
        return None;
    }
    Some(if artist.is_empty() {
        format!("{title}.lrc")
    } else {
        format!("{artist} - {title}.lrc")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song() -> Lyrics {
        Lyrics::from_lrc_bytes(b"[00:01.00]one\n[00:03.00]two\n[00:05.00]three\n").unwrap()
    }

    #[test]
    fn follows_playback_and_seeking_in_both_directions() {
        let song = song();
        assert_eq!(song.active(0), None);
        assert_eq!(song.active(1_000), Some(0));
        assert_eq!(song.active(4_999), Some(1));
        assert_eq!(song.active(60_000), Some(2));
        // Seeking back is only asking again.
        assert_eq!(song.active(1_500), Some(0));
    }

    #[test]
    fn a_window_stays_inside_the_song() {
        let song = song();
        assert_eq!(song.window(None, 1, 1), 0..2);
        assert_eq!(song.window(Some(1), 1, 1), 0..3);
        assert_eq!(song.window(Some(2), 1, 5), 1..3);
    }

    #[test]
    fn a_line_can_be_clicked_to_seek() {
        assert_eq!(song().time_of(1), Some(3_000));
        assert_eq!(song().time_of(9), None);
    }

    #[test]
    fn decodes_each_encoding_lyrics_files_come_in() {
        assert_eq!(decode("añb".as_bytes()), "añb");
        assert_eq!(decode(b"\xEF\xBB\xBFabc"), "abc");
        assert_eq!(decode(&[0xFF, 0xFE, b'a', 0, b'b', 0]), "ab");
        assert_eq!(decode(&[0xFE, 0xFF, 0, b'a', 0, b'b']), "ab");
        // Not UTF-8: Latin-1, as an old Windows player would have written it.
        assert_eq!(decode(&[b'a', 0xF1, b'b']), "añb");
    }

    #[test]
    fn a_file_without_times_is_not_lyrics_to_follow() {
        assert_eq!(Lyrics::from_lrc_bytes(b"just some text\nno times"), None);
        assert_eq!(Lyrics::from_lrc_bytes(b""), None);
        assert_eq!(Lyrics::from_lrc_bytes(&[0xFF, 0xFE, 0x00]), None);
    }

    #[test]
    fn file_names_cannot_leave_the_lyrics_folder() {
        assert_eq!(
            file_name("../../etc", "pass:wd").as_deref(),
            Some("_.._etc - pass_wd.lrc")
        );
        assert_eq!(file_name("A/B", "C?").as_deref(), Some("A_B - C_.lrc"));
        assert_eq!(file_name("", "Song").as_deref(), Some("Song.lrc"));
        assert_eq!(file_name("Artist", "..."), None);
    }
}
