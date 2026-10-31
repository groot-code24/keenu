// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Watches every MPRIS player on the session bus and publishes the active one.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::StreamExt;
use log::{debug, warn};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio::time::Instant;
use zbus::fdo::{DBusProxy, PropertiesProxy};
use zbus::names::{InterfaceName, OwnedBusName};
use zbus::object_server::InterfaceRef;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, proxy};

use super::gap::GapBridge;
use super::{MediaService, NowPlaying, PlayerChoice, Wish, Wishes, art};
use crate::clock::now_ms;
use keenu_integrations::lyrics::{self, Lyrics, LyricsStatus, TrackQuery};

const NAME_PREFIX: &str = "org.mpris.MediaPlayer2.";
/// playerctld re-exports other players; listening to it would duplicate them.
const IGNORED: &[&str] = &["org.mpris.MediaPlayer2.playerctld"];
const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
/// A player sets its properties one by one when a song changes: those coming this close together are read
/// once.
const BURST: Duration = Duration::from_millis(40);
const APP_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
/// The bus connection has no method timeout, and a hung player would
/// otherwise stall whatever awaits it for good.
const PLAYER_TIMEOUT: Duration = Duration::from_secs(5);
/// How long cover art that failed to download is left alone. Without this,
/// every update would try the dead link (offline, 404, oversized) again.
const ART_RETRY: Duration = Duration::from_secs(5 * 60);
/// First wait before looking up lyrics again after a failed lookup; it
/// doubles with every failure for the same track, up to [`LYRICS_RETRY_MAX`].
const LYRICS_RETRY: Duration = Duration::from_secs(30);
const LYRICS_RETRY_MAX: Duration = Duration::from_secs(10 * 60);

/// The MPRIS player controls Keenu uses.
#[proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2",
    gen_blocking = false
)]
pub trait Player {
    /// Toggles playback.
    fn play_pause(&self) -> zbus::Result<()>;
    /// Skips to the next track.
    fn next(&self) -> zbus::Result<()>;
    /// Back to the previous track.
    fn previous(&self) -> zbus::Result<()>;
    /// Moves by `offset` microseconds from where it is.
    fn seek(&self, offset: i64) -> zbus::Result<()>;
    /// Jumps to `position` microseconds, if `track_id` is still the track playing.
    fn set_position(&self, track_id: &ObjectPath<'_>, position: i64) -> zbus::Result<()>;
    /// Where playback is now, in microseconds.
    #[zbus(property)]
    fn position(&self) -> zbus::Result<i64>;
    /// Sent when the position jumps rather than moving on with time.
    #[zbus(signal)]
    fn seeked(&self, position: i64) -> zbus::Result<()>;
}

/// The player's app, to bring its window forward.
#[proxy(
    interface = "org.mpris.MediaPlayer2",
    default_path = "/org/mpris/MediaPlayer2",
    gen_blocking = false
)]
pub trait App {
    /// Shows the player's window.
    fn raise(&self) -> zbus::Result<()>;
}

/// What the watch loop is woken for, besides bus names and wishes from the extension.
enum Event {
    /// A player's properties changed. `seeked` is the position from a `Seeked` in that same burst.
    Player {
        name: OwnedBusName,
        seeked: Option<i64>,
    },
    /// `mpris:length` was still missing; read the player again.
    LengthRetry(OwnedBusName),
    /// A cover art download finished, successfully or not.
    ArtDone { url: String, ok: bool },
    /// The result of lyrics lookup number `lookup`.
    Lyrics { lookup: u64, lyrics: Lyrics },
    /// Time to retry lyrics lookup number `lookup`, which failed.
    RetryLyrics { lookup: u64 },
    /// A gap between songs may be over; see [`GapBridge`].
    GapOver,
}

/// Everything read from one player's `GetAll`.
#[derive(Debug, Clone, Default, PartialEq)]
struct Snapshot {
    status: String,
    title: String,
    artist: String,
    album: String,
    art_url: String,
    track_id: Option<OwnedObjectPath>,
    length_us: i64,
    position_us: i64,
    sampled_at_ms: i64,
    rate: f64,
    can_go_next: bool,
    can_go_previous: bool,
    can_play_pause: bool,
    can_seek: bool,
    /// `Position` was in the read. Absent is not the same as a reported 0.
    position_known: bool,
}

/// One player being watched.
struct Entry {
    player: PlayerProxy<'static>,
    properties: PropertiesProxy<'static>,
    identity: String,
    desktop_entry: String,
    /// A web browser, whose reads are stale at times (see [`reconcile`]).
    browser: bool,
    snapshot: Snapshot,
    /// Order in which players last started playing; the latest one wins.
    started_playing: u64,
    listener: AbortHandle,
    /// Re-reads while the track's length is still unknown, at most two.
    length_tries: u8,
    /// A length re-read is already waiting.
    length_waiting: bool,
}

impl Drop for Entry {
    /// Stops listening to a player that left the bus.
    fn drop(&mut self) {
        self.listener.abort();
    }
}

