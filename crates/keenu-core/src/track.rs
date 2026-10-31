// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Cleaning up track metadata so lyric lookups find a match, e.g. YouTube's
//! "Artist - Song (Official Video)" by "ArtistVEVO" becomes "Song" by "Artist".

/// Words that mark a bracketed suffix as noise rather than part of the title.
const NOISE: &[&str] = &[
    "official",
    "video",
    "audio",
    "lyric",
    "lyrics",
    "visualizer",
    "mv",
    "m/v",
    "hd",
    "4k",
    "remaster",
    "remastered",
    "live",
    "explicit",
    "clip",
];

/// Returns `(title, artist)` cleaned for a lyrics search.
pub fn search_terms(title: &str, artist: &str) -> (String, String) {
    let mut title = full_title(title);
    let mut artist = clean_artist(artist);

    // "Artist - Song" in the title wins over a channel name in the artist.
    if let Some((left, right)) = title.split_once(" - ") {
        let (left, right) = (left.trim().to_owned(), right.trim().to_owned());
        if artist.is_empty() || (!right.is_empty() && looks_like_channel(&artist, &left)) {
            artist = left;
            title = right;
        }
    }

    // "Song ft. Someone": the featured artist is rarely part of the stored title.
    for marker in [" ft. ", " feat. ", " ft ", " feat "] {
        let index = title.to_lowercase().find(marker);
        // Lowercasing can change byte lengths outside ASCII; only cut on a boundary.
        if let Some(index) = index.filter(|&i| title.is_char_boundary(i)) {
            title.truncate(index);
        }
    }

    (title.trim().to_owned(), artist.trim().to_owned())
}

/// The title without noise but with any "Artist - " prefix still in it: a
/// free-text search for when the split from [`search_terms`] finds nothing.
pub fn full_title(title: &str) -> String {
    // "Song | Channel" or "Song (Lyrics) | Series": what follows the bar is
    // the uploader's branding, never part of the song's name.
    let title = title.split(" | ").next().unwrap_or(title);
    strip_noise(title)
}

/// Removes bracketed suffixes like "(Official Video)" or "\[4K\]".
fn strip_noise(title: &str) -> String {
    let mut result = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(start) = rest.find(['(', '[']) {
        let close = if rest.as_bytes()[start] == b'(' {
            ')'
        } else {
            ']'
        };
        let Some(len) = rest[start..].find(close) else {
            break;
        };
        let inner = rest[start + 1..start + len].to_lowercase();
        result.push_str(&rest[..start]);
        if !NOISE
            .iter()
            .any(|word| inner.split_whitespace().any(|w| w == *word))
        {
            result.push_str(&rest[start..=start + len]);
        }
        rest = &rest[start + len + 1..];
    }
    result.push_str(rest);
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Drops YouTube's " - Topic" and "VEVO" channel suffixes from an artist name.
fn clean_artist(artist: &str) -> String {
    let artist = artist.trim();
    let artist = artist.strip_suffix(" - Topic").unwrap_or(artist);
    let artist = artist.strip_suffix("VEVO").unwrap_or(artist);
    artist.trim().to_owned()
}

/// Whether `artist` is probably the uploader's channel rather than the
/// performer named on the left side of the title.
fn looks_like_channel(artist: &str, title_artist: &str) -> bool {
    let a = artist.to_lowercase().replace(' ', "");
    let b = title_artist.to_lowercase().replace(' ', "");
    a == b || a.contains(&b) || b.contains(&a) || a.ends_with("records") || a.ends_with("music")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_metadata_is_untouched() {
        assert_eq!(
            search_terms("Paper Sky", "Lumen Coast"),
            ("Paper Sky".into(), "Lumen Coast".into())
        );
    }

    #[test]
    fn youtube_style_titles_are_split() {
        assert_eq!(
            search_terms("Lumen Coast - Paper Sky (Official Video)", "LumenCoastVEVO"),
            ("Paper Sky".into(), "Lumen Coast".into())
        );
        assert_eq!(
            search_terms("Lumen Coast - Paper Sky [4K]", ""),
            ("Paper Sky".into(), "Lumen Coast".into())
        );
    }

    #[test]
    fn topic_channels_and_features_are_cleaned() {
        assert_eq!(
            search_terms("Paper Sky ft. Someone", "Lumen Coast - Topic"),
            ("Paper Sky".into(), "Lumen Coast".into())
        );
    }

    #[test]
    fn branding_after_a_bar_is_dropped() {
        assert_eq!(
            search_terms(
                "Lumen Coast - Paper Sky (Lyrics) | Night Drive",
                "Lyric Hub"
            ),
            ("Lumen Coast - Paper Sky".into(), "Lyric Hub".into())
        );
        assert_eq!(
            full_title("Lumen Coast - Paper Sky (Lyrics) | Night Drive"),
            "Lumen Coast - Paper Sky"
        );
    }

    #[test]
    fn meaningful_brackets_are_kept() {
        assert_eq!(search_terms("Song (Acoustic)", "A").0, "Song (Acoustic)");
    }

    #[test]
    fn dash_in_real_title_is_kept_when_artist_differs() {
        assert_eq!(search_terms("Part 1 - Intro", "Band").0, "Part 1 - Intro");
    }
}
