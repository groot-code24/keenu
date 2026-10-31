// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Clipboard history, newest first: re-copies move to the top, pins stay, the rest is pruned.
//! Images go by content hash; anything that drops items returns the hashes left unused.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Unpinned items kept, oldest dropped first; small because every card costs drawing time.
pub const MAX_UNPINNED: usize = 20;

/// At most this many items can be pinned; unpin one to pin another.
pub const MAX_PINNED: usize = 10;

/// Why an item could not be pinned or unpinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinError {
    NotFound,
    /// `MAX_PINNED` items are pinned already.
    Full,
}

/// Longest text accepted, in bytes; anything bigger is likely a file copied by accident.
pub const MAX_TEXT_BYTES: usize = 512 * 1024;

/// Characters of text shown in previews: what a card shows, and no more, as any program can read the
/// list. The full text is only sent to GNOME Shell when the item is copied again.
pub const PREVIEW_CHARS: usize = 160;

/// What was copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Clip {
    Text {
        text: String,
    },
    /// Text that is nothing but a web address, so it can be shown as one.
    Link {
        text: String,
    },
    /// A PNG image, stored by the daemon under its content hash.
    Image {
        hash: String,
        width: u32,
        height: u32,
    },
    /// Text with formatting: `text` is what it says, `rtf` how it looks, for pasting it back as it was.
    Rich {
        text: String,
        rtf: String,
    },
    /// Local files, as `file://` URIs.
    Files {
        uris: Vec<String>,
    },
}

/// Why a copy cannot go into the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipError {
    Empty,
    TooLarge,
    NotLocalFiles,
}

impl fmt::Display for ClipError {
    /// The reason as the D-Bus error tells it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "nothing was copied",
            Self::TooLarge => "too large for the clipboard history",
            Self::NotLocalFiles => "only file:// URIs can be added",
        })
    }
}

impl std::error::Error for ClipError {}

impl Clip {
    /// Copied or typed text: a link if it is a lone web address, else text.
    /// The text is kept exactly as given, so copying it back is lossless.
    pub fn from_text(text: &str) -> Result<Self, ClipError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(ClipError::TooLarge);
        }
        if text.trim().is_empty() {
            return Err(ClipError::Empty);
        }
        let text = text.to_owned();
        Ok(if is_link(&text) {
            Self::Link { text }
        } else {
            Self::Text { text }
        })
    }

    /// Copied text with its formatting, kept only when both fit; otherwise plain text.
    pub fn from_rich(text: &str, rtf: &str) -> Result<Self, ClipError> {
        if rtf.is_empty() || text.len() + rtf.len() > MAX_TEXT_BYTES {
            return Self::from_text(text);
        }
        if text.trim().is_empty() {
            return Err(ClipError::Empty);
        }
        Ok(Self::Rich {
            text: text.to_owned(),
            rtf: rtf.to_owned(),
        })
    }

    /// Copied files. Anything but a non-empty list of `file://` URIs is
    /// refused: other URIs cannot be pasted as files anyway.
    pub fn from_uris(uris: Vec<String>) -> Result<Self, ClipError> {
        if uris.is_empty() || !uris.iter().all(|uri| uri.starts_with("file://")) {
            return Err(ClipError::NotLocalFiles);
        }
        Ok(Self::Files { uris })
    }

    /// The kind as named in the D-Bus API.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Link { .. } => "link",
            Self::Rich { .. } => "rich",
            Self::Image { .. } => "image",
            Self::Files { .. } => "files",
        }
    }

    /// The text of a text or link item.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text } | Self::Link { text } | Self::Rich { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Whether the whole of it (a text, or a files item's names) holds `query`, which is lower case
    /// already; an image holds none.
    pub fn contains(&self, query: &str) -> bool {
        match self {
            Self::Text { text } | Self::Link { text } | Self::Rich { text, .. } => {
                text.to_lowercase().contains(query)
            }
            Self::Files { .. } => self.preview().to_lowercase().contains(query),
            Self::Image { .. } => false,
        }
    }

    /// The content hash of an image item.
    pub fn image_hash(&self) -> Option<&str> {
        match self {
            Self::Image { hash, .. } => Some(hash),
            _ => None,
        }
    }

    /// The URIs of a files item, else nothing.
    pub fn uris(&self) -> &[String] {
        match self {
            Self::Files { uris } => uris,
            _ => &[],
        }
    }

    /// A short form for the list: the start of a text, or the names of the
    /// files one per line. Images have none; they show a thumbnail.
    pub fn preview(&self) -> String {
        match self {
            Self::Text { text } | Self::Link { text } | Self::Rich { text, .. } => {
                truncate_chars(text, PREVIEW_CHARS).into()
            }
            Self::Image { .. } => String::new(),
            Self::Files { uris } => uris
                .iter()
                .map(|uri| file_name(uri))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Whether `other` is the same copy: text as given, images by content, files by the whole list.
    fn same_as(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image { hash: a, .. }, Self::Image { hash: b, .. }) => a == b,
            (Self::Files { uris: a }, Self::Files { uris: b }) => a == b,
            _ => self.text().is_some() && self.text() == other.text(),
        }
    }
}