/// All the watching state, owned by the loop in [`watch`].
struct Watcher {
    conn: Connection,
    iface: InterfaceRef<MediaService>,
    players: HashMap<OwnedBusName, Entry>,
    play_counter: u64,
    events: mpsc::UnboundedSender<Event>,
    /// Remote art currently downloading, to avoid starting it twice.
    downloading_art: Option<String>,
    /// Remote art that could not be downloaded, and when; it is not tried
    /// again until [`ART_RETRY`] has passed and the player next updates.
    failed_art: HashMap<String, Instant>,
    /// The last art URL found on disk and its path, so publishing again needs no file lookup.
    art_found: Option<(String, String)>,
    /// The track whose lyrics are loaded or loading.
    lyrics_query: Option<TrackQuery>,
    /// Numbers each lyrics lookup, so late results and retry timers that
    /// belong to an earlier one are recognised and dropped.
    lyrics_lookup: u64,
    /// Failed lookups in a row for the current track, for the retry backoff.
    lyrics_failures: u32,
    gap: GapBridge<Option<(PlayerProxy<'static>, Option<OwnedObjectPath>)>>,
    /// When a `GapOver` is due, so only one waits at a time.
    gap_check: Option<Instant>,
    /// The player the user picked; it stays until another starts playing.
    chosen: Option<OwnedBusName>,
    /// Whether lyrics are looked up online (Settings); `None` until the extension says.
    lyrics_online: Option<bool>,
    /// The current track's lyrics wait to be looked up (see [`lyrics_step`]).
    lyrics_waiting: bool,
}

/// Runs forever, keeping `iface` in sync with the active player and acting on its `wished`.
pub async fn watch(
    conn: Connection,
    iface: InterfaceRef<MediaService>,
    mut wished: Wishes,
) -> anyhow::Result<()> {
    let dbus = DBusProxy::new(&conn).await?;
    // Subscribe before listing, so no player can slip in between; only players' names, not every app's.
    let owners_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg0ns(NAME_PREFIX.trim_end_matches('.'))?
        .build();
    let mut owners = MessageStream::for_match_rule(owners_rule, &conn, None).await?;
    let (events, mut received) = mpsc::unbounded_channel();
    let mut watcher = Watcher {
        conn,
        iface,
        players: HashMap::new(),
        play_counter: 0,
        events,
        downloading_art: None,
        failed_art: HashMap::new(),
        art_found: None,
        lyrics_query: None,
        lyrics_lookup: 0,
        lyrics_failures: 0,
        gap: GapBridge::default(),
        gap_check: None,
        chosen: None,
        lyrics_online: None,
        lyrics_waiting: false,
    };

    for name in dbus.list_names().await? {
        if is_player(name.as_str()) {
            watcher.add(name).await;
        }
    }
    watcher.publish().await;

    loop {
        tokio::select! {
            Some(Ok(signal)) = owners.next() => {
                let Ok((name, _old, new)) = signal.body().deserialize::<(String, String, String)>()
                else {
                    continue;
                };
                if !is_player(&name) {
                    continue;
                }
                let Ok(name) = OwnedBusName::try_from(name) else { continue };
                watcher.players.remove(&name);
                if !new.is_empty() {
                    watcher.add(name).await;
                }
            }
            Some(wish) = wished.recv() => watcher.wish(wish),
            Some(event) = received.recv() => match event {
                Event::Player { name, seeked } => watcher.refresh(&name, seeked).await,
                Event::LengthRetry(name) => {
                    if let Some(entry) = watcher.players.get_mut(&name) {
                        entry.length_waiting = false;
                    }
                    watcher.refresh(&name, None).await;
                }
                Event::ArtDone { url, ok } => watcher.art_done(url, ok),
                Event::Lyrics { lookup, lyrics } => {
                    watcher.lyrics_done(lookup, lyrics).await;
                    continue;
                }
                Event::RetryLyrics { lookup } => {
                    watcher.retry_lyrics(lookup);
                    continue;
                }
                Event::GapOver => watcher.gap_check = None,
            },
            else => return Ok(()),
        }
        watcher.publish().await;
    }
}

/// The player to show: the one picked by hand while it has something to show, else one with a track
/// (playing first), else the one playing, else the one that played last (each with when it started).
fn pick<'a, N: PartialEq + 'a>(
    players: impl Iterator<Item = (&'a N, &'a Snapshot, u64)> + Clone,
    chosen: Option<&N>,
) -> Option<&'a N> {
    if let Some((name, ..)) = players
        .clone()
        .find(|&(name, snapshot, _)| Some(name) == chosen && shows(snapshot))
    {
        return Some(name);
    }
    // A player stopped with nothing in it never hides a paused song.
    players
        .max_by_key(|&(_, s, started)| {
            let track = s.status != "Stopped" && shows(s);
            (track, s.status == "Playing", started)
        })
        .map(|(name, ..)| name)
}

/// Whether the island has anything to show for a player: with no title, it says "Nothing playing".
fn shows(snapshot: &Snapshot) -> bool {
    !snapshot.title.is_empty()
}

/// An MPRIS name, but not playerctld's copies of other players.
fn is_player(name: &str) -> bool {
    name.starts_with(NAME_PREFIX) && !IGNORED.contains(&name)
}

/// Awaits a call to a player, giving up after [`PLAYER_TIMEOUT`].
pub(super) async fn bounded<T, E: From<zbus::Error>>(
    call: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    tokio::time::timeout(PLAYER_TIMEOUT, call)
        .await
        .unwrap_or_else(|_| {
            let err = zbus::Error::Failure("the media player did not answer in time".into());
            Err(err.into())
        })
}

impl Watcher {
    /// Starts watching a player that appeared, and reads it once.
    async fn add(&mut self, name: OwnedBusName) {
        match self.connect(name.clone()).await {
            Ok(entry) => {
                debug!("found media player {name}");
                self.players.insert(name.clone(), entry);
                self.refresh(&name, None).await;
            }
            Err(err) => warn!("could not watch media player {name}: {err}"),
        }
    }

