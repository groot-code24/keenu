// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Clipboard history over D-Bus: the shell reports copies, keenud keeps the list and images.
//! Images are stored once per hash in `~/.local/share/keenu/clips`, each with a thumbnail.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use image::ImageFormat;
use keenu_core::clipboard::{
    Added, Clip, ClipError, ClipboardHistory, MAX_PINNED, PinError, content_hash,
};
use log::{debug, warn};
use serde::Serialize;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

use crate::clock::now_ms;
use crate::store::{Store, create_private_dir, data_dir, write_private};

/// Largest image accepted, in bytes of PNG.
const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;

/// Thumbnails fit within this box, which is about the size the island
/// shows them at on a high-density display.
const THUMB_WIDTH: u32 = 320;
const THUMB_HEIGHT: u32 = 200;

/// The Clipboard1 D-Bus object: the saved history and the image files it refers to.
pub struct ClipboardService {
    history: ClipboardHistory,
    store: Store<ClipboardHistory>,
    clips: ClipFiles,
}

impl ClipboardService {
    /// Loads the history and tidies it against the image files on disk.
    pub fn load() -> Self {
        let mut store = Store::new("clipboard");
        let mut history: ClipboardHistory = store.load();
        let clips = ClipFiles {
            dir: data_dir().join("clips"),
        };

        // An image item is useless without its files, which the user may
        // have deleted while keenud was not running.
        let before = history.items().len();
        history.retain(|item| item.clip.image_hash().is_none_or(|hash| clips.has(hash)));
        // Their files go with the orphans below.
        history.prune();
        if history.items().len() != before {
            store.save(&history);
        }
        // Folders made by older versions were readable by others.
        if clips.dir.is_dir()
            && let Err(err) = create_private_dir(&clips.dir)
        {
            warn!("could not make {} private: {err}", clips.dir.display());
        }
        // Without the real list (unreadable, or corrupt and set aside), every
        // image would look like an orphan.
        if !store.lost_track() {
            clips.remove_orphans(&history.image_hashes().collect());
        }
        Self {
            history,
            store,
            clips,
        }
    }

    /// Saves the history and signals `Items`.
    async fn changed(&self, emitter: &SignalEmitter<'_>) -> zbus::Result<()> {
        self.store.save(&self.history);
        self.items_changed(emitter).await
    }

    /// Saves and announces an addition, then deletes the image files the
    /// addition pushed out of the history.
    async fn added(&self, added: Added, emitter: &SignalEmitter<'_>) -> fdo::Result<u64> {
        self.changed(emitter).await?;
        self.clips.remove(&added.unreferenced);
        Ok(added.id)
    }

    /// The `Items` list, built fresh from the history.
    fn view(&self) -> Vec<ItemView<'_>> {
        self.history
            .items()
            .iter()
            .map(|item| {
                let image = match &item.clip {
                    Clip::Image {
                        hash,
                        width,
                        height,
                    } => Some((hash, *width, *height)),
                    _ => None,
                };
                ItemView {
                    id: item.id,
                    kind: item.clip.kind(),
                    preview: item.clip.preview(),
                    uris: item.clip.uris(),
                    image: image.map(|(hash, ..)| path_string(&self.clips.image(hash))),
                    thumb: image.map(|(hash, ..)| path_string(&self.clips.thumb(hash))),
                    width: image.map(|(_, width, _)| width),
                    height: image.map(|(.., height)| height),
                    app_id: &item.app_id,
                    at_ms: item.at_ms,
                    pinned: item.pinned,
                }
            })
            .collect()
    }
}

/// An `Items` entry: text cut to a preview, images as paths, so the list stays small.
/// `GetText` returns the full content.
#[derive(Serialize)]
struct ItemView<'a> {
    id: u64,
    kind: &'static str,
    preview: String,
    uris: &'a [String],
    image: Option<String>,
    thumb: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    app_id: &'a str,
    at_ms: i64,
    pinned: bool,
}

/// The error for an id that is not in the history.
fn not_found(id: u64) -> fdo::Error {
    fdo::Error::InvalidArgs(format!("no clipboard item with id {id}"))
}

/// A clip the history refused, as a D-Bus error.
fn invalid(err: ClipError) -> fdo::Error {
    fdo::Error::InvalidArgs(err.to_string())
}