/// One entry in the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipItem {
    pub id: u64,
    pub clip: Clip,
    /// Desktop app id of the app it was copied from, or "" if unknown.
    pub app_id: String,
    /// When it was last copied.
    pub at_ms: i64,
    pub pinned: bool,
}

/// The whole history, as saved to disk; ids never repeat.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardHistory {
    /// Newest first.
    items: Vec<ClipItem>,
    next_id: u64,
}

/// The outcome of adding to the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    /// The new item, or the existing one the copy was merged into.
    pub id: u64,
    /// Image hashes no item refers to any more after pruning.
    pub unreferenced: Vec<String>,
}

impl ClipboardHistory {
    /// Newest first.
    pub fn items(&self) -> &[ClipItem] {
        &self.items
    }

    /// The item with this id, if it is still kept.
    pub fn get(&self, id: u64) -> Option<&ClipItem> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Size of the image with this content hash, if it is in the history.
    pub fn image_size(&self, hash: &str) -> Option<(u32, u32)> {
        self.items.iter().find_map(|item| match &item.clip {
            Clip::Image {
                hash: h,
                width,
                height,
            } if h == hash => Some((*width, *height)),
            _ => None,
        })
    }

    /// Records a copy at the top. A copy already in the history moves up and keeps its pin;
    /// it only takes `app_id` if it had none, so the app it first came from stays known.
    pub fn add(&mut self, clip: Clip, app_id: &str, now: i64) -> Added {
        self.add_keeping(clip, app_id, now, MAX_UNPINNED)
    }

    /// As [`Self::add`], keeping `max_unpinned` unpinned items rather than the default.
    pub fn add_keeping(
        &mut self,
        clip: Clip,
        app_id: &str,
        now: i64,
        max_unpinned: usize,
    ) -> Added {
        let item = match self.items.iter().position(|item| item.clip.same_as(&clip)) {
            Some(index) => {
                let mut item = self.items.remove(index);
                item.at_ms = now;
                if item.app_id.is_empty() {
                    item.app_id = app_id.to_owned();
                }
                item
            }
            None => {
                self.next_id += 1;
                ClipItem {
                    id: self.next_id,
                    clip,
                    app_id: app_id.to_owned(),
                    at_ms: now,
                    pinned: false,
                }
            }
        };
        let id = item.id;
        self.items.insert(0, item);
        Added {
            id,
            unreferenced: self.prune_to(max_unpinned),
        }
    }