    /// Reads the player's name and app, and listens for its changes in bursts.
    async fn connect(&self, name: OwnedBusName) -> zbus::Result<Entry> {
        let player = PlayerProxy::builder(&self.conn)
            .destination(name.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let properties = PropertiesProxy::builder(&self.conn)
            .destination(name.clone())?
            .path(OBJECT_PATH)?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;

        let app =
            bounded(properties.get_all(InterfaceName::from_static_str_unchecked(APP_INTERFACE)))
                .await
                .unwrap_or_default();
        let identity = app
            .get("Identity")
            .and_then(|v| string(v))
            .unwrap_or_default();
        let desktop_entry = app
            .get("DesktopEntry")
            .and_then(|v| string(v))
            .unwrap_or_default();
        let browser = is_browser(&[name.as_str(), &identity, &desktop_entry]);

        // Only the player's own properties: Identity and the like are read once, above.
        let mut changed = properties
            .receive_properties_changed_with_args(&[(0, PLAYER_INTERFACE)])
            .await?;
        let mut seeked = player.receive_seeked().await?;
        let events = self.events.clone();
        let listener = tokio::spawn(async move {
            // Firefox reports Position as 0 after a seek, but the Seeked signal carries the real one.
            let mut seek_to: Option<i64> = None;
            loop {
                tokio::select! {
                    Some(_) = changed.next() => {}
                    Some(signal) = seeked.next() => {
                        if let Ok(args) = signal.args() {
                            seek_to = Some(args.position);
                        }
                    }
                    else => break,
                }
                // The rest of a burst comes with it, and the player is read once.
                let quiet = tokio::time::sleep(BURST);
                tokio::pin!(quiet);
                loop {
                    tokio::select! {
                        () = &mut quiet => break,
                        Some(_) = changed.next() => {}
                        Some(signal) = seeked.next() => {
                            if let Ok(args) = signal.args() {
                                seek_to = Some(args.position);
                            }
                        }
                    }
                }
                let seeked_us = seek_to.take();
                if events
                    .send(Event::Player {
                        name: name.clone(),
                        seeked: seeked_us,
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .abort_handle();

        Ok(Entry {
            player,
            properties,
            identity,
            desktop_entry,
            browser,
            snapshot: Snapshot::default(),
            started_playing: 0,
            listener,
            length_tries: 0,
            length_waiting: false,
        })
    }

    /// Reads a player again; one that just started playing takes over from a hand-picked one.
    /// `seeked` is the position from a `Seeked` in this burst, which wins over the property read.
    async fn refresh(&mut self, name: &OwnedBusName, seeked: Option<i64>) {
        let Some(entry) = self.players.get_mut(name) else {
            return;
        };
        let all = bounded(
            entry
                .properties
                .get_all(InterfaceName::from_static_str_unchecked(PLAYER_INTERFACE)),
        )
        .await;
        let read = match all {
            Ok(all) => snapshot(&all),
            Err(err) => {
                debug!("could not read {name}: {err}");
                return;
            }
        };
        let previous = entry.snapshot.clone();
        let same = same_track(&previous, &read);
        let snapshot = reconcile(&previous, read, same, seeked, entry.browser);
        if snapshot.status == "Playing" && previous.status != "Playing" {
            self.play_counter += 1;
            entry.started_playing = self.play_counter;
            // Another player started: the one picked by hand gives way.
            if self.chosen.as_ref().is_some_and(|chosen| chosen != name) {
                self.chosen = None;
            }
        }
        if !same {
            entry.length_tries = 0;
            entry.length_waiting = false;
        }
        let needs_length = snapshot.status == "Playing" && snapshot.length_us <= 0;
        if !needs_length {
            entry.length_tries = 0;
        }
        entry.snapshot = snapshot;
        if needs_length {
            self.schedule_length_retry(name);
        }
    }

    /// Reads the player again soon when it is playing and still has no length.
    /// Firefox often sends the duration a moment after playback starts, without another signal worth trusting.
    fn schedule_length_retry(&mut self, name: &OwnedBusName) {
        let Some(entry) = self.players.get_mut(name) else {
            return;
        };
        if entry.length_waiting || entry.length_tries >= 2 {
            return;
        }
        entry.length_waiting = true;
        entry.length_tries += 1;
        let delay = Duration::from_secs(if entry.length_tries == 1 { 1 } else { 3 });
        let events = self.events.clone();
        let name = name.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = events.send(Event::LengthRetry(name));
        });
    }

    /// The player picked by hand, else the one that is playing, else the one that played most recently.
    fn active(&self) -> Option<&Entry> {
        self.active_name().and_then(|name| self.players.get(name))
    }

    /// The bus name of the player to show (see `pick`).
    fn active_name(&self) -> Option<&OwnedBusName> {
        let players = self
            .players
            .iter()
            .map(|(name, entry)| (name, &entry.snapshot, entry.started_playing));
        pick(players, self.chosen.as_ref())
    }

    /// What the user asked for in the island: a player picked, or lyrics turned on or off.
    fn wish(&mut self, wish: Wish) {
        match wish {
            Wish::Player(name) => {
                self.chosen = OwnedBusName::try_from(name)
                    .ok()
                    .filter(|name| self.players.contains_key(name));
            }
            Wish::Lyrics(online) if Some(online) != self.lyrics_online => {
                self.lyrics_online = Some(online);
                // The next publish looks them up at once (or puts them away) for the current track.
                self.lyrics_query = None;
                self.lyrics_lookup += 1;
            }
            Wish::Lyrics(_) => {}
        }
    }

    /// The players with something to show, for picking one; empty with fewer than two.
    /// An idle one (Rhythmbox stopped, `mpv --idle`) would show "Nothing playing" and no way back.
    fn choices(&self) -> Vec<PlayerChoice> {
        let active = self.active_name();
        let mut choices: Vec<PlayerChoice> = self
            .players
            .iter()
            .filter(|(_, entry)| shows(&entry.snapshot))
            .map(|(name, entry)| PlayerChoice {
                name: name.to_string(),
                identity: entry.identity.clone(),
                desktop_entry: entry.desktop_entry.clone(),
                playing: entry.snapshot.status == "Playing",
                active: Some(name) == active,
            })
            .collect();
        if choices.len() < 2 {
            return Vec::new();
        }
        choices.sort_by(|a, b| a.identity.cmp(&b.identity).then(a.name.cmp(&b.name)));
        choices
    }

    /// Sends the active player, the player list and lyrics to the extension, when they changed.
    async fn publish(&mut self) {
        let (art_url, length_coming) = match self.active() {
            Some(entry) => (Some(entry.snapshot.art_url.clone()), entry.length_waiting),
            None => (None, false),
        };
        let art_path = art_url.map(|url| self.art_path(url)).unwrap_or_default();
        let (now_playing, active) = match self.active() {
            Some(entry) => {
                let s = &entry.snapshot;
                let now_playing = NowPlaying {
                    available: true,
                    identity: entry.identity.clone(),
                    desktop_entry: entry.desktop_entry.clone(),
                    status: s.status.clone(),
                    title: s.title.clone(),
                    artist: s.artist.clone(),
                    album: s.album.clone(),
                    art_path,
                    length_us: s.length_us,
                    position_us: s.position_us,
                    sampled_at_ms: s.sampled_at_ms,
                    rate: s.rate,
                    can_go_next: s.can_go_next,
                    can_go_previous: s.can_go_previous,
                    can_play_pause: s.can_play_pause,
                    can_seek: s.can_seek,
                };
                let art_url = s.art_url.clone();
                let active = (entry.player.clone(), s.track_id.clone());
                if now_playing.art_path.is_empty() && art::is_remote(&art_url) {
                    self.download_art(art_url);
                }
                (now_playing, Some(active))
            }
            None => (NowPlaying::default(), None),
        };
        let (now_playing, active, gap_ends) = self.gap.show(now_playing, active, Instant::now());
        if let Some(at) = gap_ends {
            self.check_gap_at(at);
        }

        let query = (!now_playing.title.is_empty()).then(|| TrackQuery {
            title: now_playing.title.clone(),
            artist: now_playing.artist.clone(),
            album: now_playing.album.clone(),
            length_s: now_playing.length_us as f64 / 1e6,
        });
        self.load_lyrics(query, length_coming).await;

        let choices = self.choices();
        let mut service = self.iface.get_mut().await;
        service.active = active;
        if service.players != choices {
            service.players = choices;
            if let Err(err) = service.players_changed(self.iface.signal_emitter()).await {
                warn!("could not publish the players: {err}");
            }
        }
        if !service.now_playing.continues_as(&now_playing) {
            service.now_playing = now_playing;
            if let Err(err) = service
                .now_playing_changed(self.iface.signal_emitter())
                .await
            {
                warn!("could not publish now playing: {err}");
            }
        }
    }

    /// Where the art at `url` is on disk, or empty while it is not (yet).
    fn art_path(&mut self, url: String) -> String {
        if let Some((found, path)) = &self.art_found
            && *found == url
        {
            return path.clone();
        }
        let Some(path) = art::local_path(&url) else {
            return String::new();
        };
        self.art_found = Some((url, path.clone()));
        path
    }

    /// Publishes again at `at`, when a gap between songs stops being bridged.
    fn check_gap_at(&mut self, at: Instant) {
        if self.gap_check == Some(at) {
            return;
        }
        self.gap_check = Some(at);
        let events = self.events.clone();
        tokio::spawn(async move {
            tokio::time::sleep_until(at).await;
            let _ = events.send(Event::GapOver);
        });
    }

    /// Downloads remote cover art in the background, once at a time and not again soon after a failure.
    fn download_art(&mut self, url: String) {
        if self.downloading_art.as_ref() == Some(&url)
            || self
                .failed_art
                .get(&url)
                .is_some_and(|failed| failed.elapsed() < ART_RETRY)
        {
            return;
        }
        self.downloading_art = Some(url.clone());
        let events = self.events.clone();
        tokio::task::spawn_blocking(move || {
            let result = art::download(&url);
            if let Err(err) = &result {
                debug!("could not download cover art {url}: {err}");
            }
            let ok = result.is_ok();
            let _ = events.send(Event::ArtDone { url, ok });
        });
    }

    /// Notes how a download went, so a dead link waits [`ART_RETRY`].
    fn art_done(&mut self, url: String, ok: bool) {
        if self.downloading_art.as_ref() == Some(&url) {
            self.downloading_art = None;
        }
        // Expired entries go here, which keeps the map from growing.
        self.failed_art
            .retain(|_, failed| failed.elapsed() < ART_RETRY);
        if ok {
            self.failed_art.remove(&url);
        } else {
            self.failed_art.insert(url, Instant::now());
        }
    }

    /// Starts loading lyrics when the track changes, or once what they wait on is in (see [`lyrics_step`]).
    async fn load_lyrics(&mut self, query: Option<TrackQuery>, length_coming: bool) {
        let key = query.as_ref().map(TrackQuery::key);
        if key == self.lyrics_query.as_ref().map(TrackQuery::key) && !self.lyrics_waiting {
            return;
        }
        self.lyrics_query = query.clone();
        self.lyrics_failures = 0;
        self.lyrics_waiting = false;
        let Some(query) = query else {
            // Forget any lookup still running for the previous track.
            self.lyrics_lookup += 1;
            self.set_lyrics(Lyrics::default()).await;
            return;
        };
        match lyrics_step(self.lyrics_online, &query, length_coming) {
            LyricsStep::Off => {
                self.lyrics_lookup += 1;
                self.set_lyrics(query.status(LyricsStatus::Off)).await;
            }
            LyricsStep::Wait => {
                self.lyrics_lookup += 1;
                self.lyrics_waiting = true;
                self.set_lyrics(query.status(LyricsStatus::Loading)).await;
            }
            LyricsStep::Fetch => {
                self.set_lyrics(query.status(LyricsStatus::Loading)).await;
                self.fetch_lyrics(query);
            }
        }
    }

    /// Looks lyrics up off the async threads, under a new lookup number.
    fn fetch_lyrics(&mut self, query: TrackQuery) {
        self.lyrics_lookup += 1;
        let lookup = self.lyrics_lookup;
        let events = self.events.clone();
        tokio::task::spawn_blocking(move || {
            let lyrics = lyrics::fetch(&query);
            let _ = events.send(Event::Lyrics { lookup, lyrics });
        });
    }

    /// Shows a lookup's result and schedules a retry if it failed (offline, LRCLIB busy).
    /// A track that simply has no lyrics is not retried until it next starts.
    async fn lyrics_done(&mut self, lookup: u64, lyrics: Lyrics) {
        if lookup != self.lyrics_lookup {
            return;
        }
        if lyrics.status.is_failure() {
            let delay = LYRICS_RETRY
                .saturating_mul(1 << self.lyrics_failures.min(8))
                .min(LYRICS_RETRY_MAX);
            self.lyrics_failures += 1;
            let events = self.events.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = events.send(Event::RetryLyrics { lookup });
            });
        }
        self.set_lyrics(lyrics).await;
    }

    /// Looks the lyrics up again, unless the track has changed since.
    fn retry_lyrics(&mut self, lookup: u64) {
        if lookup != self.lyrics_lookup {
            return;
        }
        if let Some(query) = self.lyrics_query.clone() {
            // The failed state stays on screen until the new result is in.
            self.fetch_lyrics(query);
        }
    }

    /// Publishes lyrics that still belong to the current track.
    async fn set_lyrics(&self, lyrics: Lyrics) {
        // Drop results for a track that is no longer playing.
        if lyrics.status != LyricsStatus::None
            && self.lyrics_query.as_ref().map(TrackQuery::key).as_ref() != Some(&lyrics.track)
        {
            return;
        }
        let mut service = self.iface.get_mut().await;
        if service.lyrics != lyrics {
            service.lyrics = lyrics;
            if let Err(err) = service.lyrics_changed(self.iface.signal_emitter()).await {
                warn!("could not publish lyrics: {err}");
            }
        }
    }
}

/// What a track's lyrics do next.
#[derive(Debug, PartialEq)]
enum LyricsStep {
    /// Turned off in Settings: the lyrics say so.
    Off,
    /// Not told yet whether to look, or the length is still coming: shown as looking.
    Wait,
    Fetch,
}

/// The step for `query`. `online` is `None` until the extension says; `length_coming` while its player
/// is read again for it: Firefox sends it late, and a lookup without it would run twice.
fn lyrics_step(online: Option<bool>, query: &TrackQuery, length_coming: bool) -> LyricsStep {
    match online {
        Some(false) => LyricsStep::Off,
        Some(true) if !length_coming || query.length_s > 0.0 => LyricsStep::Fetch,
        _ => LyricsStep::Wait,
    }
}

/// Reads what Keenu shows from a player's `GetAll` reply, with safe defaults for what is missing.
fn snapshot(all: &HashMap<String, OwnedValue>) -> Snapshot {
    let get = |key: &str| all.get(key).map(|value| &**value);
    let flag = |key: &str| get(key).and_then(boolean).unwrap_or(false);

    let metadata: HashMap<String, &Value> = match get("Metadata") {
        Some(Value::Dict(dict)) => dict
            .iter()
            .filter_map(|(key, value)| Some((string(key)?, unwrap_variant(value))))
            .collect(),
        _ => HashMap::new(),
    };
    let meta = |key: &str| metadata.get(key).copied();
    let position = get("Position").and_then(integer);

    Snapshot {
        status: get("PlaybackStatus")
            .and_then(string)
            .unwrap_or_else(|| "Stopped".into()),
        // A file with no title tag (a video in VLC) goes by its name, as the player shows it.
        title: meta("xesam:title")
            .and_then(string)
            .filter(|title| !title.trim().is_empty())
            .or_else(|| {
                meta("xesam:url")
                    .and_then(string)
                    .and_then(|url| file_title(&url))
            })
            .unwrap_or_default(),
        artist: meta("xesam:artist")
            .map(strings)
            .unwrap_or_default()
            .join(", "),
        album: meta("xesam:album").and_then(string).unwrap_or_default(),
        art_url: meta("mpris:artUrl").and_then(string).unwrap_or_default(),
        track_id: meta("mpris:trackid").and_then(|value| match value {
            Value::ObjectPath(path) => Some(path.clone().into()),
            // Some players send the id as a plain string.
            other => string(other).and_then(|s| OwnedObjectPath::try_from(s).ok()),
        }),
        length_us: meta("mpris:length").and_then(integer).unwrap_or(0),
        position_us: position.unwrap_or(0),
        position_known: position.is_some(),
        sampled_at_ms: now_ms(),
        rate: get("Rate").and_then(float).unwrap_or(1.0),
        can_go_next: flag("CanGoNext"),
        can_go_previous: flag("CanGoPrevious"),
        can_play_pause: flag("CanPause") || flag("CanPlay"),
        can_seek: flag("CanSeek"),
    }
}

/// The value inside any number of nested variants.
fn unwrap_variant<'a>(value: &'a Value<'a>) -> &'a Value<'a> {
    match value {
        Value::Value(inner) => unwrap_variant(inner),
        other => other,
    }
}

/// "Holiday clip" for `file:///…/Holiday%20clip.mp4`; `None` for a stream or a page.
fn file_title(url: &str) -> Option<String> {
    let path = crate::uri::to_path(url)?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    (!stem.trim().is_empty()).then_some(stem)
}

/// A string or object path, as players send either.
fn string(value: &Value<'_>) -> Option<String> {
    match unwrap_variant(value) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(path) => Some(path.to_string()),
        _ => None,
    }
}

