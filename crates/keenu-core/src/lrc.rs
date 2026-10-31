// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Parser for LRC, the de-facto format for time-synced lyrics.

use serde::{Deserialize, Serialize};

/// One line of lyrics and when it is sung.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LyricLine {
    /// Start of the line in milliseconds from the beginning of the track.
    pub time_ms: i64,
    pub text: String,
}

/// Parses synced lyrics, returning lines sorted by time. Lines without a
/// timestamp, metadata tags and timestamps too large to be real are skipped.
pub fn parse(source: &str) -> Vec<LyricLine> {
    let mut offset_ms = 0;
    let mut lines = Vec::new();

    for raw in source.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();

        while let Some(tag_end) = rest.strip_prefix('[').and_then(|r| r.find(']')) {
            let tag = &rest[1..=tag_end];
            if let Some(time) = parse_timestamp(tag) {
                times.push(time);
            } else if let Some(value) = tag.strip_prefix("offset:") {
                // A positive offset makes lyrics appear sooner.
                offset_ms = value.trim().parse().unwrap_or(0);
            }
            rest = rest[tag_end + 2..].trim_start();
        }

        let text = rest.trim();
        lines.extend(times.into_iter().map(|time_ms| LyricLine {
            // Saturating: the offset comes from the file and may be absurd.
            time_ms: time_ms.saturating_sub(offset_ms).max(0),
            text: text.to_owned(),
        }));
    }

    lines.sort_by_key(|line| line.time_ms);
    lines
}

/// Splits unsynced lyrics into lines, with `time_ms` set to -1.
pub fn plain(source: &str) -> Vec<LyricLine> {
    source
        .lines()
        .map(|line| LyricLine {
            time_ms: -1,
            text: line.trim().to_owned(),
        })
        .collect()
}

/// Parses `mm:ss`, `mm:ss.xx` or `mm:ss.xxx` into milliseconds; `None` if negative or too big.
fn parse_timestamp(tag: &str) -> Option<i64> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes: i64 = minutes.parse().ok()?;
    let (whole, fraction) = seconds.split_once(['.', ':']).unwrap_or((seconds, ""));
    let whole: i64 = whole.parse().ok()?;
    if minutes < 0 || !(0..60).contains(&whole) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // Scale ".5", ".50" and ".500" all to 500 ms.
    let fraction_ms = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<i64>().ok()? * 100,
        2 => fraction.parse::<i64>().ok()? * 10,
        _ => fraction[..3].parse::<i64>().ok()?,
    };
    minutes
        .checked_mul(60_000)?
        .checked_add(whole * 1000 + fraction_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines_and_skips_metadata() {
        let lines = parse("[ar:Someone]\n[00:01.50]Hello\n[00:03.250]World\nno time");
        assert_eq!(
            lines,
            vec![
                LyricLine {
                    time_ms: 1500,
                    text: "Hello".into()
                },
                LyricLine {
                    time_ms: 3250,
                    text: "World".into()
                },
            ]
        );
    }

    #[test]
    fn repeated_timestamps_are_sorted() {
        let lines = parse("[00:10.00][00:02.00]Chorus\n[00:05.00]Verse");
        let times: Vec<_> = lines.iter().map(|l| l.time_ms).collect();
        assert_eq!(times, [2000, 5000, 10000]);
    }

    #[test]
    fn offset_shifts_lines_earlier() {
        let lines = parse("[offset:+500]\n[00:02.00]Line");
        assert_eq!(lines[0].time_ms, 1500);
    }

    #[test]
    fn keeps_empty_instrumental_lines() {
        let lines = parse("[00:01.00]\n[00:02.00]Words");
        assert_eq!(lines[0].text, "");
    }

    #[test]
    fn rejects_malformed_timestamps() {
        assert_eq!(parse_timestamp("ab:cd"), None);
        assert_eq!(parse_timestamp("01:75.00"), None);
        assert_eq!(parse_timestamp("01:02"), Some(62_000));
        assert_eq!(parse_timestamp("-1:02"), None);
        assert_eq!(parse_timestamp("01:-2"), None);
    }

    #[test]
    fn skips_timestamps_that_overflow() {
        assert_eq!(parse_timestamp("99999999999999999:00.00"), None);
        let lines = parse("[99999999999999999:00.00]Never\n[00:01.00]Line");
        assert_eq!(
            lines,
            vec![LyricLine {
                time_ms: 1000,
                text: "Line".into()
            }]
        );
    }

    #[test]
    fn extreme_offsets_saturate() {
        let early = parse(&format!("[offset:{}]\n[00:02.00]Line", i64::MAX));
        assert_eq!(early[0].time_ms, 0);
        let late = parse(&format!("[offset:{}]\n[00:02.00]Line", i64::MIN));
        assert_eq!(late[0].time_ms, i64::MAX);
    }
}