    /// Moves an item to the top, as it was just copied again.
    pub fn touch(&mut self, id: u64, now: i64) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let mut item = self.items.remove(index);
        item.at_ms = now;
        self.items.insert(0, item);
        true
    }

    /// Pins or unpins an item; pinning past `MAX_PINNED` fails, unpinning never does.
    pub fn set_pinned(&mut self, id: u64, pinned: bool) -> Result<(), PinError> {
        let index = self.index_of(id).ok_or(PinError::NotFound)?;
        let count = self.items.iter().filter(|item| item.pinned).count();
        if pinned && !self.items[index].pinned && count >= MAX_PINNED {
            return Err(PinError::Full);
        }
        self.items[index].pinned = pinned;
        Ok(())
    }

    /// Removes an item. Returns `None` if there is no such item, else the
    /// image hashes that became unreferenced.
    pub fn remove(&mut self, id: u64) -> Option<Vec<String>> {
        self.index_of(id)?;
        Some(self.drop_where(|item| item.id == id))
    }

    /// Removes every unpinned item. Returns the image hashes that became
    /// unreferenced.
    pub fn clear(&mut self) -> Vec<String> {
        self.drop_where(|item| !item.pinned)
    }

    /// Keeps only the items for which `keep` returns true. Returns the image
    /// hashes that became unreferenced.
    pub fn retain(&mut self, keep: impl Fn(&ClipItem) -> bool) -> Vec<String> {
        self.drop_where(|item| !keep(item))
    }

    /// Hashes of every image in the history.
    pub fn image_hashes(&self) -> impl Iterator<Item = &str> {
        self.items.iter().filter_map(|item| item.clip.image_hash())
    }

    /// Drops unpinned items beyond `MAX_UNPINNED`, oldest first; returns image hashes left unused.
    pub fn prune(&mut self) -> Vec<String> {
        self.prune_to(MAX_UNPINNED)
    }

    /// Drops unpinned items beyond `max_unpinned`, oldest first; returns image hashes left unused.
    pub fn prune_to(&mut self, max_unpinned: usize) -> Vec<String> {
        let mut unpinned = 0;
        self.drop_where(|item| {
            if item.pinned {
                return false;
            }
            unpinned += 1;
            unpinned > max_unpinned
        })
    }

    /// Removes the items matching `drop`, visited newest first, and
    /// returns the image hashes left without an item.
    fn drop_where(&mut self, mut drop: impl FnMut(&ClipItem) -> bool) -> Vec<String> {
        let mut dropped = Vec::new();
        self.items.retain(|item| {
            let gone = drop(item);
            if let (true, Some(hash)) = (gone, item.clip.image_hash()) {
                dropped.push(hash.to_owned());
            }
            !gone
        });
        dropped.sort_unstable();
        dropped.dedup();
        dropped.retain(|hash| !self.image_hashes().any(|kept| kept == hash));
        dropped
    }

    /// Where the item with this id sits in the list.
    fn index_of(&self, id: u64) -> Option<usize> {
        self.items.iter().position(|item| item.id == id)
    }
}

/// A stable 64-bit FNV-1a hash of `bytes` as 16 hex digits, naming stored images.
/// It is written to disk so it must never change, which rules out std's hashers.
pub fn content_hash(bytes: &[u8]) -> String {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let hash = bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    });
    format!("{hash:016x}")
}