/// A list of strings; a lone string counts as a list of one.
fn strings(value: &Value<'_>) -> Vec<String> {
    match unwrap_variant(value) {
        Value::Array(array) => array.iter().filter_map(string).collect(),
        other => string(other).into_iter().collect(),
    }
}

/// Any integer type a player might use, or a float cut down.
fn integer(value: &Value<'_>) -> Option<i64> {
    match unwrap_variant(value) {
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::F64(n) => Some(*n as i64),
        // Firefox has sent the length as a string of digits.
        Value::Str(text) => text
            .parse::<i64>()
            .ok()
            .or_else(|| text.parse::<f64>().ok().map(|n| n as i64)),
        _ => None,
    }
}

/// A browser's reported position this far behind the one already tracked is a stale read, not a seek.
const STALE_BEHIND_US: f64 = 1_500_000.0;

/// Browsers, whose media reads are stale at times; other players' reads are taken as they come.
const BROWSERS: &[&str] = &[
    "firefox",
    "zen",
    "chrom",
    "brave",
    "edge",
    "vivaldi",
    "opera",
    "epiphany",
    "librewolf",
    "floorp",
    "waterfox",
    "mullvad",
    "icecat",
];

/// Whether a player is a web browser, by its bus name, identity or desktop entry.
fn is_browser(names: &[&str]) -> bool {
    names.iter().any(|name| {
        let name = name.to_lowercase();
        BROWSERS.iter().any(|browser| name.contains(browser))
    })
}