#[interface(name = "io.github.zickrian.isle.Clipboard1")]
impl ClipboardService {
    /// Records copied text and returns its id; copying the same text again
    /// moves the existing item to the top.
    async fn add_text(
        &mut self,
        text: &str,
        app_id: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<u64> {
        let clip = Clip::from_text(text).map_err(invalid)?;
        let added = self.history.add(clip, app_id, now_ms());
        self.added(added, &emitter).await
    }

    /// Records a copied PNG image and returns its id. Decoding and scaling
    /// a large screenshot takes a moment, so it runs off the event loop.
    async fn add_image(
        &mut self,
        png: Vec<u8>,
        app_id: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<u64> {
        if png.len() > MAX_IMAGE_BYTES {
            return Err(invalid(ClipError::TooLarge));
        }
        let (png, hash) = blocking(move || {
            let hash = content_hash(&png);
            (png, hash)
        })
        .await?;

        // A known image only needs its files written again if they are gone.
        let known = self
            .history
            .image_size(&hash)
            .filter(|_| self.clips.has(&hash));
        let (width, height) = match known {
            Some(size) => size,
            None => {
                let clips = self.clips.clone();
                let hash = hash.clone();
                blocking(move || clips.store(&hash, &png)).await??
            }
        };
        let clip = Clip::Image {
            hash,
            width,
            height,
        };
        let added = self.history.add(clip, app_id, now_ms());
        let added = self.added(added, &emitter).await;
        // The image and its decoded pixels are gone now.
        crate::trim_memory();
        added
    }

    /// Records copied files (`file://` URIs) and returns the item's id.
    async fn add_files(
        &mut self,
        uris: Vec<String>,
        app_id: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<u64> {
        let clip = Clip::from_uris(uris).map_err(invalid)?;
        let added = self.history.add(clip, app_id, now_ms());
        self.added(added, &emitter).await
    }

    /// The full content to put back on the clipboard: the text, the file
    /// URIs one per line, or the path of an image's PNG file. GNOME Shell's to ask, no other program's.
    async fn get_text(
        &self,
        id: u64,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<String> {
        crate::caller::shell_only(connection, &header).await?;
        let item = self.history.get(id).ok_or_else(|| not_found(id))?;
        Ok(match &item.clip {
            Clip::Text { text } | Clip::Link { text } | Clip::Rich { text, .. } => text.clone(),
            Clip::Files { uris } => uris.join("\n"),
            Clip::Image { hash, .. } => path_string(&self.clips.image(hash)),
        })
    }

    /// The ids of the items whose whole text (or files' names) holds `query`, ignoring case, in the list's
    /// order. GNOME Shell's to ask, no other program's: guess after guess, it would tell what was copied.
    async fn search(
        &self,
        query: &str,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<Vec<u64>> {
        crate::caller::shell_only(connection, &header).await?;
        let query = query.to_lowercase();
        // A copy of the texts, searched off the event loop: they can be long.
        let items: Vec<(u64, Clip)> = self
            .history
            .items()
            .iter()
            .filter(|item| item.clip.image_hash().is_none())
            .map(|item| (item.id, item.clip.clone()))
            .collect();
        blocking(move || {
            items
                .into_iter()
                .filter(|(_, clip)| clip.contains(&query))
                .map(|(id, _)| id)
                .collect()
        })
        .await
    }

    /// Moves an item to the top, as the user just copied it again.
    async fn touch(
        &mut self,
        id: u64,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        if !self.history.touch(id, now_ms()) {
            return Err(not_found(id));
        }
        Ok(self.changed(&emitter).await?)
    }

    /// Pins or unpins an item; at most `MAX_PINNED` can be pinned.
    async fn set_pinned(
        &mut self,
        id: u64,
        pinned: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        match self.history.set_pinned(id, pinned) {
            Ok(()) => {}
            Err(PinError::NotFound) => return Err(not_found(id)),
            Err(PinError::Full) => {
                return Err(fdo::Error::LimitsExceeded(format!(
                    "at most {MAX_PINNED} items can be pinned"
                )));
            }
        }
        Ok(self.changed(&emitter).await?)
    }

    /// Removes one item, and its image files if nothing else uses them.
    async fn remove(
        &mut self,
        id: u64,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let unreferenced = self.history.remove(id).ok_or_else(|| not_found(id))?;
        self.changed(&emitter).await?;
        self.clips.remove(&unreferenced);
        Ok(())
    }

    /// Removes every item that is not pinned.
    async fn clear(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let unreferenced = self.history.clear();
        self.changed(&emitter).await?;
        self.clips.remove(&unreferenced);
        Ok(())
    }

    /// JSON array of items, newest first (see `ItemView`).
    #[zbus(property)]
    fn items(&self) -> String {
        serde_json::to_string(&self.view()).unwrap_or_default()
    }
}

/// Runs blocking work on tokio's blocking pool, so the D-Bus connection,
/// which shares keenud's only thread with everything else, keeps flowing.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> fdo::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|err| fdo::Error::Failed(format!("clipboard work failed: {err}")))
}

/// The directory of stored images: `<hash>.png`, the image as copied, and
/// `<hash>.thumb.png`, its thumbnail.
#[derive(Clone)]
struct ClipFiles {
    dir: PathBuf,
}

impl ClipFiles {
    /// The image as copied.
    fn image(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.png"))
    }

    /// Its thumbnail.
    fn thumb(&self, hash: &str) -> PathBuf {
        self.dir.join(format!("{hash}.thumb.png"))
    }

    /// Both files are there.
    fn has(&self, hash: &str) -> bool {
        self.image(hash).is_file() && self.thumb(hash).is_file()
    }

    /// Stores a PNG and its thumbnail, returning the image's size. Blocks; the decoder's
    /// default limits cap how much memory a hostile image can claim.
    fn store(&self, hash: &str, png: &[u8]) -> fdo::Result<(u32, u32)> {
        let image = image::load_from_memory_with_format(png, ImageFormat::Png)
            .map_err(|err| fdo::Error::InvalidArgs(format!("not a readable PNG image: {err}")))?;
        let (width, height) = (image.width(), image.height());

        let io_error =
            |err: io::Error| fdo::Error::IOError(format!("could not store image: {err}"));
        create_private_dir(&self.dir).map_err(io_error)?;
        write_replacing(&self.image(hash), png).map_err(io_error)?;
        if width <= THUMB_WIDTH && height <= THUMB_HEIGHT {
            // Already small enough; scaling it up would only blur it.
            write_replacing(&self.thumb(hash), png).map_err(io_error)?;
        } else {
            let mut thumb = Cursor::new(Vec::new());
            image
                .thumbnail(THUMB_WIDTH, THUMB_HEIGHT)
                .write_to(&mut thumb, ImageFormat::Png)
                .map_err(|err| fdo::Error::Failed(format!("could not make a thumbnail: {err}")))?;
            write_replacing(&self.thumb(hash), thumb.get_ref()).map_err(io_error)?;
        }
        Ok((width, height))
    }

    /// Deletes the files of images no item refers to any more.
    fn remove(&self, hashes: &[String]) {
        for hash in hashes {
            for path in [self.image(hash), self.thumb(hash)] {
                remove_file(&path);
            }
        }
    }

    /// Deletes files not in `referenced`: images dropped while keenud was down, and
    /// leftovers of interrupted writes.
    fn remove_orphans(&self, referenced: &HashSet<&str>) {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return,
            Err(err) => {
                debug!("could not list {}: {err}", self.dir.display());
                return;
            }
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let hash = name.to_str().and_then(|name| {
                name.strip_suffix(".thumb.png")
                    .or_else(|| name.strip_suffix(".png"))
            });
            if !hash.is_some_and(|hash| referenced.contains(hash)) {
                remove_file(&entry.path());
            }
        }
    }
}

/// Writes through a temporary file and renames it into place, so an
/// interrupted write never leaves a half-written image under a real name.
fn write_replacing(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    write_private(Path::new(&temp), bytes)?;
    fs::rename(&temp, path)
}

/// Best effort: a file that cannot be deleted only costs disk space, and is
/// tried again as an orphan when keenud next starts.
fn remove_file(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => debug!("removed {}", path.display()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => debug!("could not remove {}: {err}", path.display()),
    }
}

/// For D-Bus, which wants text even for a path that is not UTF-8.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