/// A single `http://` or `https://` address with nothing around it.
fn is_link(text: &str) -> bool {
    let text = text.trim();
    let has_scheme = ["https://", "http://"].into_iter().any(|scheme| {
        text.len() > scheme.len()
            && text
                .get(..scheme.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    });
    has_scheme && !text.contains(char::is_whitespace)
}

/// The first `max` characters of `text`, never splitting one.
fn truncate_chars(text: &str, max: usize) -> &str {
    text.char_indices()
        .nth(max)
        .map_or(text, |(end, _)| &text[..end])
}

/// The decoded last path segment of a `file://` URI, e.g. "My notes.txt"
/// for `file:///home/me/My%20notes.txt`.
fn file_name(uri: &str) -> String {
    let path = uri.trim_end_matches('/');
    let name = path.rsplit('/').next().unwrap_or(path);
    percent_decode(name)
}

/// Decodes `%XX` escapes. Invalid escapes are kept as they are, and bytes
/// that do not form UTF-8 are replaced, since this is only for display.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = match bytes.get(i..i + 3) {
            Some([b'%', high, low]) => hex_digit(*high).zip(hex_digit(*low)),
            _ => None,
        };
        match escaped {
            Some((high, low)) => {
                decoded.push(high << 4 | low);
                i += 3;
            }
            None => {
                decoded.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// The value of one hex digit, either case.
fn hex_digit(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).map(|digit| digit as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Clip {
        Clip::from_text(text).unwrap()
    }

    fn image(hash: &str) -> Clip {
        Clip::Image {
            hash: hash.into(),
            width: 10,
            height: 5,
        }
    }

    fn ids(history: &ClipboardHistory) -> Vec<u64> {
        history.items().iter().map(|item| item.id).collect()
    }

    #[test]
    fn links_are_lone_web_addresses() {
        assert_eq!(text("https://example.org/a?b=c").kind(), "link");
        assert_eq!(text("  HTTP://example.org\n").kind(), "link");
        assert_eq!(text("see https://example.org").kind(), "text");
        assert_eq!(text("https://example.org two").kind(), "text");
        assert_eq!(text("https://").kind(), "text");
        assert_eq!(text("ftp://example.org").kind(), "text");
        assert_eq!(text("example.org").kind(), "text");
        assert_eq!(text("ğ").kind(), "text");
    }

    #[test]
    fn text_is_kept_exactly_but_must_have_content() {
        assert_eq!(text("  a\n").text(), Some("  a\n"));
        assert_eq!(Clip::from_text(" \n\t"), Err(ClipError::Empty));
        assert_eq!(
            Clip::from_text(&"x".repeat(MAX_TEXT_BYTES + 1)),
            Err(ClipError::TooLarge)
        );
        assert!(Clip::from_text(&"x".repeat(MAX_TEXT_BYTES)).is_ok());
    }

    #[test]
    fn files_must_be_local() {
        assert_eq!(Clip::from_uris(vec![]), Err(ClipError::NotLocalFiles));
        assert_eq!(
            Clip::from_uris(vec!["file:///a".into(), "https://b".into()]),
            Err(ClipError::NotLocalFiles)
        );
        assert!(Clip::from_uris(vec!["file:///a".into()]).is_ok());
    }

    #[test]
    fn copies_already_present_move_to_the_top() {
        let mut history = ClipboardHistory::default();
        let a = history.add(text("a"), "", 1).id;
        let b = history.add(text("b"), "first.desktop", 2).id;
        history.set_pinned(b, true).unwrap();
        history.add(text("a"), "x.desktop", 3);
        assert_eq!(history.add(text("b"), "second.desktop", 4).id, b);
        assert_eq!(ids(&history), [b, a]);

        let top = &history.items()[0];
        assert_eq!(top.at_ms, 4);
        assert!(top.pinned);
        assert_eq!(top.app_id, "first.desktop");
        assert_eq!(history.items()[1].app_id, "x.desktop");
    }

    #[test]
    fn images_and_files_dedupe_by_content() {
        let mut history = ClipboardHistory::default();
        let files = |uris: &[&str]| Clip::from_uris(uris.iter().map(|&u| u.into()).collect());
        let a = history.add(image("aa"), "", 1).id;
        let b = history
            .add(files(&["file:///x", "file:///y"]).unwrap(), "", 2)
            .id;
        assert_eq!(history.add(image("aa"), "", 3).id, a);
        assert_eq!(
            history
                .add(files(&["file:///x", "file:///y"]).unwrap(), "", 4)
                .id,
            b
        );
        assert_ne!(
            history
                .add(files(&["file:///y", "file:///x"]).unwrap(), "", 5)
                .id,
            b
        );
        assert_ne!(history.add(image("bb"), "", 6).id, a);
        assert_eq!(history.items().len(), 4);
        assert_eq!(history.image_size("aa"), Some((10, 5)));
    }

    #[test]
    fn pruning_keeps_pinned_items_and_reports_images() {
        let mut history = ClipboardHistory::default();
        let pinned = history.add(text("keep"), "", 0).id;
        history.set_pinned(pinned, true).unwrap();
        let oldest = history.add(image("old"), "", 1).id;
        for n in 1..MAX_UNPINNED {
            let added = history.add(text(&n.to_string()), "", n as i64 + 1);
            assert!(added.unreferenced.is_empty());
        }
        assert!(history.get(oldest).is_some());

        let added = history.add(text("newest"), "", 1_000);
        assert_eq!(added.unreferenced, ["old"]);
        assert!(history.get(oldest).is_none());
        assert!(history.get(pinned).is_some());
        assert_eq!(history.items().len(), MAX_UNPINNED + 1);
    }

    #[test]
    fn pins_are_capped() {
        let mut history = ClipboardHistory::default();
        let ids: Vec<_> = (0..=MAX_PINNED)
            .map(|n| history.add(text(&format!("pin {n}")), "", n as i64).id)
            .collect();
        for &id in &ids[..MAX_PINNED] {
            history.set_pinned(id, true).unwrap();
        }
        assert_eq!(
            history.set_pinned(ids[MAX_PINNED], true),
            Err(PinError::Full)
        );
        // Re-pinning a pinned item and unpinning still work when full.
        history.set_pinned(ids[0], true).unwrap();
        history.set_pinned(ids[0], false).unwrap();
        history.set_pinned(ids[MAX_PINNED], true).unwrap();
    }

    #[test]
    fn clear_keeps_pinned_items() {
        let mut history = ClipboardHistory::default();
        let kept = history.add(image("kept"), "", 1).id;
        history.add(image("gone"), "", 2);
        history.add(text("x"), "", 3);
        history.set_pinned(kept, true).unwrap();
        assert_eq!(history.clear(), ["gone"]);
        assert_eq!(ids(&history), [kept]);
    }

    #[test]
    fn touch_remove_and_pin_need_a_known_id() {
        let mut history = ClipboardHistory::default();
        let a = history.add(image("aa"), "", 1).id;
        let b = history.add(text("b"), "", 2).id;
        assert!(history.touch(a, 3));
        assert_eq!(ids(&history), [a, b]);
        assert_eq!(history.items()[0].at_ms, 3);
        assert!(!history.touch(99, 4));
        assert_eq!(history.set_pinned(99, true), Err(PinError::NotFound));
        assert_eq!(history.remove(99), None);
        assert_eq!(history.remove(b), Some(vec![]));
        assert_eq!(history.remove(a), Some(vec!["aa".into()]));
        assert!(history.items().is_empty());
    }

    #[test]
    fn shared_images_stay_referenced() {
        let mut history = ClipboardHistory::default();
        history.add(image("aa"), "", 1);
        // Not created by `add`, which dedupes, but possible in an edited file.
        let mut copy = history.items()[0].clone();
        copy.id = 50;
        history.items.push(copy);
        assert_eq!(history.remove(50), Some(vec![]));
        assert_eq!(history.retain(|_| false), ["aa"]);
    }

    #[test]
    fn previews_cut_on_character_boundaries() {
        let short = text("héllo");
        assert_eq!(short.preview(), "héllo");
        let long = text(&"é🙂".repeat(PREVIEW_CHARS));
        let preview = long.preview();
        assert_eq!(preview.chars().count(), PREVIEW_CHARS);
        assert!(preview.ends_with('🙂'));
        assert_eq!(image("aa").preview(), "");
    }

    #[test]
    fn finds_words_past_the_preview_ignoring_case() {
        let long = text(&format!(
            "{} Needle at the end",
            "x".repeat(PREVIEW_CHARS * 2)
        ));
        assert!(!long.preview().to_lowercase().contains("needle"));
        assert!(long.contains("needle"));
        assert!(text("Grüße aus Köln").contains("grüße"));
        assert!(!text("hello").contains("world"));
        assert!(!image("aa").contains(""));
        let files = Clip::from_uris(vec!["file:///home/a/My%20notes.txt".into()]).unwrap();
        assert!(files.contains("my notes"));
        assert!(!files.contains("home"));
    }

    #[test]
    fn file_previews_list_decoded_names() {
        let clip = Clip::from_uris(vec![
            "file:///home/me/My%20notes.txt".into(),
            "file:///home/me/Photos/".into(),
            "file:///tmp/caf%C3%A9%zz%4".into(),
        ])
        .unwrap();
        assert_eq!(clip.preview(), "My notes.txt\nPhotos\ncafé%zz%4");
        assert_eq!(clip.uris().len(), 3);
        assert!(text("a").uris().is_empty());
    }

    #[test]
    fn content_hash_is_fnv1a() {
        assert_eq!(content_hash(b""), "cbf29ce484222325");
        assert_eq!(content_hash(b"a"), "af63dc4c8601ec8c");
        assert_eq!(content_hash(b"foobar"), "85944171f73967e8");
    }

    #[test]
    fn history_survives_a_round_trip() {
        let mut history = ClipboardHistory::default();
        history.add(text("https://example.org"), "a.desktop", 1);
        history.add(image("aa"), "", 2);
        history.add(Clip::from_uris(vec!["file:///a".into()]).unwrap(), "", 3);
        let json = serde_json::to_string(&history).unwrap();
        let restored: ClipboardHistory = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.items(), history.items());
        assert_eq!(restored.next_id, 3);
    }
}