/// The same media: the track id when both have one, and the title and artist. Firefox gives every
/// video one track id, and Chromium "NoTrack" when a page sets none: the title tells them apart.
fn same_track(previous: &Snapshot, next: &Snapshot) -> bool {
    let titled = |snapshot: &Snapshot| !snapshot.title.is_empty();
    let same_title = previous.title == next.title && previous.artist == next.artist;
    match (&previous.track_id, &next.track_id) {
        (Some(before), Some(after)) => {
            before == after && (!titled(previous) || !titled(next) || same_title)
        }
        _ => titled(previous) && same_title,
    }
}

/// Folds a fresh read (of the `same` track or not) into the trusted sample. `Seeked` always wins; a
/// player's read going back is a restart, even to 0; a `browser`'s 0 or lagging read is passed over.
fn reconcile(
    previous: &Snapshot,
    mut next: Snapshot,
    same: bool,
    seeked_us: Option<i64>,
    browser: bool,
) -> Snapshot {
    // Zen and Firefox drop `mpris:length` after a seek.
    if next.length_us <= 0 && previous.length_us > 0 && same {
        next.length_us = previous.length_us;
    }
    if next.status == "Playing" && next.rate <= 0.0 {
        next.rate = if previous.rate > 0.0 {
            previous.rate
        } else {
            1.0
        };
    }
    if let Some(position) = seeked_us {
        next.position_us = position.max(0);
        next.sampled_at_ms = now_ms();
        next.position_known = true;
        return next;
    }
    let running = |s: &Snapshot| s.status == "Playing" || s.status == "Paused";
    if !same || !running(previous) || !running(&next) {
        return next;
    }
    // Where it is believed to be at this read; it moves only while playing.
    let elapsed_ms = if previous.status == "Playing" {
        (next.sampled_at_ms - previous.sampled_at_ms).max(0) as f64
    } else {
        0.0
    };
    let predicted = previous.position_us as f64 + elapsed_ms * previous.rate.max(0.0) * 1000.0;
    let lagging = previous.status == "Playing"
        && next.status == "Playing"
        && predicted - next.position_us as f64 > STALE_BEHIND_US;
    // Unknown is no read. Zen and Firefox report 0 while playing and on pause or resume, and lag at times.
    let stale = !next.position_known || (browser && (next.position_us <= 0 || lagging));
    // Past its end, the track has started over (on repeat): the read is right.
    let over = previous.length_us > 0 && predicted >= previous.length_us as f64;
    if stale && !over {
        // Taken at this read's time, so a new rate counts only from now on.
        next.position_us = predicted as i64;
        next.position_known = previous.position_known;
    }
    next
}

/// A float, or an integer some players send for the rate.
fn float(value: &Value<'_>) -> Option<f64> {
    match unwrap_variant(value) {
        Value::F64(n) => Some(*n),
        other => integer(other).map(|n| n as f64),
    }
}

/// A plain boolean.
fn boolean(value: &Value<'_>) -> Option<bool> {
    match unwrap_variant(value) {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{LyricsStep, Snapshot, TrackQuery, file_title, lyrics_step, pick};

    /// Each as (name, status, title, when it last started playing).
    const PLAYERS: [(&str, &str, &str, u64); 3] = [
        ("spotify", "Paused", "Song", 3),
        ("firefox", "Playing", "Video", 5),
        ("mpv", "Playing", "Film", 4),
    ];

    fn pick_among(
        players: &[(&'static str, &str, &str, u64)],
        chosen: Option<&'static str>,
    ) -> Option<&'static str> {
        let players: Vec<_> = players
            .iter()
            .map(|&(name, status, title, started)| {
                let snapshot = Snapshot {
                    status: status.into(),
                    title: title.into(),
                    ..Snapshot::default()
                };
                (name, snapshot, started)
            })
            .collect();
        let players = players.iter().map(|(name, s, started)| (name, s, *started));
        pick(players, chosen.as_ref()).copied()
    }

    fn shot(title: &str, position_us: i64, sampled_at_ms: i64, length_us: i64) -> Snapshot {
        Snapshot {
            status: "Playing".into(),
            title: title.into(),
            position_us,
            sampled_at_ms,
            length_us,
            rate: 1.0,
            position_known: true,
            ..Snapshot::default()
        }
    }

    /// `reconcile` as `refresh` calls it.
    fn fold(
        previous: &Snapshot,
        next: Snapshot,
        seeked_us: Option<i64>,
        browser: bool,
    ) -> Snapshot {
        let same = super::same_track(previous, &next);
        super::reconcile(previous, next, same, seeked_us, browser)
    }

    #[test]
    fn a_real_position_replaces_the_sample() {
        let previous = shot("Song", 30_000_000, 1_000, 180_000_000);
        let next = shot("Song", 32_000_000, 3_000, 180_000_000);
        let got = fold(&previous, next, None, true);
        assert_eq!(got.position_us, 32_000_000);
        assert_eq!(got.sampled_at_ms, 3_000);
    }

    #[test]
    fn a_zero_while_playing_does_not_rewind() {
        let previous = shot("Song", 30_000_000, 1_000, 180_000_000);
        let mut next = shot("Song", 0, 5_000, 0);
        next.position_known = true;
        let got = fold(&previous, next, None, true);
        // Four seconds on from 30 s, taken at the new read's time.
        assert_eq!(got.position_us, 34_000_000);
        assert_eq!(got.sampled_at_ms, 5_000);
        assert_eq!(got.length_us, 180_000_000);
    }

    #[test]
    fn seeked_wins_over_a_following_zero() {
        let previous = shot("Song", 30_000_000, 1_000, 180_000_000);
        let next = shot("Song", 0, 5_000, 0);
        let got = fold(&previous, next, Some(90_000_000), true);
        assert_eq!(got.position_us, 90_000_000);
        assert!(got.sampled_at_ms >= 5_000);
        assert_eq!(got.length_us, 180_000_000);
    }

    #[test]
    fn a_players_restart_is_taken_as_it_comes() {
        // Repeat one: the same 20 s track starts over at 10 s, with no `Seeked`.
        let previous = shot("Song", 9_000_000, 1_000, 20_000_000);
        let next = shot("Song", 2_000_000, 4_000, 20_000_000);
        let got = fold(&previous, next.clone(), None, false);
        assert_eq!((got.position_us, got.sampled_at_ms), (2_000_000, 4_000));
        // A browser's read that falls behind is a stale one: 9 s and three more.
        let got = fold(&previous, next, None, true);
        assert_eq!((got.position_us, got.sampled_at_ms), (12_000_000, 4_000));
    }

    #[test]
    fn a_browsers_zero_on_pause_and_resume_keeps_the_place() {
        let playing = shot("Song", 30_000_000, 1_000, 180_000_000);
        let paused = Snapshot {
            status: "Paused".into(),
            ..shot("Song", 0, 5_000, 180_000_000)
        };
        let got = fold(&playing, paused.clone(), None, true);
        assert_eq!((got.position_us, got.sampled_at_ms), (34_000_000, 5_000));
        // Resumed a minute later: still where it was paused.
        let resumed = shot("Song", 0, 65_000, 180_000_000);
        let got = fold(&got, resumed.clone(), None, true);
        assert_eq!((got.position_us, got.sampled_at_ms), (34_000_000, 65_000));
        assert_eq!(got.status, "Playing");
        // A paused read with no position at all, from any player, is no read either.
        let unknown = Snapshot {
            position_known: false,
            ..paused
        };
        let got = fold(&playing, unknown, None, false);
        assert_eq!(got.position_us, 34_000_000);
        // A player's own 0 on resume is where it is.
        let paused_at = Snapshot {
            status: "Paused".into(),
            ..shot("Song", 34_000_000, 5_000, 180_000_000)
        };
        assert_eq!(fold(&paused_at, resumed, None, false).position_us, 0);
    }

    #[test]
    fn a_new_rate_counts_from_the_read_that_brings_it() {
        let previous = shot("Song", 30_000_000, 1_000, 180_000_000);
        let faster = Snapshot {
            rate: 2.0,
            ..shot("Song", 0, 5_000, 180_000_000)
        };
        let got = fold(&previous, faster, None, true);
        // The four seconds before it went at the old speed.
        assert_eq!((got.position_us, got.sampled_at_ms), (34_000_000, 5_000));
        assert_eq!(got.rate, 2.0);
    }

    #[test]
    fn a_browser_starting_over_from_stopped_is_at_0() {
        let stopped = Snapshot {
            status: "Stopped".into(),
            ..shot("Clip", 20_000_000, 1_000, 20_000_000)
        };
        let got = fold(&stopped, shot("Clip", 0, 3_000, 20_000_000), None, true);
        assert_eq!(got.position_us, 0);
    }

    #[test]
    fn past_its_end_a_track_has_started_over() {
        // A video on loop in a browser: the tracked position ran past the end.
        let previous = shot("Clip", 18_000_000, 1_000, 20_000_000);
        let next = shot("Clip", 3_000_000, 6_000, 20_000_000);
        let got = fold(&previous, next, None, true);
        assert_eq!(got.position_us, 3_000_000);
    }

    #[test]
    fn a_browsers_zero_is_passed_over_and_a_players_is_a_restart() {
        let previous = shot("Song", 30_000_000, 1_000, 180_000_000);
        let next = shot("Song", 0, 5_000, 180_000_000);
        let got = fold(&previous, next.clone(), None, true);
        assert_eq!(got.position_us, 34_000_000);
        // Previous, pressed in the player itself: back to the start.
        let got = fold(&previous, next, None, false);
        assert_eq!((got.position_us, got.sampled_at_ms), (0, 5_000));
    }

    #[test]
    fn another_video_under_firefoxs_one_track_id_is_another_track() {
        let id = zbus::zvariant::OwnedObjectPath::try_from("/org/mpris/MediaPlayer2/firefox").ok();
        let previous = Snapshot {
            track_id: id.clone(),
            ..shot("First video", 60_000_000, 1_000, 300_000_000)
        };
        let next = Snapshot {
            track_id: id,
            length_us: 0,
            ..shot("Second video", 0, 3_000, 0)
        };
        let got = fold(&previous, next, None, true);
        assert_eq!((got.position_us, got.length_us), (0, 0));
    }

    #[test]
    fn browsers_are_told_apart() {
        assert!(super::is_browser(&[
            "org.mpris.MediaPlayer2.firefox.instance_1_7",
            "Mozilla Firefox",
            "firefox"
        ]));
        assert!(super::is_browser(&[
            "org.mpris.MediaPlayer2.zen",
            "Zen",
            "zen"
        ]));
        assert!(super::is_browser(&[
            "org.mpris.MediaPlayer2.chromium.instance42",
            "Chrome",
            "google-chrome"
        ]));
        assert!(!super::is_browser(&[
            "org.mpris.MediaPlayer2.spotify",
            "Spotify",
            "spotify"
        ]));
        assert!(!super::is_browser(&[
            "org.mpris.MediaPlayer2.vlc",
            "VLC media player",
            "vlc"
        ]));
    }

    #[test]
    fn a_new_title_drops_the_old_length() {
        let previous = shot("Song", 30_000_000, 1_000, 180_000_000);
        let next = shot("Other", 0, 5_000, 0);
        let got = fold(&previous, next, None, true);
        assert_eq!(got.length_us, 0);
        assert_eq!(got.position_us, 0);
    }

    #[test]
    fn picks_the_chosen_player_else_the_one_playing_last() {
        assert_eq!(pick_among(&PLAYERS, None), Some("firefox"));
        assert_eq!(pick_among(&PLAYERS, Some("spotify")), Some("spotify"));
        // Picked, then gone: back to the rule.
        assert_eq!(pick_among(&PLAYERS, Some("vlc")), Some("firefox"));
    }

    #[test]
    fn a_paused_song_beats_a_player_stopped_since() {
        // VLC played a video after the song was paused, and the video ended.
        let after_video = [
            ("spotify", "Paused", "Song", 3),
            ("vlc", "Stopped", "Clip", 6),
        ];
        assert_eq!(pick_among(&after_video, None), Some("spotify"));
        // Playing with nothing to show: the paused song still shows.
        let untitled = [
            ("spotify", "Paused", "Song", 3),
            ("chrome", "Playing", "", 6),
        ];
        assert_eq!(pick_among(&untitled, None), Some("spotify"));
    }

    #[test]
    fn a_pick_left_empty_gives_way() {
        // Picked, then its file ended: `mpv --idle` has nothing to show, and Spotify plays.
        let idle = [("spotify", "Playing", "Song", 3), ("mpv", "Stopped", "", 4)];
        assert_eq!(pick_among(&idle, Some("mpv")), Some("spotify"));
        // Stopped with its song still in it, it has: the switch shows to go back.
        let stopped = [
            ("spotify", "Playing", "Song", 3),
            ("mpv", "Stopped", "Film", 4),
        ];
        assert_eq!(pick_among(&stopped, Some("mpv")), Some("mpv"));
        assert!(!super::shows(&Snapshot::default()));
    }

    #[test]
    fn lyrics_wait_for_the_setting_and_a_length_on_its_way() {
        let query = |length_s| TrackQuery {
            title: "Song".into(),
            artist: "Lumen Coast".into(),
            album: String::new(),
            length_s,
        };
        // Until the extension says, nothing is looked up.
        assert_eq!(lyrics_step(None, &query(216.0), false), LyricsStep::Wait);
        assert_eq!(
            lyrics_step(Some(false), &query(216.0), false),
            LyricsStep::Off
        );
        assert_eq!(
            lyrics_step(Some(true), &query(216.0), true),
            LyricsStep::Fetch
        );
        // Firefox's length comes a moment late: one lookup, with it.
        assert_eq!(lyrics_step(Some(true), &query(0.0), true), LyricsStep::Wait);
        // Never coming (a stream): looked up without.
        assert_eq!(
            lyrics_step(Some(true), &query(0.0), false),
            LyricsStep::Fetch
        );
    }

    #[test]
    fn a_file_with_no_title_goes_by_its_name() {
        assert_eq!(
            file_title("file:///home/user/Videos/Holiday-2026.mp4").as_deref(),
            Some("Holiday-2026")
        );
        assert_eq!(
            file_title("file:///home/user/My%20Videos/My%20clip.final.mkv").as_deref(),
            Some("My clip.final")
        );
        assert_eq!(file_title("https://www.youtube.com/watch?v=x"), None);
    }
}
