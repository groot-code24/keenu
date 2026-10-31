// SPDX-License-Identifier: GPL-3.0-or-later

//! The application: what is known, what the island should show, and what each click does.
//!
//! A host calls [`App::pump`] when woken, [`App::tick`] about once a second and [`App::act`] on a click. Slow
//! work happens on threads that report back through a channel, so the host's UI thread never waits on a
//! network or a device.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Local, TimeZone};
use keenu_core::clipboard::{Clip, ClipboardHistory};
use keenu_core::file_shelf::{self, AddError, FileShelf};
use keenu_core::json_file;
use keenu_core::lrc::LyricLine;
use keenu_core::lyrics::{self, Lyrics};
use keenu_core::notifications::{Notification, Priority, Queue};
use keenu_core::release::Channel;
use keenu_core::reminders::Reminders;
use keenu_core::settings::{CalendarAccount, Settings, SpeechEngine, UpdateChannel};
use keenu_core::usage_log::UsageLog;
use keenu_integrations::agents::protocol::{self, Answer, ApprovalRequest};
use keenu_integrations::agents::{self, Agent};
use keenu_integrations::calendar::caldav::CalDav;
use keenu_integrations::calendar::engine::{self, AccountCache, AgendaItem, PendingEdit};
use keenu_integrations::calendar::google::{self, Google};
use keenu_integrations::calendar::graph::{self, Graph};
use keenu_integrations::calendar::{CalendarProvider, EventDraft};
use keenu_integrations::oauth::{self, TokenSource};
use keenu_integrations::updater::{self, Update, UpdateConfig};
use keenu_integrations::{lyrics as online_lyrics, voice as remote_voice};
use keenu_platform::{
    AudioDevice, Availability, ClipboardContent, ClipboardProvider, FileShelfProvider,
    MediaCommand, MediaProvider, MediaSession, MonitorProvider, NativeNotification,
    NotificationProvider, PlaybackState, Recording, UpdateProvider, UsageProvider, VoiceProvider,
    WindowLifecycleProvider,
};
use keenu_security::credentials::CredentialStore;
use keenu_security::replay::ReplayGuard;
use keenu_ui::scene::Scene;
use keenu_ui::view::{
    self, AccountRow, Action, AgendaRow, AgentRow, ApprovalView, ClipRow, Field, InputView,
    LyricsView, MediaView, OnboardingView, Page, Setting, SettingsView, ShelfRow, SwitchRow,
    UsageRow, View, VoiceView,
};

use crate::data::{self, Paths, load_or_default, load_settings, now_ms};

const SYNC_EVERY_MS: i64 = 15 * 60 * 1000;
const UPDATE_EVERY_MS: i64 = 24 * 60 * 60 * 1000;
const MAINTAIN_EVERY_MS: i64 = 60 * 60 * 1000;
/// Time without input after which the app in front is no longer being used.
const USAGE_IDLE_MS: u64 = 5 * 60 * 1000;
/// Lines of lyrics shown around the current one.
const LYRICS_BEFORE: usize = 1;
const LYRICS_AFTER: usize = 3;
/// How far ahead the calendar page looks.
const AGENDA_DAYS: i64 = 14;
const CREDENTIAL_VOICE: &str = "voice/api-key";
/// How long a request must have been on screen before Allow counts. It appears on its own, wherever the
/// pointer happens to be: a click meant for something else must not approve a command nobody has read.
const ALLOW_ARMS_AFTER_MS: i64 = 700;

/// The operating system, as this crate sees it.
pub struct Platform {
    pub media: Arc<dyn MediaProvider>,
    pub notifier: Arc<dyn NotificationProvider>,
    pub clipboard: Arc<dyn ClipboardProvider>,
    pub shelf: Arc<dyn FileShelfProvider>,
    pub voice: Arc<dyn VoiceProvider>,
    pub monitors: Arc<dyn MonitorProvider>,
    pub usage: Arc<dyn UsageProvider>,
    pub lifecycle: Arc<dyn WindowLifecycleProvider>,
    pub updates: Arc<dyn UpdateProvider>,
    pub credentials: Arc<dyn CredentialStore>,
    /// Opens a web page in the default browser, for signing in.
    pub open_url: fn(&str) -> Result<(), String>,
    /// Plays WAV audio from a speech API.
    pub play_wav: fn(&[u8]) -> Result<(), String>,
}

/// Lets a worker thread tell the host's UI thread there is news.
pub trait Waker: Send + Sync + 'static {
    fn wake(&self);
}

/// What the host must do after an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Close down: save and leave the message loop.
    Quit,
}

/// Keys the island handles itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Tab,
    ShiftTab,
    Backspace,
    Char(char),
}

/// A permission waiting for the user.
struct PendingApproval {
    request: ApprovalRequest,
    reply: Box<dyn FnOnce(Answer) + Send>,
    abandoned: Box<dyn Fn() -> bool + Send>,
    deadline_ms: i64,
}

enum Event {
    Media(Result<Vec<MediaSession>, String>),
    Artwork(String, Vec<u8>),
    Lyrics(String, Option<Vec<LyricLine>>, String),
    UsageStretch(String, i64, i64),
    Approval(PendingApproval),
    Status(String),
    Notify(Notification),
    CalendarDone(String, Box<AccountCache>, Result<(), String>),
    AccountAdded(CalendarAccount),
    AccountFailed(String),
    VoiceHeard(String),
    VoiceAnswer(String),
    VoiceFailed(String),
    VoiceDone,
    Thumbnail(String, Vec<u8>),
    UpdateFound(Option<Update>, String),
    UpdateProgress(String),
    UpdateReady(PathBuf, String),
    UpdateFailed(String),
}

/// What other threads may do to the app.
#[derive(Clone)]
pub struct AppHandle {
    sender: Sender<Event>,
    waker: Arc<dyn Waker>,
    timeout_secs: u32,
}

impl AppHandle {
    fn send(&self, event: Event) {
        let _ = self.sender.send(event);
        self.waker.wake();
    }

    /// Puts a permission request before the user. `reply` gets the answer once; `abandoned` says the
    /// asker has gone away.
    pub fn approval(
        &self,
        request: ApprovalRequest,
        reply: Box<dyn FnOnce(Answer) + Send>,
        abandoned: Box<dyn Fn() -> bool + Send>,
    ) {
        self.send(Event::Approval(PendingApproval {
            request,
            reply,
            abandoned,
            deadline_ms: now_ms() + i64::from(self.timeout_secs) * 1000,
        }));
    }

    /// A notification from another thread.
    pub fn notify(&self, key: &str, title: &str, body: &str) {
        self.send(Event::Notify(Notification {
            key: key.to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            priority: Priority::Normal,
            actions: Vec::new(),
        }));
    }
}

pub struct App {
    platform: Arc<Platform>,
    paths: Paths,
    pub settings: Settings,
    view: View,
    sender: Sender<Event>,
    receiver: Receiver<Event>,
    waker: Arc<dyn Waker>,

    sessions: Vec<MediaSession>,
    chosen_session: Option<String>,
    track: String,
    lyrics: Option<Lyrics>,
    lyrics_note: String,
    images: HashMap<String, Arc<Vec<u8>>>,

    clips: ClipboardHistory,
    shelf: FileShelf,
    usage: UsageLog,
    reminders: Reminders,
    notifications: Queue,
    notifications_unavailable: bool,

    approvals: VecDeque<PendingApproval>,
    replay: ReplayGuard,
    /// Which request is on screen, and since when.
    shown_approval: Option<(String, i64)>,

    caches: BTreeMap<String, AccountCache>,
    syncing: Vec<String>,
    /// CalDAV's answers so far, while adding an account.
    caldav_draft: (String, String),
    last_sync_ms: i64,
    agenda_items: Vec<AgendaItem>,

    devices: Vec<AudioDevice>,
    voice_error: String,
    voice_busy: bool,
    voice_heard: String,
    voice_answer: String,

    update_status: String,
    update: Option<Update>,
    update_file: Option<(PathBuf, String)>,
    last_update_check_ms: i64,

    last_maintained_ms: i64,
    onboarding_step: Option<usize>,

    recording_usage: Arc<AtomicBool>,
    session_active: Arc<AtomicBool>,
    media_fast: Arc<AtomicBool>,
    stopping: Arc<AtomicBool>,
}

/// Local midnight today.
pub fn start_of_today_ms() -> i64 {
    let today = Local::now().date_naive();
    today
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| Local.from_local_datetime(&midnight).earliest())
        .map_or_else(now_ms, |time| time.timestamp_millis())
}

/// A history item as what goes back on the clipboard.
pub fn clip_to_content(clip: &Clip, paths: &Paths) -> Option<ClipboardContent> {
    Some(match clip {
        Clip::Text { text } | Clip::Link { text } => ClipboardContent::Text(text.clone()),
        Clip::Rich { text, rtf } => ClipboardContent::Rich {
            text: text.clone(),
            rtf: rtf.clone(),
        },
        Clip::Image {
            hash,
            width,
            height,
        } => ClipboardContent::Image {
            png: std::fs::read(paths.clip_images().join(format!("{hash}.png"))).ok()?,
            width: *width,
            height: *height,
        },
        Clip::Files { uris } => {
            ClipboardContent::Files(uris.iter().filter_map(|uri| uri_to_path(uri)).collect())
        }
    })
}

/// `C:\a b.txt` or `/a b.txt` as a `file://` URI.
pub fn path_to_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b':' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// The reverse of [`path_to_uri`].
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let decoded = keenu_integrations::http::decode_component(&rest.replace('+', "%2B"));
    if cfg!(windows) {
        Some(PathBuf::from(
            decoded.trim_start_matches('/').replace('/', "\\"),
        ))
    } else {
        Some(PathBuf::from(decoded))
    }
}

impl App {
    /// Sets up from what is on disk, bringing over what an earlier version kept. Starts no threads.
    pub fn new(platform: Platform, paths: Paths, waker: Arc<dyn Waker>) -> Self {
        let notes = data::migrate_legacy(&paths);
        let settings = load_settings(&paths);
        let (sender, receiver) = channel();
        let mut caches = BTreeMap::new();
        caches.insert(
            "local".to_owned(),
            load_or_default::<AccountCache>(&paths.local_calendar()),
        );
        for account in &settings.calendar.accounts {
            caches.insert(
                account.id.clone(),
                load_or_default(&paths.calendar_cache(&account.id)),
            );
        }
        let mut app = Self {
            platform: Arc::new(platform),
            view: View::default(),
            sender,
            receiver,
            waker,
            sessions: Vec::new(),
            chosen_session: None,
            track: String::new(),
            lyrics: None,
            lyrics_note: String::new(),
            images: HashMap::new(),
            clips: load_or_default(&paths.clipboard()),
            shelf: load_or_default(&paths.shelf()),
            usage: load_or_default(&paths.usage()),
            reminders: load_or_default(&paths.reminders()),
            notifications: Queue::default(),
            notifications_unavailable: false,
            approvals: VecDeque::new(),
            replay: ReplayGuard::default(),
            shown_approval: None,
            caches,
            syncing: Vec::new(),
            caldav_draft: (String::new(), String::new()),
            last_sync_ms: 0,
            agenda_items: Vec::new(),
            devices: Vec::new(),
            voice_error: String::new(),
            voice_busy: false,
            voice_heard: String::new(),
            voice_answer: String::new(),
            update_status: String::new(),
            update: None,
            update_file: None,
            last_update_check_ms: 0,
            last_maintained_ms: 0,
            onboarding_step: if settings.onboarded { None } else { Some(0) },
            recording_usage: Arc::new(AtomicBool::new(settings.usage.recording())),
            session_active: Arc::new(AtomicBool::new(true)),
            media_fast: Arc::new(AtomicBool::new(false)),
            stopping: Arc::new(AtomicBool::new(false)),
            paths,
            settings,
        };
        app.devices = app.platform.voice.devices().unwrap_or_default();
        if let Some(note) = notes.first() {
            app.view.status = note.clone();
        }
        app.shelf.refresh_missing();
        app.maintain(now_ms());
        app.refresh_view();
        app
    }

    pub fn handle(&self) -> AppHandle {
        AppHandle {
            sender: self.sender.clone(),
            waker: self.waker.clone(),
            timeout_secs: self.settings.agents.answer_timeout_secs,
        }
    }

    /// Starts the threads that watch media and, while it is on, the app in front.
    pub fn start_workers(&self) {
        self.start_media_watch();
        self.start_usage_watch();
    }

    fn start_media_watch(&self) {
        let (media, handle, fast, stopping) = (
            self.platform.media.clone(),
            self.handle(),
            self.media_fast.clone(),
            self.stopping.clone(),
        );
        let _ = thread::Builder::new()
            .name("keenu-media".into())
            .spawn(move || {
                while !stopping.load(Ordering::Relaxed) {
                    handle.send(Event::Media(media.sessions()));
                    // Only as often as anyone can see: quickly while open, slowly while closed with nothing playing.
                    let wait = if fast.load(Ordering::Relaxed) {
                        500
                    } else {
                        2000
                    };
                    thread::sleep(Duration::from_millis(wait));
                }
            });
    }

    fn start_usage_watch(&self) {
        let (provider, handle, recording, active, stopping) = (
            self.platform.usage.clone(),
            self.handle(),
            self.recording_usage.clone(),
            self.session_active.clone(),
            self.stopping.clone(),
        );
        let _ = thread::Builder::new()
            .name("keenu-usage".into())
            .spawn(move || {
                let mut current: Option<(String, i64)> = None;
                while !stopping.load(Ordering::Relaxed) {
                    let now = now_ms();
                    let using = recording.load(Ordering::Relaxed)
                        && active.load(Ordering::Relaxed)
                        && provider.idle_ms().is_none_or(|idle| idle < USAGE_IDLE_MS);
                    // Asked only while recording: off means the system is not even looked at.
                    let front = if using {
                        provider.foreground_app()
                    } else {
                        None
                    };
                    let same =
                        matches!((&current, &front), (Some((app, _)), Some(next)) if app == next);
                    if !same {
                        if let Some((app, start)) = current.take() {
                            handle.send(Event::UsageStretch(app, start, now));
                        }
                        current = front.map(|app| (app, now));
                    }
                    thread::sleep(Duration::from_secs(2));
                }
                if let Some((app, start)) = current {
                    handle.send(Event::UsageStretch(app, start, now_ms()));
                }
            });
    }

    /// Stops the threads at their next look and saves everything.
    pub fn shut_down(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        self.pump();
        self.save_all();
        // A hook left waiting gets an answer that makes its agent ask in the terminal.
        while let Some(pending) = self.approvals.pop_front() {
            (pending.reply)(Answer::NoDecision);
        }
    }

    fn save_all(&self) {
        self.save_settings();
        self.save_file(&self.paths.clipboard(), &self.clips);
        self.save_file(&self.paths.shelf(), &self.shelf);
        self.save_file(&self.paths.usage(), &self.usage);
        self.save_file(&self.paths.reminders(), &self.reminders);
    }

    fn save_file<T: serde::Serialize>(&self, path: &Path, value: &T) {
        if let Err(err) = json_file::save(path, value) {
            log::warn!("could not save {}: {err}", path.display());
        }
    }

    fn save_settings(&self) {
        self.save_file(&self.paths.settings(), &self.settings);
    }

    pub fn view(&self) -> &View {
        &self.view
    }

    pub fn scene(&self) -> Scene<Action> {
        view::build(&self.view)
    }

    /// Image bytes by the keys the scene names.
    pub fn images(&self) -> &HashMap<String, Arc<Vec<u8>>> {
        &self.images
    }

    /// Whether the island must stay open, whatever the pointer does.
    pub fn holds_open(&self) -> bool {
        self.view.approval.is_some() || self.view.onboarding.is_some() || self.view.input.is_some()
    }

    /// The display name chosen, empty for the primary.
    pub fn monitor_name(&self) -> &str {
        &self.settings.window.monitor
    }

    /// Whether it should step aside when another app is fullscreen, right now.
    pub fn may_hide_for_fullscreen(&self) -> bool {
        self.settings.window.hide_in_fullscreen && !self.holds_open()
    }

    pub fn set_expanded(&mut self, expanded: bool) {
        if self.view.expanded != expanded {
            self.view.expanded = expanded;
            self.view.focus = None;
            self.media_fast.store(expanded, Ordering::Relaxed);
            self.refresh_view();
        }
    }

    /// The screen locked or the computer slept: the app in front is not being used.
    pub fn set_session_active(&self, active: bool) {
        self.session_active.store(active, Ordering::Relaxed);
    }

    /// Handles what workers have reported. True if anything changed.
    pub fn pump(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.receiver.try_recv() {
            self.on_event(event);
            changed = true;
        }
        if changed {
            self.refresh_view();
        }
        changed
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Media(Ok(sessions)) => self.on_sessions(sessions),
            Event::Media(Err(why)) => {
                self.sessions.clear();
                self.view.media_note = why;
            }
            Event::Artwork(key, bytes) => {
                // One cover in memory at a time, plus what the shelf holds.
                self.images.retain(|known, _| !known.starts_with("art:"));
                self.images.insert(key, Arc::new(bytes));
            }
            Event::Lyrics(track, lines, note) => {
                if track == self.track {
                    self.lyrics = lines.map(Lyrics::from_lines);
                    self.lyrics_note = note;
                }
            }
            Event::UsageStretch(app, start, stop) => {
                if self.settings.usage.enabled && self.usage.record(&app, start, stop) {
                    self.save_file(&self.paths.usage(), &self.usage);
                }
            }
            Event::Approval(pending) => self.on_approval(pending),
            Event::Status(text) => self.view.status = text,
            Event::Notify(notification) => {
                self.notifications.push(notification, now_ms());
            }
            Event::CalendarDone(id, cache, result) => {
                self.syncing.retain(|known| known != &id);
                self.save_file(&self.paths.calendar_cache(&id), &*cache);
                let cache = *cache;
                if !cache.conflicts.is_empty() {
                    self.view.status = format!(
                        "\"{}\" was changed elsewhere first, so your edit was not sent.",
                        cache.conflicts[cache.conflicts.len() - 1].title
                    );
                }
                self.caches.insert(id, cache);
                if let Err(why) = result {
                    self.view.calendar_note = why;
                } else {
                    self.view.calendar_note.clear();
                }
            }
            Event::AccountAdded(account) => {
                self.view.status = format!("Connected {}.", account.label);
                let id = account.id.clone();
                self.settings.calendar.accounts.push(account);
                self.save_settings();
                self.caches.entry(id.clone()).or_default();
                self.sync_account(&id);
            }
            Event::AccountFailed(why) => self.view.status = why,
            Event::VoiceHeard(text) => self.voice_heard = text,
            Event::VoiceAnswer(text) => self.voice_answer = text,
            Event::VoiceFailed(why) => {
                self.voice_error = why;
                self.voice_busy = false;
            }
            Event::VoiceDone => self.voice_busy = false,
            Event::Thumbnail(key, bytes) => {
                self.images.insert(key, Arc::new(bytes));
            }
            Event::UpdateFound(update, status) => {
                self.update_status = status;
                let found = update.is_some();
                self.update = update;
                if found && self.settings.updates.auto_install {
                    self.install_update();
                }
            }
            Event::UpdateProgress(status) => self.update_status = status,
            Event::UpdateReady(file, version) => {
                self.update_status = format!("Version {version} is ready to install.");
                self.update_file = Some((file, version));
            }
            Event::UpdateFailed(why) => self.update_status = why,
        }
    }

    fn on_sessions(&mut self, sessions: Vec<MediaSession>) {
        self.view.media_note.clear();
        self.sessions = sessions;
        let Some(session) = self.current_session().cloned() else {
            self.track.clear();
            self.lyrics = None;
            self.lyrics_note.clear();
            return;
        };
        let track = format!(
            "{}\u{1f}{}\u{1f}{}",
            session.id, session.artist, session.title
        );
        if track != self.track {
            self.track = track.clone();
            self.lyrics = None;
            self.lyrics_note.clear();
            self.load_artwork(&session, &track);
            self.load_lyrics(&session, track);
        }
    }

    fn current_session(&self) -> Option<&MediaSession> {
        let chosen = self.chosen_session.as_ref();
        chosen
            .and_then(|id| self.sessions.iter().find(|session| &session.id == id))
            .or_else(|| self.sessions.first())
    }

    fn artwork_key(track: &str) -> String {
        format!(
            "art:{}",
            keenu_core::clipboard::content_hash(track.as_bytes())
        )
    }

    fn load_artwork(&self, session: &MediaSession, track: &str) {
        if !session.has_artwork {
            return;
        }
        let (media, handle, id, key) = (
            self.platform.media.clone(),
            self.handle(),
            session.id.clone(),
            Self::artwork_key(track),
        );
        thread::spawn(move || {
            if let Ok(Some(bytes)) = media.artwork(&id) {
                handle.send(Event::Artwork(key, bytes));
            }
        });
    }

    fn lyrics_folder(&self) -> PathBuf {
        if self.settings.lyrics.folder.is_empty() {
            keenu_core::paths::data_dir().join("lyrics")
        } else {
            PathBuf::from(&self.settings.lyrics.folder)
        }
    }

    /// A local file wins; online only if the user turned that on.
    fn load_lyrics(&self, session: &MediaSession, track: String) {
        let folder = self.lyrics_folder();
        let online = self.settings.lyrics.online;
        let handle = self.handle();
        let (title, artist, album, duration) = (
            session.title.clone(),
            session.artist.clone(),
            session.album.clone(),
            session.duration_ms,
        );
        thread::spawn(move || {
            let local = lyrics::file_name(&artist, &title)
                .and_then(|name| std::fs::read(folder.join(name)).ok());
            if let Some(bytes) = local {
                match Lyrics::from_lrc_bytes(&bytes) {
                    Some(found) => handle.send(Event::Lyrics(
                        track,
                        Some(found.lines().to_vec()),
                        String::new(),
                    )),
                    None => handle.send(Event::Lyrics(
                        track,
                        None,
                        "This song's lyrics file has no timed lines.".into(),
                    )),
                }
                return;
            }
            if !online {
                handle.send(Event::Lyrics(track, None, String::new()));
                return;
            }
            let found = online_lyrics::fetch(&online_lyrics::TrackQuery {
                title,
                artist,
                album,
                length_s: duration as f64 / 1000.0,
            });
            if found.lines.is_empty() {
                handle.send(Event::Lyrics(track, None, "No synced lyrics found.".into()));
            } else {
                handle.send(Event::Lyrics(track, Some(found.lines), String::new()));
            }
        });
    }

    fn on_approval(&mut self, pending: PendingApproval) {
        if !self.replay.first_time(&pending.request.id) {
            (pending.reply)(Answer::NoDecision);
            return;
        }
        // Cleaned again here: the hook did, but what crossed the pipe is only as good as whoever sent it.
        let mut pending = pending;
        let request = &mut pending.request;
        request.detail = protocol::clean(&request.detail, 2000).0;
        request.cwd = protocol::clean(&request.cwd, 512).0;
        request.project = protocol::clean(&request.project, 512).0;
        request.tool = protocol::clean(&request.tool, 512).0;
        for permission in &mut request.suggested_permissions {
            *permission = protocol::clean(permission, 512).0;
        }
        self.approvals.push_back(pending);
    }

    /// About once a second: the clock, timeouts, and what falls due.
    pub fn tick(&mut self) {
        let now = now_ms();
        self.pump();
        self.expire_approvals(now);
        self.deliver_due_reminders(now);
        self.deliver_notifications();
        if now - self.last_maintained_ms >= MAINTAIN_EVERY_MS {
            self.maintain(now);
        }
        if now - self.last_sync_ms >= SYNC_EVERY_MS && !self.settings.calendar.accounts.is_empty() {
            self.sync_all();
        }
        if self.settings.updates.check && now - self.last_update_check_ms >= UPDATE_EVERY_MS {
            self.check_updates();
        }
        self.refresh_view();
    }

    fn expire_approvals(&mut self, now: i64) {
        let mut kept = VecDeque::new();
        while let Some(pending) = self.approvals.pop_front() {
            if (pending.abandoned)() {
                // Answered in its terminal: nothing left to ask.
                (pending.reply)(Answer::NoDecision);
            } else if now >= pending.deadline_ms {
                // No answer in time is not an allow: back to the agent's own prompt.
                (pending.reply)(Answer::NoDecision);
            } else {
                kept.push_back(pending);
            }
        }
        self.approvals = kept;
    }

    fn deliver_due_reminders(&mut self, now: i64) {
        let due: Vec<_> = self
            .reminders
            .items()
            .iter()
            .filter(|reminder| reminder.due_ms <= now)
            .cloned()
            .collect();
        if due.is_empty() {
            return;
        }
        for reminder in due {
            self.notifications.push(
                Notification {
                    key: format!("reminder:{}", reminder.id),
                    title: "Reminder".into(),
                    body: reminder.text.clone(),
                    priority: Priority::High,
                    actions: Vec::new(),
                },
                now,
            );
            self.reminders.dismiss(reminder.id);
        }
        self.save_file(&self.paths.reminders(), &self.reminders);
    }

    fn deliver_notifications(&mut self) {
        if !self.settings.notifications.enabled {
            while self.notifications.take_next().is_some() {}
            return;
        }
        while let Some((notification, attempts)) = self.notifications.take_next() {
            let native = NativeNotification {
                tag: notification.key.clone(),
                title: notification.title.clone(),
                body: notification.body.clone(),
                actions: notification
                    .actions
                    .iter()
                    .map(|action| (action.id.clone(), action.label.clone()))
                    .collect(),
            };
            if let Err(why) = self.platform.notifier.show(&native) {
                log::warn!("{why}");
                self.notifications_unavailable = true;
                // Back on the queue for the next tick, up to its limit.
                self.notifications.failed(notification, attempts);
                break;
            }
        }
    }

    /// Retention: what the user said to keep no longer is.
    fn maintain(&mut self, now: i64) {
        self.last_maintained_ms = now;
        let day = 86_400_000;
        let clip_cutoff = now - i64::from(self.settings.clipboard.retention_days) * day;
        let unused = self
            .clips
            .retain(|item| item.pinned || item.at_ms >= clip_cutoff);
        self.remove_clip_images(&unused);
        if !unused.is_empty() {
            self.save_file(&self.paths.clipboard(), &self.clips);
        }
        if self
            .usage
            .prune(now - i64::from(self.settings.usage.retention_days) * day)
        {
            self.save_file(&self.paths.usage(), &self.usage);
        }
        if self.shelf.refresh_missing() {
            self.save_file(&self.paths.shelf(), &self.shelf);
        }
    }

    fn remove_clip_images(&self, hashes: &[String]) {
        for hash in hashes {
            let _ = std::fs::remove_file(self.paths.clip_images().join(format!("{hash}.png")));
        }
    }

    /// The clipboard changed. Reads it only while the user has history on and running.
    pub fn clipboard_changed(&mut self) {
        if !self.settings.clipboard.recording() {
            return;
        }
        let content = match self.platform.clipboard.read() {
            Ok(Some(content)) => content,
            // Empty, or marked by the app that copied it as not for history.
            Ok(None) => return,
            Err(why) => {
                log::debug!("clipboard: {why}");
                return;
            }
        };
        let clip = match content {
            ClipboardContent::Text(text) => Clip::from_text(&text).ok(),
            ClipboardContent::Rich { text, rtf } => Clip::from_rich(&text, &rtf).ok(),
            ClipboardContent::Files(files) => {
                Clip::from_uris(files.iter().map(|file| path_to_uri(file)).collect()).ok()
            }
            ClipboardContent::Image { png, width, height } => {
                if !self.settings.clipboard.capture_images {
                    return;
                }
                let hash = keenu_core::clipboard::content_hash(&png);
                let file = self.paths.clip_images().join(format!("{hash}.png"));
                if let Err(err) = json_file::write_private(&file, &png) {
                    log::warn!("could not keep a copied image: {err}");
                    return;
                }
                Some(Clip::Image {
                    hash,
                    width,
                    height,
                })
            }
        };
        let Some(clip) = clip else {
            return;
        };
        let added = self
            .clips
            .add_keeping(clip, "", now_ms(), self.settings.clipboard.max_items);
        self.remove_clip_images(&added.unreferenced);
        self.save_file(&self.paths.clipboard(), &self.clips);
        self.refresh_view();
    }

    /// Files dropped on the island go on the shelf, by reference.
    pub fn files_dropped(&mut self, files: Vec<PathBuf>) {
        let now = now_ms();
        for file in files {
            match self.shelf.add(&file, now) {
                Ok(()) | Err(AddError::Duplicate) => self.fetch_thumbnail(&file),
                Err(AddError::Unsafe) => {
                    self.view.status =
                        format!("{} was not added: not a plain file path.", file.display());
                }
                Err(AddError::NotFound) => {
                    self.view.status = format!("{} was not found.", file.display());
                }
            }
        }
        self.save_file(&self.paths.shelf(), &self.shelf);
        self.view.page = Page::Shelf;
        self.view.expanded = true;
        self.refresh_view();
    }

    fn fetch_thumbnail(&self, file: &Path) {
        let (shelf, handle, file) = (
            self.platform.shelf.clone(),
            self.handle(),
            file.to_path_buf(),
        );
        thread::spawn(move || {
            if let Ok(Some(png)) = shelf.thumbnail(&file, 96) {
                handle.send(Event::Thumbnail(thumbnail_key(&file), png));
            }
        });
    }

    fn sync_all(&mut self) {
        self.last_sync_ms = now_ms();
        let ids: Vec<String> = self
            .settings
            .calendar
            .accounts
            .iter()
            .map(|account| account.id.clone())
            .collect();
        for id in ids {
            self.sync_account(&id);
        }
    }

    fn provider_for(&self, account: &CalendarAccount) -> Result<Box<dyn CalendarProvider>, String> {
        let key = credential_key(&account.id);
        match account.provider.as_str() {
            "caldav" => {
                let password = self
                    .platform
                    .credentials
                    .get(&key)
                    .map_err(|err| err.to_string())?
                    .ok_or("The saved password is gone. Add the account again.")?;
                Ok(Box::new(
                    CalDav::new(&account.server, &account.username, &password)
                        .map_err(|err| err.to_string())?,
                ))
            }
            "google" => {
                let provider = google::provider().ok_or("This build has no Google client ID.")?;
                Ok(Box::new(Google::new(TokenSource::new(
                    provider,
                    self.platform.credentials.clone(),
                    key,
                ))))
            }
            "microsoft" => {
                let provider = graph::provider().ok_or("This build has no Microsoft client ID.")?;
                Ok(Box::new(Graph::new(TokenSource::new(
                    provider,
                    self.platform.credentials.clone(),
                    key,
                ))))
            }
            other => Err(format!("Unknown calendar provider {other}.")),
        }
    }

    fn sync_account(&mut self, id: &str) {
        if self.syncing.iter().any(|known| known == id) {
            return;
        }
        let Some(account) = self
            .settings
            .calendar
            .accounts
            .iter()
            .find(|account| account.id == id)
            .cloned()
        else {
            return;
        };
        let provider = match self.provider_for(&account) {
            Ok(provider) => provider,
            Err(why) => {
                self.view.calendar_note = why;
                return;
            }
        };
        self.syncing.push(id.to_owned());
        let mut cache = self.caches.get(id).cloned().unwrap_or_default();
        let (handle, id) = (self.handle(), id.to_owned());
        thread::spawn(move || {
            let result = engine::sync(&*provider, &mut cache).map_err(|err| err.to_string());
            handle.send(Event::CalendarDone(id, Box::new(cache), result));
        });
    }

    fn add_caldav_account(&mut self, password: String) {
        let (server, username) = std::mem::take(&mut self.caldav_draft);
        let (handle, credentials) = (self.handle(), self.platform.credentials.clone());
        self.view.status = "Checking the server...".into();
        thread::spawn(move || {
            let check =
                CalDav::new(&server, &username, &password).and_then(|account| account.calendars());
            match check {
                Ok(_) => {
                    let Ok(random) = keenu_security::random_hex(8) else {
                        handle.send(Event::AccountFailed(
                            "No secure random numbers on this system.".into(),
                        ));
                        return;
                    };
                    let id = format!("caldav-{random}");
                    match credentials.set(&credential_key(&id), &password) {
                        Ok(()) => handle.send(Event::AccountAdded(CalendarAccount {
                            id,
                            provider: "caldav".into(),
                            label: format!("{username} on {}", host_of(&server)),
                            server,
                            username,
                        })),
                        // It stays out of Keenu rather than in a plain file.
                        Err(why) => handle.send(Event::AccountFailed(why.to_string())),
                    }
                }
                Err(why) => handle.send(Event::AccountFailed(why.to_string())),
            }
        });
    }

    fn add_oauth_account(&mut self, kind: &'static str) {
        let (provider, setup) = match kind {
            "google" => (google::provider(), "KEENU_GOOGLE_CLIENT_ID"),
            _ => (graph::provider(), "KEENU_MICROSOFT_CLIENT_ID"),
        };
        let Some(provider) = provider else {
            self.view.status =
                format!("This build has no client ID: set {setup}. See docs/calendar-setup.md.");
            return;
        };
        let sign_in = match oauth::begin(&provider) {
            Ok(sign_in) => sign_in,
            Err(why) => {
                self.view.status = why.to_string();
                return;
            }
        };
        if let Err(why) = (self.platform.open_url)(&sign_in.url) {
            self.view.status = why;
            return;
        }
        self.view.status = format!("Continue in your browser to sign in to {}.", provider.name);
        let (handle, credentials) = (self.handle(), self.platform.credentials.clone());
        thread::spawn(move || {
            let tokens = match sign_in.finish(&provider, Duration::from_secs(300)) {
                Ok(tokens) => tokens,
                Err(why) => {
                    handle.send(Event::AccountFailed(why.to_string()));
                    return;
                }
            };
            let Ok(random) = keenu_security::random_hex(8) else {
                handle.send(Event::AccountFailed(
                    "No secure random numbers on this system.".into(),
                ));
                return;
            };
            let id = format!("{kind}-{random}");
            let source = TokenSource::new(provider.clone(), credentials, credential_key(&id));
            match source.save(&tokens) {
                Ok(()) => handle.send(Event::AccountAdded(CalendarAccount {
                    id,
                    provider: kind.to_owned(),
                    label: provider.name.to_owned(),
                    server: String::new(),
                    username: String::new(),
                })),
                Err(why) => handle.send(Event::AccountFailed(why.to_string())),
            }
        });
    }

    fn remove_account(&mut self, index: usize) {
        if index >= self.settings.calendar.accounts.len() {
            return;
        }
        let account = self.settings.calendar.accounts.remove(index);
        let key = credential_key(&account.id);
        if account.provider == "google" {
            // Told first, so the access does not outlive the account here. Best effort: offline it still goes.
            if let Some(provider) = google::provider() {
                let source =
                    TokenSource::new(provider, self.platform.credentials.clone(), key.clone());
                if let Some(token) = source.refresh_token() {
                    thread::spawn(move || {
                        let _ = google::revoke(&token);
                    });
                }
            }
        }
        let _ = self.platform.credentials.delete(&key);
        self.caches.remove(&account.id);
        let _ = std::fs::remove_file(self.paths.calendar_cache(&account.id));
        self.save_settings();
        self.view.status = format!("Removed {}.", account.label);
    }

    fn add_event(&mut self, text: &str) {
        let Some(parsed) = keenu_core::calendar::parse(text) else {
            self.view.status = "That needs a title, such as Lunch 1:00 pm.".into();
            return;
        };
        let Some((account_id, calendar_id)) = self.first_writable_calendar() else {
            self.view.status = "Connect a calendar and sync it first.".into();
            return;
        };
        let today = Local::now().date_naive();
        let at = |minutes: u32| {
            today
                .and_hms_opt(minutes / 60, minutes % 60, 0)
                .and_then(|time| Local.from_local_datetime(&time).earliest())
                .map(|time| time.timestamp_millis())
        };
        let all_day = parsed.start.is_none();
        let start_ms = at(parsed.start.unwrap_or(0)).unwrap_or_else(now_ms);
        let end_ms = match (parsed.end.and_then(at), all_day) {
            (Some(end), _) => end,
            (None, true) => start_ms + 86_400_000,
            (None, false) => start_ms + 3_600_000,
        };
        let draft = EventDraft {
            title: parsed.title,
            location: String::new(),
            description: String::new(),
            start_ms,
            end_ms,
            all_day,
            time_zone: local_zone_name(),
            recurrence: None,
        };
        if let Some(cache) = self.caches.get_mut(&account_id) {
            cache.queue(PendingEdit::Create {
                calendar: calendar_id,
                draft: (&draft).into(),
            });
            self.save_file(
                &self.paths.calendar_cache(&account_id),
                &self.caches[&account_id],
            );
        }
        self.sync_account(&account_id);
        self.view.status = format!("Adding {}...", draft.title);
    }

    fn first_writable_calendar(&self) -> Option<(String, String)> {
        for account in &self.settings.calendar.accounts {
            let cache = self.caches.get(&account.id)?;
            let writable = cache
                .calendars
                .iter()
                .find(|(_, state)| state.info.as_ref().is_some_and(|info| !info.read_only));
            if let Some((calendar, _)) = writable {
                return Some((account.id.clone(), calendar.clone()));
            }
        }
        None
    }

    fn delete_event(&mut self, index: usize) {
        let Some(item) = self.agenda_items.get(index).cloned() else {
            return;
        };
        let owner = self
            .settings
            .calendar
            .accounts
            .iter()
            .map(|account| account.id.clone())
            .find(|id| {
                self.caches
                    .get(id)
                    .is_some_and(|cache| cache.calendars.contains_key(&item.calendar_id))
            });
        let Some(owner) = owner else {
            return;
        };
        if let Some(cache) = self.caches.get_mut(&owner) {
            cache.queue(PendingEdit::Delete {
                calendar: item.calendar_id.clone(),
                event: item.id.clone(),
                etag: item.etag.clone(),
            });
            if let Some(state) = cache.calendars.get_mut(&item.calendar_id) {
                state.events.remove(&item.id);
            }
            self.save_file(&self.paths.calendar_cache(&owner), &self.caches[&owner]);
        }
        self.sync_account(&owner);
    }

    fn check_updates(&mut self) {
        self.last_update_check_ms = now_ms();
        if let Some(why) = self.platform.updates.availability().reason() {
            self.update_status = why.to_owned();
            return;
        }
        let Some(config) = UpdateConfig::built_in() else {
            return;
        };
        self.update_status = "Looking for a new version...".into();
        let channel = match self.settings.updates.channel {
            UpdateChannel::Stable => Channel::Stable,
            UpdateChannel::Prerelease => Channel::Prerelease,
        };
        let kinds = self.platform.updates.installable_kinds();
        let handle = self.handle();
        thread::spawn(move || {
            let result = updater::fetch_manifest(&config).and_then(|manifest| {
                let platform = keenu_core::release::current_platform()
                    .ok_or("This platform has no releases.")?;
                updater::find_update(
                    &manifest,
                    env!("CARGO_PKG_VERSION"),
                    channel,
                    platform,
                    &kinds,
                )
            });
            match result {
                Ok(Some(update)) => {
                    let status = format!("Version {} is available.", update.version);
                    handle.send(Event::UpdateFound(Some(update), status));
                }
                Ok(None) => handle.send(Event::UpdateFound(None, "Up to date.".into())),
                Err(why) => handle.send(Event::UpdateFailed(why)),
            }
        });
    }

    /// Downloads and checks the update, or installs one that already has been. True when the installer
    /// has started and Keenu should get out of its way.
    fn install_update(&mut self) -> bool {
        if let Some((file, _)) = self.update_file.take() {
            // Checked again here: the file has sat on disk since it was downloaded.
            if let Err(why) = self.platform.updates.verify_platform_signature(&file) {
                let _ = std::fs::remove_file(&file);
                self.update_status = why;
                return false;
            }
            return match self.platform.updates.install(&file) {
                Ok(()) => true,
                Err(why) => {
                    self.update_status = why;
                    false
                }
            };
        }
        let Some(update) = self.update.clone() else {
            return false;
        };
        let (handle, updates, dir) = (
            self.handle(),
            self.platform.updates.clone(),
            self.paths.updates(),
        );
        self.update_status = "Downloading...".into();
        thread::spawn(move || {
            let mut last = 0;
            let result = updater::download(&update.artifact, &dir, |done, total| {
                let percent = (done * 100).checked_div(total).unwrap_or(0);
                if percent != last {
                    last = percent;
                    handle.send(Event::UpdateProgress(format!("Downloading... {percent}%")));
                }
            });
            match result {
                Ok(file) => match updates.verify_platform_signature(&file) {
                    Ok(()) => handle.send(Event::UpdateReady(file, update.version.clone())),
                    Err(why) => {
                        let _ = std::fs::remove_file(&file);
                        handle.send(Event::UpdateFailed(why));
                    }
                },
                Err(why) => handle.send(Event::UpdateFailed(why)),
            }
        });
        false
    }

    fn voice_endpoint(&self) -> Option<(String, String)> {
        let endpoint = self.settings.voice.endpoint.clone();
        if endpoint.is_empty() {
            return None;
        }
        let key = self
            .platform
            .credentials
            .get(CREDENTIAL_VOICE)
            .ok()
            .flatten()
            .unwrap_or_default();
        Some((endpoint, key))
    }

    fn voice_toggle(&mut self) {
        if !self.settings.voice.enabled || self.voice_busy {
            return;
        }
        self.voice_error.clear();
        if self.settings.voice.stt_engine == SpeechEngine::Local {
            self.voice_listen_local();
            return;
        }
        if self.platform.voice.recording() {
            self.voice_busy = true;
            match self.platform.voice.stop() {
                Ok(recording) => self.voice_process(recording),
                Err(why) => {
                    self.voice_error = why;
                    self.voice_busy = false;
                }
            }
        } else {
            self.voice_heard.clear();
            self.voice_answer.clear();
            if let Err(why) = self.platform.voice.start(&self.settings.voice.device) {
                self.voice_error = why;
            }
        }
    }

    fn voice_listen_local(&mut self) {
        self.voice_busy = true;
        self.voice_heard.clear();
        self.voice_answer.clear();
        let (voice, handle, settings, endpoint, play) = (
            self.platform.voice.clone(),
            self.handle(),
            self.settings.voice.clone(),
            self.voice_endpoint(),
            self.platform.play_wav,
        );
        thread::spawn(move || match voice.listen_local() {
            Ok(text) => {
                handle.send(Event::VoiceHeard(text.clone()));
                respond(&*voice, &handle, &settings, endpoint, &text, play);
            }
            Err(why) => handle.send(Event::VoiceFailed(why)),
        });
    }

    fn voice_process(&mut self, recording: Recording) {
        let (voice, handle, settings, endpoint, play) = (
            self.platform.voice.clone(),
            self.handle(),
            self.settings.voice.clone(),
            self.voice_endpoint(),
            self.platform.play_wav,
        );
        thread::spawn(move || {
            if recording.samples.is_empty() {
                handle.send(Event::VoiceFailed(
                    "Nothing was recorded. Is the microphone working?".into(),
                ));
                return;
            }
            let Some((base, key)) = &endpoint else {
                handle.send(Event::VoiceFailed("Set the voice endpoint first.".into()));
                return;
            };
            let wav = remote_voice::wav(recording.sample_rate, &recording.samples);
            let target = remote_voice::Endpoint { base, api_key: key };
            match remote_voice::transcribe(&target, &settings.stt_model, &wav) {
                Ok(text) => {
                    handle.send(Event::VoiceHeard(text.clone()));
                    respond(&*voice, &handle, &settings, endpoint.clone(), &text, play);
                }
                Err(why) => handle.send(Event::VoiceFailed(why)),
            }
        });
    }

    /// What a click means.
    pub fn act(&mut self, action: Action) -> Effect {
        let effect = self.perform(action);
        self.refresh_view();
        effect
    }

    fn perform(&mut self, action: Action) -> Effect {
        log::debug!("action: {}", action_name(&action));
        self.view.status.clear();
        match action {
            Action::Expand => self.set_expanded(true),
            Action::Collapse => self.set_expanded(false),
            Action::SelectPage(page) => {
                self.view.page = page;
                self.view.scroll = 0;
                self.view.focus = None;
                self.view.settings.confirming_delete = false;
                if page == Page::Calendar && self.last_sync_ms == 0 {
                    self.sync_all();
                }
            }
            Action::Dismiss => self.view.input = None,
            Action::PlayPause => self.media_command(MediaCommand::PlayPause),
            Action::NextTrack => self.media_command(MediaCommand::Next),
            Action::PreviousTrack => self.media_command(MediaCommand::Previous),
            Action::SeekFraction(thousandths) => {
                if let Some(session) = self.current_session() {
                    let at = session.duration_ms * i64::from(thousandths) / 1000;
                    self.media_command(MediaCommand::Seek(at));
                }
            }
            Action::SeekToLine(index) => {
                if let Some(at) = self
                    .lyrics
                    .as_ref()
                    .and_then(|lyrics| lyrics.time_of(index))
                {
                    self.media_command(MediaCommand::Seek(at));
                }
            }
            Action::SelectSession(_) => self.next_session(),
            Action::CopyClip(id) => self.copy_clip(id),
            Action::PinClip(id, pinned) => {
                if self.clips.set_pinned(id, pinned).is_err() {
                    self.view.status = "Only so many can be pinned; unpin one first.".into();
                }
                self.save_file(&self.paths.clipboard(), &self.clips);
            }
            Action::DeleteClip(id) => {
                if let Some(unused) = self.clips.remove(id) {
                    self.remove_clip_images(&unused);
                    self.save_file(&self.paths.clipboard(), &self.clips);
                }
            }
            Action::ClearClips => {
                let unused = self.clips.clear();
                self.remove_clip_images(&unused);
                self.save_file(&self.paths.clipboard(), &self.clips);
            }
            Action::OpenShelf(index) => {
                if let Some(entry) = self.shelf.entries().get(index)
                    && let Err(why) = self.platform.shelf.open(&entry.path)
                {
                    self.view.status = why;
                }
            }
            Action::RevealShelf(index) => {
                if let Some(entry) = self.shelf.entries().get(index)
                    && let Err(why) = self.platform.shelf.reveal(&entry.path)
                {
                    self.view.status = why;
                }
            }
            Action::PinShelf(index, pinned) => {
                if let Some(path) = self
                    .shelf
                    .entries()
                    .get(index)
                    .map(|entry| entry.path.clone())
                {
                    self.shelf.set_pinned(&path, pinned);
                    self.save_file(&self.paths.shelf(), &self.shelf);
                }
            }
            Action::RemoveShelf(index) => {
                if let Some(path) = self
                    .shelf
                    .entries()
                    .get(index)
                    .map(|entry| entry.path.clone())
                {
                    self.shelf.remove(&path);
                    self.save_file(&self.paths.shelf(), &self.shelf);
                }
            }
            Action::ClearShelf => {
                self.shelf.clear_unpinned();
                self.save_file(&self.paths.shelf(), &self.shelf);
            }
            Action::StartInput(field) => {
                let text = if field == Field::ClipboardSearch {
                    self.view.clip_query.clone()
                } else {
                    String::new()
                };
                self.view.input = Some(InputView { field, text });
            }
            Action::AddCalDavAccount => {
                self.view.input = Some(InputView {
                    field: Field::CalDavServer,
                    text: String::new(),
                });
            }
            Action::AddGoogleAccount => self.add_oauth_account("google"),
            Action::AddMicrosoftAccount => self.add_oauth_account("microsoft"),
            Action::RemoveAccount(index) => self.remove_account(index),
            Action::DeleteEvent(index) => self.delete_event(index),
            Action::SyncCalendars => {
                if self.settings.calendar.accounts.is_empty() {
                    self.view.status = "Connect a calendar first.".into();
                } else {
                    self.sync_all();
                }
            }
            Action::VoiceToggle => self.voice_toggle(),
            Action::VoiceCancel => {
                let _ = self.platform.voice.cancel();
            }
            Action::NextMicrophone => self.next_microphone(),
            Action::ApprovalAllow(id) => {
                let shown_since = self
                    .shown_approval
                    .as_ref()
                    .filter(|(shown, _)| *shown == id)
                    .map(|(_, at)| *at);
                if shown_since.is_some_and(|at| now_ms() - at >= ALLOW_ARMS_AFTER_MS) {
                    self.answer(&id, Answer::Allow);
                } else {
                    log::info!(
                        "a click on Allow came before the request could have been read: ignored"
                    );
                }
            }
            Action::ApprovalDeny(id) => self.answer(&id, Answer::Deny),
            Action::ApprovalInTerminal(id) => self.answer(&id, Answer::NoDecision),
            Action::ClearUsage => {
                self.usage.clear();
                self.save_file(&self.paths.usage(), &self.usage);
            }
            Action::ExportUsage => self.export_usage(),
            Action::DeleteUsageApp(index) => {
                if let Some(app) = self.view.usage.get(index).map(|row| row.app.clone()) {
                    self.usage.delete_app(&app);
                    self.save_file(&self.paths.usage(), &self.usage);
                }
            }
            Action::CategorizeUsageApp(index) => {
                if self.view.usage.get(index).is_some() {
                    self.caldav_draft.0 = self.view.usage[index].app.clone();
                    self.view.input = Some(InputView {
                        field: Field::UsageCategory,
                        text: self.view.usage[index].category.clone(),
                    });
                }
            }
            Action::Toggle(setting) => self.toggle(setting),
            Action::CheckUpdates => self.check_updates(),
            Action::InstallUpdate => {
                if self.install_update() {
                    return Effect::Quit;
                }
            }
            Action::ExportData => {
                let file = self.paths.root.join("keenu-export.json");
                self.view.status = match data::export_all(&self.paths, &file) {
                    Ok(()) => format!("Saved to {}.", file.display()),
                    Err(why) => why,
                };
            }
            Action::DeleteAllData => self.view.settings.confirming_delete = true,
            Action::ConfirmDeleteAllData => {
                self.view.settings.confirming_delete = false;
                self.delete_all();
            }
            Action::ResetSettings => {
                let onboarded = self.settings.onboarded;
                self.settings = Settings {
                    onboarded,
                    calendar: std::mem::take(&mut self.settings.calendar),
                    ..Settings::default()
                };
                self.apply_settings();
                self.save_settings();
                self.view.status = "Settings are back to how they started.".into();
            }
            Action::OpenDataFolder => {
                if let Err(why) = self.platform.shelf.open(&self.paths.root) {
                    self.view.status = why;
                }
            }
            Action::OnboardingNext => self.onboarding_next(),
            Action::OnboardingSkip => self.onboarding_finish(),
            Action::SelectMonitor => self.next_monitor(),
            Action::Quit => return Effect::Quit,
        }
        Effect::None
    }

    /// Applies what settings decide outside themselves.
    fn apply_settings(&mut self) {
        self.recording_usage
            .store(self.settings.usage.recording(), Ordering::Relaxed);
        let _ = self
            .platform
            .lifecycle
            .set_start_at_login(self.settings.window.start_at_login);
    }

    fn media_command(&mut self, command: MediaCommand) {
        let Some(id) = self.current_session().map(|session| session.id.clone()) else {
            return;
        };
        // Apps can take a moment over it; the island should not.
        let (media, handle) = (self.platform.media.clone(), self.handle());
        thread::spawn(move || {
            if let Err(why) = media.command(&id, command) {
                handle.send(Event::Status(why));
            }
        });
    }

    fn next_session(&mut self) {
        if self.sessions.len() < 2 {
            return;
        }
        let current = self.current_session().map(|session| session.id.clone());
        let at = self
            .sessions
            .iter()
            .position(|session| Some(&session.id) == current.as_ref())
            .unwrap_or(0);
        self.chosen_session = Some(self.sessions[(at + 1) % self.sessions.len()].id.clone());
        let sessions = std::mem::take(&mut self.sessions);
        self.on_sessions(sessions);
    }

    fn copy_clip(&mut self, id: u64) {
        let Some(item) = self.clips.get(id) else {
            return;
        };
        let Some(content) = clip_to_content(&item.clip, &self.paths) else {
            self.view.status = "That image's file is gone.".into();
            return;
        };
        match self.platform.clipboard.write(&content) {
            Ok(()) => {
                self.clips.touch(id, now_ms());
                self.save_file(&self.paths.clipboard(), &self.clips);
            }
            Err(why) => self.view.status = why,
        }
    }

    fn next_microphone(&mut self) {
        self.devices = self.platform.voice.devices().unwrap_or_default();
        if self.devices.is_empty() {
            return;
        }
        let at = self
            .devices
            .iter()
            .position(|device| device.id == self.settings.voice.device)
            .unwrap_or(0);
        self.settings.voice.device = self.devices[(at + 1) % self.devices.len()].id.clone();
        self.save_settings();
    }

    fn next_monitor(&mut self) {
        let Ok(monitors) = self.platform.monitors.monitors() else {
            return;
        };
        if monitors.is_empty() {
            return;
        }
        let at = monitors
            .iter()
            .position(|monitor| monitor.name == self.settings.window.monitor)
            .unwrap_or(0);
        self.settings.window.monitor = monitors[(at + 1) % monitors.len()].name.clone();
        self.save_settings();
    }

    fn answer(&mut self, id: &str, answer: Answer) {
        log::info!("agent request answered: {answer:?}");
        // Taken out of the queue first: a second click finds nothing to answer.
        if let Some(at) = self
            .approvals
            .iter()
            .position(|pending| pending.request.id == id)
            && let Some(pending) = self.approvals.remove(at)
        {
            (pending.reply)(answer);
        }
    }

    fn export_usage(&mut self) {
        let file = self.paths.root.join("keenu-usage.csv");
        let csv = self.usage.to_csv(&self.settings.usage.categories);
        self.view.status = match std::fs::write(&file, csv) {
            Ok(()) => format!("Saved to {}.", file.display()),
            Err(err) => err.to_string(),
        };
    }

    fn delete_all(&mut self) {
        self.clips = ClipboardHistory::default();
        self.shelf = FileShelf::default();
        self.usage = UsageLog::default();
        self.reminders = Reminders::default();
        for account in std::mem::take(&mut self.settings.calendar.accounts) {
            let _ = self
                .platform
                .credentials
                .delete(&credential_key(&account.id));
        }
        let _ = self.platform.credentials.delete(CREDENTIAL_VOICE);
        self.caches.clear();
        self.caches.insert("local".into(), AccountCache::default());
        self.settings.voice.endpoint.clear();
        self.save_settings();
        self.view.status = match data::delete_all_data(&self.paths) {
            Ok(()) => "Your data here has been deleted.".into(),
            Err(why) => why,
        };
    }

    fn toggle(&mut self, setting: Setting) {
        match setting {
            Setting::ClipboardHistory => {
                self.settings.clipboard.enabled = !self.settings.clipboard.enabled
            }
            Setting::ClipboardPaused => {
                self.settings.clipboard.paused = !self.settings.clipboard.paused
            }
            Setting::UsageTracking => self.settings.usage.enabled = !self.settings.usage.enabled,
            Setting::UsagePaused => self.settings.usage.paused = !self.settings.usage.paused,
            Setting::VoiceEnabled => {
                self.settings.voice.enabled = !self.settings.voice.enabled;
                if !self.settings.voice.enabled {
                    let _ = self.platform.voice.cancel();
                }
            }
            Setting::OnlineLyrics => self.settings.lyrics.online = !self.settings.lyrics.online,
            Setting::UpdateChecks => {
                self.settings.updates.check = !self.settings.updates.check;
                if !self.settings.updates.check {
                    self.settings.updates.auto_install = false;
                }
            }
            Setting::UpdateAutoInstall => {
                self.settings.updates.auto_install = !self.settings.updates.auto_install
            }
            Setting::PrereleaseChannel => {
                self.settings.updates.channel = match self.settings.updates.channel {
                    UpdateChannel::Stable => UpdateChannel::Prerelease,
                    UpdateChannel::Prerelease => UpdateChannel::Stable,
                };
            }
            Setting::StartAtLogin => {
                let wanted = !self.settings.window.start_at_login;
                match self.platform.lifecycle.set_start_at_login(wanted) {
                    Ok(()) => self.settings.window.start_at_login = wanted,
                    Err(why) => self.view.status = why,
                }
            }
            Setting::HideInFullscreen => {
                self.settings.window.hide_in_fullscreen = !self.settings.window.hide_in_fullscreen
            }
            Setting::Notifications => {
                self.settings.notifications.enabled = !self.settings.notifications.enabled
            }
            Setting::ClaudeHooks => self.toggle_hooks(Agent::Claude),
            Setting::CodexHooks => self.toggle_hooks(Agent::Codex),
        }
        self.recording_usage
            .store(self.settings.usage.recording(), Ordering::Relaxed);
        self.save_settings();
    }

    fn toggle_hooks(&mut self, agent: Agent) {
        let result = if agents::installed(agent) {
            agents::uninstall(agent).map(drop)
        } else {
            std::env::current_exe()
                .map_err(anyhow::Error::from)
                .and_then(|exe| agents::install(agent, &agents::hook_command(&exe, agent)))
        };
        self.view.status = match result {
            Ok(()) if agents::installed(agent) => agent
                .after_install()
                .unwrap_or("Hooks installed.")
                .to_owned(),
            Ok(()) => "Hooks removed.".to_owned(),
            Err(why) => format!("{}: {why:#}", agent.title()),
        };
    }

    fn onboarding_next(&mut self) {
        match self.onboarding_step {
            Some(step) if step + 1 < ONBOARDING_STEPS => self.onboarding_step = Some(step + 1),
            _ => self.onboarding_finish(),
        }
    }

    fn onboarding_finish(&mut self) {
        self.onboarding_step = None;
        self.settings.onboarded = true;
        self.save_settings();
    }

    /// A key while the island has focus.
    pub fn key(&mut self, key: Key) -> Effect {
        let effect = self.handle_key(key);
        self.refresh_view();
        effect
    }

    fn handle_key(&mut self, key: Key) -> Effect {
        if self.view.input.is_some() {
            self.input_key(key);
            return Effect::None;
        }
        let scene = view::build(&self.view);
        match key {
            Key::Escape => {
                if self.view.approval.is_some() {
                    // Leaving the question means the terminal asks instead, never an allow.
                    if let Some(id) = self
                        .view
                        .approval
                        .as_ref()
                        .map(|approval| approval.id.clone())
                    {
                        self.answer(&id, Answer::NoDecision);
                    }
                } else {
                    self.set_expanded(false);
                }
            }
            Key::Tab | Key::ShiftTab if !scene.hits.is_empty() => {
                let count = scene.hits.len();
                self.view.focus = Some(match (self.view.focus, key) {
                    (None, Key::ShiftTab) => count - 1,
                    (None, _) => 0,
                    (Some(at), Key::ShiftTab) => (at + count - 1) % count,
                    (Some(at), _) => (at + 1) % count,
                });
            }
            Key::Enter | Key::Char(' ') => {
                if let Some(hit) = self.view.focus.and_then(|focus| scene.hits.get(focus)) {
                    let action = hit.action.clone();
                    return self.perform(action);
                }
                if key == Key::Char(' ') && self.view.expanded {
                    self.media_command(MediaCommand::PlayPause);
                }
            }
            _ => {}
        }
        Effect::None
    }

    fn input_key(&mut self, key: Key) {
        let Some(input) = self.view.input.as_mut() else {
            return;
        };
        match key {
            Key::Escape => {
                let cancelled = input.field;
                self.view.input = None;
                if cancelled == Field::ClipboardSearch {
                    self.view.clip_query.clear();
                }
                self.caldav_draft = (String::new(), String::new());
            }
            Key::Backspace => {
                input.text.pop();
                if input.field == Field::ClipboardSearch {
                    self.view.clip_query = input.text.clone();
                }
            }
            Key::Char(c) if !c.is_control() => {
                input.text.push(c);
                if input.field == Field::ClipboardSearch {
                    self.view.clip_query = input.text.clone();
                }
            }
            Key::Enter => {
                if let Some(input) = self.view.input.take() {
                    self.commit_input(input);
                }
            }
            _ => {}
        }
    }

    fn commit_input(&mut self, input: InputView) {
        let text = input.text.trim().to_owned();
        match input.field {
            Field::ClipboardSearch => self.view.clip_query = text,
            Field::NewEvent => self.add_event(&text),
            Field::CalDavServer => {
                if keenu_integrations::http::is_secure_url(&text) {
                    self.caldav_draft.0 = text;
                    self.view.input = Some(InputView {
                        field: Field::CalDavUser,
                        text: String::new(),
                    });
                } else {
                    self.view.status =
                        "The address must start with https://, so your password is safe.".into();
                }
            }
            Field::CalDavUser => {
                self.caldav_draft.1 = text;
                self.view.input = Some(InputView {
                    field: Field::CalDavPassword,
                    text: String::new(),
                });
            }
            Field::CalDavPassword => self.add_caldav_account(input.text),
            Field::VoiceEndpoint => {
                if text.is_empty() {
                    self.settings.voice.endpoint.clear();
                    self.settings.voice.stt_engine = SpeechEngine::Local;
                    self.settings.voice.tts_engine = SpeechEngine::Local;
                    let _ = self.platform.credentials.delete(CREDENTIAL_VOICE);
                    self.save_settings();
                } else if keenu_integrations::http::is_secure_url(&text) {
                    let voice = &mut self.settings.voice;
                    voice.endpoint = text;
                    voice.stt_engine = SpeechEngine::Remote;
                    voice.tts_engine = SpeechEngine::Remote;
                    if voice.stt_model.is_empty() {
                        voice.stt_model = "whisper-1".into();
                    }
                    self.save_settings();
                    self.view.input = Some(InputView {
                        field: Field::VoiceApiKey,
                        text: String::new(),
                    });
                } else {
                    self.view.status = "The address must use HTTPS, or be on this computer.".into();
                }
            }
            Field::VoiceApiKey => {
                if !input.text.is_empty() {
                    self.view.status =
                        match self.platform.credentials.set(CREDENTIAL_VOICE, &input.text) {
                            Ok(()) => "API key saved in the system's credential store.".into(),
                            Err(why) => why.to_string(),
                        };
                }
            }
            Field::UsageCategory => {
                let app = std::mem::take(&mut self.caldav_draft.0);
                if text.is_empty() {
                    self.settings.usage.categories.remove(&app);
                } else {
                    self.settings.usage.categories.insert(app, text);
                }
                self.save_settings();
            }
        }
    }

    /// Moves a list up or down, by notches of a mouse wheel.
    pub fn scroll(&mut self, notches: i32) {
        let rows = match self.view.page {
            Page::Calendar => self.view.agenda.len(),
            Page::Clipboard => self.view.clips.len(),
            Page::Shelf => self.view.shelf.len(),
            Page::Usage => self.view.usage.len(),
            Page::Settings => view::settings_rows(&self.view),
            _ => 0,
        };
        self.view.scroll = view::clamp_scroll(self.view.scroll as i64 - i64::from(notches), rows);
        self.refresh_view();
    }

    /// Rebuilds what the island shows from what is known. Everything shown comes from here, so what
    /// is on screen cannot drift from what is true.
    pub fn refresh_view(&mut self) {
        let now = Local::now();
        self.view.time = now.format("%H:%M").to_string();
        self.view.date = now.format("%a %-d %b").to_string();
        self.fill_media();
        self.fill_calendar(now);
        self.fill_clipboard();
        self.fill_shelf();
        self.fill_usage();
        self.fill_voice();
        self.fill_agents();
        self.fill_settings();
        self.fill_approval();
        self.fill_onboarding();
    }

    fn fill_media(&mut self) {
        let session = self.current_session().cloned();
        self.view.session_count = self.sessions.len();
        self.view.media = session.map(|session| MediaView {
            artwork_key: {
                let key = Self::artwork_key(&self.track);
                self.images.contains_key(&key).then_some(key)
            },
            title: session.title,
            artist: session.artist,
            album: session.album,
            app: session.app_name,
            playing: session.state == PlaybackState::Playing,
            position_ms: session.position_ms,
            duration_ms: session.duration_ms,
            can_next: session.can_next,
            can_previous: session.can_previous,
            can_seek: session.can_seek,
        });
        let position = self
            .view
            .media
            .as_ref()
            .map_or(0, |media| media.position_ms);
        self.view.lyrics = match &self.lyrics {
            Some(lyrics) => {
                let active = lyrics.active(position);
                let range = lyrics.window(active, LYRICS_BEFORE, LYRICS_AFTER);
                LyricsView {
                    lines: range
                        .map(|at| (at, lyrics.lines()[at].text.clone()))
                        .collect(),
                    active,
                    note: String::new(),
                }
            }
            None => LyricsView {
                lines: Vec::new(),
                active: None,
                note: self.lyrics_note.clone(),
            },
        };
    }

    fn fill_calendar(&mut self, now: DateTime<Local>) {
        let from = now.timestamp_millis();
        let to = from + AGENDA_DAYS * 86_400_000;
        let mut items: Vec<(AgendaItem, bool)> = Vec::new();
        for (id, cache) in &self.caches {
            let deletable = id != "local";
            items.extend(
                cache
                    .agenda(from, to)
                    .into_iter()
                    .map(|item| (item, deletable)),
            );
        }
        items.sort_by_key(|(item, _)| (item.start_ms, item.title.clone()));
        self.view.agenda = items
            .iter()
            .map(|(item, deletable)| {
                let start = Local
                    .timestamp_millis_opt(item.start_ms)
                    .single()
                    .unwrap_or(now);
                let end = Local
                    .timestamp_millis_opt(item.end_ms)
                    .single()
                    .unwrap_or(now);
                AgendaRow {
                    deletable: *deletable,
                    day: start.format("%a %-d %b").to_string(),
                    time: if item.all_day {
                        "All day".to_owned()
                    } else {
                        format!("{} - {}", start.format("%H:%M"), end.format("%H:%M"))
                    },
                    title: item.title.clone(),
                    detail: if item.location.is_empty() {
                        item.calendar.clone()
                    } else {
                        format!("{}  ·  {}", item.location, item.calendar)
                    },
                }
            })
            .collect();
        self.agenda_items = items.into_iter().map(|(item, _)| item).collect();
        self.view.accounts = self
            .settings
            .calendar
            .accounts
            .iter()
            .map(|account| {
                let cache = self.caches.get(&account.id);
                let error = cache.map_or("", |cache| cache.last_error.as_str());
                let status = if self.syncing.contains(&account.id) {
                    "syncing".to_owned()
                } else if !error.is_empty() {
                    "not synced".to_owned()
                } else if cache.is_some_and(|cache| cache.last_sync_ms > 0) {
                    "synced".to_owned()
                } else {
                    "waiting".to_owned()
                };
                AccountRow {
                    label: account.label.clone(),
                    ok: error.is_empty(),
                    status,
                }
            })
            .collect();
    }

    fn fill_clipboard(&mut self) {
        let settings = &self.settings.clipboard;
        self.view.clips_note = if !settings.enabled {
            "Clipboard history is off. Nothing you copy is read or kept. Turn it on in Settings."
                .to_owned()
        } else if settings.paused {
            "Paused: nothing new is being recorded.".to_owned()
        } else if self.clips.items().is_empty() {
            "Nothing yet. What you copy will show up here, and stays on this computer.".to_owned()
        } else {
            String::new()
        };
        let query = self.view.clip_query.to_lowercase();
        self.view.clips = self
            .clips
            .items()
            .iter()
            .filter(|item| query.is_empty() || item.clip.contains(&query))
            .map(|item| {
                let image_key = item.clip.image_hash().map(|hash| format!("clip:{hash}"));
                if let (Some(key), Some(hash)) = (&image_key, item.clip.image_hash())
                    && !self.images.contains_key(key)
                    && let Ok(bytes) =
                        std::fs::read(self.paths.clip_images().join(format!("{hash}.png")))
                {
                    self.images.insert(key.clone(), Arc::new(bytes));
                }
                ClipRow {
                    id: item.id,
                    preview: if item.clip.image_hash().is_some() {
                        "Image".to_owned()
                    } else {
                        item.clip.preview()
                    },
                    kind: item.clip.kind().to_owned(),
                    pinned: item.pinned,
                    image_key,
                }
            })
            .collect();
    }

    fn fill_shelf(&mut self) {
        self.view.shelf = self
            .shelf
            .entries()
            .iter()
            .map(|entry| {
                let key = thumbnail_key(&entry.path);
                ShelfRow {
                    name: entry.name.clone(),
                    detail: format!(
                        "{}  ·  {}",
                        if entry.is_dir {
                            "Folder".to_owned()
                        } else {
                            file_shelf::format_size(entry.size)
                        },
                        entry
                            .path
                            .parent()
                            .map(|parent| parent.display().to_string())
                            .unwrap_or_default(),
                    ),
                    pinned: entry.pinned,
                    missing: entry.missing,
                    thumbnail_key: self.images.contains_key(&key).then_some(key),
                }
            })
            .collect();
    }

    fn fill_usage(&mut self) {
        let settings = &self.settings.usage;
        self.view.usage_note = if !settings.enabled {
            "App usage is off. The app in front is not looked at. Turn it on in Settings to see where your time goes; only an app's name and for how long are kept, on this computer.".to_owned()
        } else if settings.paused {
            "Paused: nothing new is being recorded.".to_owned()
        } else {
            String::new()
        };
        let rows = self
            .usage
            .summary(start_of_today_ms(), &settings.categories);
        let top = rows.first().map_or(1, |row| row.duration_ms.max(1));
        self.view.usage = rows
            .into_iter()
            .map(|row| UsageRow {
                fraction: row.duration_ms as f32 / top as f32,
                app: row.app,
                category: row.category,
                duration_ms: row.duration_ms,
            })
            .collect();
    }

    fn fill_voice(&mut self) {
        let settings = &self.settings.voice;
        let microphone = self
            .devices
            .iter()
            .find(|device| device.id == settings.device)
            .map_or_else(|| "System default".to_owned(), |device| device.name.clone());
        let availability = self.platform.voice.availability();
        self.view.voice = VoiceView {
            enabled: settings.enabled,
            recording: self.platform.voice.recording(),
            busy: self.voice_busy,
            microphone,
            heard: self.voice_heard.clone(),
            answer: self.voice_answer.clone(),
            error: match availability.reason() {
                Some(why) => why.to_owned(),
                None => self.voice_error.clone(),
            },
            engine: match settings.stt_engine {
                SpeechEngine::Local => "On this computer".to_owned(),
                SpeechEngine::Remote => host_of(&settings.endpoint),
            },
        };
    }

    fn fill_agents(&mut self) {
        self.view.agents = Agent::ALL
            .iter()
            .map(|agent| {
                let installed = agents::installed(*agent);
                AgentRow {
                    name: agent.title().to_owned(),
                    setting: match agent {
                        Agent::Claude => Setting::ClaudeHooks,
                        Agent::Codex => Setting::CodexHooks,
                    },
                    installed,
                    note: match (installed, agent.after_install()) {
                        (true, Some(note)) => note.to_owned(),
                        (true, None) => "Hooks installed.".to_owned(),
                        (false, _) => format!(
                            "Adds Keenu's hooks to {}; the rest of that file is left alone.",
                            agent.hooks_file().map_or_else(
                                || "its settings".to_owned(),
                                |path| path.display().to_string()
                            )
                        ),
                    },
                }
            })
            .collect();
    }

    fn fill_settings(&mut self) {
        let settings = &self.settings;
        let update_block = self.platform.updates.availability();
        let update_reason = update_block.reason().map(str::to_owned);
        let switch =
            |title: &str, detail: &str, setting: Setting, on: bool, unavailable: Option<String>| {
                SwitchRow {
                    title: title.to_owned(),
                    detail: detail.to_owned(),
                    setting,
                    on,
                    unavailable,
                }
            };
        let voice_reason = match self.platform.voice.availability() {
            Availability::Unsupported(why) => Some(why),
            _ => None,
        };
        let clipboard_reason = self
            .platform
            .clipboard
            .availability()
            .reason()
            .map(str::to_owned);
        let usage_reason = self
            .platform
            .usage
            .availability()
            .reason()
            .map(str::to_owned);
        let notification_reason = self
            .platform
            .notifier
            .availability()
            .reason()
            .map(str::to_owned);
        let confirming = self.view.settings.confirming_delete;
        let monitor = if settings.window.monitor.is_empty() {
            "Primary".to_owned()
        } else {
            settings.window.monitor.clone()
        };
        self.view.settings = SettingsView {
            switches: vec![
                switch(
                    "Clipboard history",
                    "Keeps what you copy, on this computer",
                    Setting::ClipboardHistory,
                    settings.clipboard.enabled,
                    clipboard_reason,
                ),
                switch(
                    "Pause clipboard history",
                    "Stops recording for now",
                    Setting::ClipboardPaused,
                    settings.clipboard.paused,
                    None,
                ),
                switch(
                    "App usage",
                    "Records the app in front and for how long",
                    Setting::UsageTracking,
                    settings.usage.enabled,
                    usage_reason,
                ),
                switch(
                    "Pause app usage",
                    "Stops recording for now",
                    Setting::UsagePaused,
                    settings.usage.paused,
                    None,
                ),
                switch(
                    "Voice",
                    "The microphone opens only while you record",
                    Setting::VoiceEnabled,
                    settings.voice.enabled,
                    voice_reason,
                ),
                switch(
                    "Online lyrics",
                    "Sends the song's title and artist to LRCLIB",
                    Setting::OnlineLyrics,
                    settings.lyrics.online,
                    None,
                ),
                switch(
                    "Look for updates",
                    "Once a day, from the release manifest",
                    Setting::UpdateChecks,
                    settings.updates.check,
                    update_reason.clone(),
                ),
                switch(
                    "Install updates automatically",
                    "After their signatures are checked",
                    Setting::UpdateAutoInstall,
                    settings.updates.auto_install,
                    update_reason.clone(),
                ),
                switch(
                    "Include prereleases",
                    "Test versions before they are stable",
                    Setting::PrereleaseChannel,
                    settings.updates.channel == UpdateChannel::Prerelease,
                    update_reason.clone(),
                ),
                switch(
                    "Start at login",
                    "Opens Keenu when you sign in",
                    Setting::StartAtLogin,
                    settings.window.start_at_login,
                    None,
                ),
                switch(
                    "Step aside for fullscreen",
                    "Hides while a game or video is fullscreen",
                    Setting::HideInFullscreen,
                    settings.window.hide_in_fullscreen,
                    None,
                ),
                switch(
                    "Notifications",
                    "Reminders and agents, through the system",
                    Setting::Notifications,
                    settings.notifications.enabled,
                    notification_reason,
                ),
            ],
            version: env!("CARGO_PKG_VERSION").to_owned(),
            update_status: if self.update_status.is_empty() {
                update_reason
                    .clone()
                    .unwrap_or_else(|| "Not checked yet.".to_owned())
            } else {
                self.update_status.clone()
            },
            update_ready: self
                .update_file
                .as_ref()
                .map(|(_, version)| version.clone())
                .or_else(|| self.update.as_ref().map(|update| update.version.clone())),
            update_available: update_reason.is_none(),
            monitor,
            confirming_delete: confirming,
        };
    }

    fn fill_approval(&mut self) {
        let now = now_ms();
        let front = self
            .approvals
            .front()
            .map(|pending| pending.request.id.clone());
        if front.as_ref() != self.shown_approval.as_ref().map(|(id, _)| id) {
            self.shown_approval = front.map(|id| (id, now));
            // Keyboard focus is not carried over: Enter must not answer something just arrived.
            self.view.focus = None;
        }
        self.view.approval = self.approvals.front().map(|pending| {
            let request = &pending.request;
            ApprovalView {
                id: request.id.clone(),
                agent: request.agent.title().to_owned(),
                project: request.project.clone(),
                tool: request.tool.clone(),
                detail: request.detail.clone(),
                truncated: request.truncated,
                cwd: request.cwd.clone(),
                permissions: request.suggested_permissions.clone(),
                seconds_left: ((pending.deadline_ms - now).max(0) / 1000) as u32,
            }
        });
    }

    fn fill_onboarding(&mut self) {
        self.view.onboarding = self
            .onboarding_step
            .map(|step| onboarding_view(step, &self.settings));
    }
}

const ONBOARDING_STEPS: usize = 3;

/// An action's name for the log, without the ids or text it may carry.
fn action_name(action: &Action) -> String {
    let debug = format!("{action:?}");
    debug
        .split(['(', ' '])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// The first-run screens. Everything starts off; these are where it can be turned on.
fn onboarding_view(step: usize, settings: &Settings) -> OnboardingView {
    let switch = |title: &str, detail: &str, setting: Setting, on: bool| SwitchRow {
        title: title.to_owned(),
        detail: detail.to_owned(),
        setting,
        on,
        unavailable: None,
    };
    let (title, body, switches) = match step {
        0 => (
            "Welcome to Keenu",
            "An island at the top of your screen: music with lyrics, your calendar, a clipboard history, a file shelf, voice, and coding agents asking you right here. Everything that watches, listens or reaches the network starts off. You decide.",
            Vec::new(),
        ),
        1 => (
            "What may Keenu keep?",
            "All of it stays on this computer, and you can pause, export or delete it at any time in Settings.",
            vec![
                switch(
                    "Clipboard history",
                    "Keeps what you copy; apps can mark things as private",
                    Setting::ClipboardHistory,
                    settings.clipboard.enabled,
                ),
                switch(
                    "App usage",
                    "Only an app's name and for how long",
                    Setting::UsageTracking,
                    settings.usage.enabled,
                ),
                switch(
                    "Voice",
                    "The microphone opens only while you record",
                    Setting::VoiceEnabled,
                    settings.voice.enabled,
                ),
            ],
        ),
        _ => (
            "What may Keenu reach?",
            "Calendars connect from the Calendar page. Coding agents connect from the Agents page.",
            vec![
                switch(
                    "Online lyrics",
                    "Sends the song's title and artist to LRCLIB",
                    Setting::OnlineLyrics,
                    settings.lyrics.online,
                ),
                switch(
                    "Look for updates",
                    "Once a day, from the release manifest",
                    Setting::UpdateChecks,
                    settings.updates.check,
                ),
                switch(
                    "Start at login",
                    "Opens Keenu when you sign in",
                    Setting::StartAtLogin,
                    settings.window.start_at_login,
                ),
            ],
        ),
    };
    OnboardingView {
        step,
        steps: ONBOARDING_STEPS,
        title: title.to_owned(),
        body: body.to_owned(),
        switches,
    }
}

/// Answers what was heard, and says the answer, as far as the user has set it up.
fn respond(
    voice: &dyn VoiceProvider,
    handle: &AppHandle,
    settings: &keenu_core::settings::VoiceSettings,
    endpoint: Option<(String, String)>,
    heard: &str,
    play: fn(&[u8]) -> Result<(), String>,
) {
    let Some((base, key)) = &endpoint else {
        handle.send(Event::VoiceDone);
        return;
    };
    if settings.chat_model.is_empty() {
        handle.send(Event::VoiceDone);
        return;
    }
    let target = remote_voice::Endpoint { base, api_key: key };
    let reply = match remote_voice::answer(&target, &settings.chat_model, heard) {
        Ok(reply) => reply,
        Err(why) => {
            handle.send(Event::VoiceFailed(why));
            return;
        }
    };
    handle.send(Event::VoiceAnswer(reply.clone()));
    let spoken = match settings.tts_engine {
        SpeechEngine::Local => voice.speak_local(&reply),
        SpeechEngine::Remote if !settings.tts_model.is_empty() => {
            remote_voice::speak(&target, &settings.tts_model, &settings.tts_voice, &reply)
                .and_then(|audio| play(&audio))
        }
        SpeechEngine::Remote => voice.speak_local(&reply),
    };
    match spoken {
        Ok(()) => handle.send(Event::VoiceDone),
        Err(why) => handle.send(Event::VoiceFailed(why)),
    }
}

fn credential_key(account: &str) -> String {
    format!("calendar/{account}")
}

fn thumbnail_key(path: &Path) -> String {
    format!(
        "thumb:{}",
        keenu_core::clipboard::content_hash(path.to_string_lossy().as_bytes())
    )
}

/// The server's name out of an address.
fn host_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// This computer's IANA zone where the system says, for events planned here.
fn local_zone_name() -> String {
    std::env::var("TZ")
        .ok()
        .filter(|zone| zone.contains('/'))
        .unwrap_or_else(|| "UTC".to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    use keenu_core::lrc::LyricLine;
    use keenu_integrations::agents::Agent;
    use keenu_platform::Monitor;
    use keenu_security::credentials::MemoryStore;

    use super::*;

    /// A platform where everything works, and remembers what was asked of it: a mock.
    #[derive(Default)]
    struct MockState {
        clipboard_reads: AtomicUsize,
        clipboard: Mutex<Option<ClipboardContent>>,
        written: Mutex<Vec<ClipboardContent>>,
        notifications: Mutex<Vec<NativeNotification>>,
        notify_fails: AtomicBool,
        recording: AtomicBool,
        recording_starts: AtomicUsize,
        foreground_asks: AtomicUsize,
        start_at_login: AtomicBool,
        opened: Mutex<Vec<PathBuf>>,
        media_commands: Mutex<Vec<MediaCommand>>,
    }

    #[derive(Clone, Default)]
    struct Mock(Arc<MockState>);

    impl MediaProvider for Mock {
        fn availability(&self) -> Availability {
            Availability::Available
        }
        fn sessions(&self) -> keenu_platform::Result<Vec<MediaSession>> {
            Ok(Vec::new())
        }
        fn command(&self, _: &str, command: MediaCommand) -> keenu_platform::Result<()> {
            self.0.media_commands.lock().unwrap().push(command);
            Ok(())
        }
        fn artwork(&self, _: &str) -> keenu_platform::Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }

    impl NotificationProvider for Mock {
        fn availability(&self) -> Availability {
            Availability::Available
        }
        fn show(&self, notification: &NativeNotification) -> keenu_platform::Result<()> {
            if self.0.notify_fails.load(Ordering::Relaxed) {
                return Err("no".into());
            }
            self.0
                .notifications
                .lock()
                .unwrap()
                .push(notification.clone());
            Ok(())
        }
    }

    impl ClipboardProvider for Mock {
        fn availability(&self) -> Availability {
            Availability::Available
        }
        fn read(&self) -> keenu_platform::Result<Option<ClipboardContent>> {
            self.0.clipboard_reads.fetch_add(1, Ordering::Relaxed);
            Ok(self.0.clipboard.lock().unwrap().clone())
        }
        fn write(&self, content: &ClipboardContent) -> keenu_platform::Result<()> {
            self.0.written.lock().unwrap().push(content.clone());
            Ok(())
        }
    }

    impl FileShelfProvider for Mock {
        fn open(&self, path: &Path) -> keenu_platform::Result<()> {
            self.0.opened.lock().unwrap().push(path.to_path_buf());
            Ok(())
        }
        fn reveal(&self, _: &Path) -> keenu_platform::Result<()> {
            Ok(())
        }
        fn thumbnail(&self, _: &Path, _: u32) -> keenu_platform::Result<Option<Vec<u8>>> {
            Ok(None)
        }
    }

    impl VoiceProvider for Mock {
        fn availability(&self) -> Availability {
            Availability::Available
        }
        fn devices(&self) -> keenu_platform::Result<Vec<AudioDevice>> {
            Ok(vec![AudioDevice {
                id: String::new(),
                name: "Default".into(),
                is_default: true,
            }])
        }
        fn start(&self, _: &str) -> keenu_platform::Result<()> {
            self.0.recording.store(true, Ordering::Relaxed);
            self.0.recording_starts.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn stop(&self) -> keenu_platform::Result<Recording> {
            self.0.recording.store(false, Ordering::Relaxed);
            Ok(Recording::default())
        }
        fn cancel(&self) -> keenu_platform::Result<()> {
            self.0.recording.store(false, Ordering::Relaxed);
            Ok(())
        }
        fn recording(&self) -> bool {
            self.0.recording.load(Ordering::Relaxed)
        }
        fn listen_local(&self) -> keenu_platform::Result<String> {
            self.0.recording_starts.fetch_add(1, Ordering::Relaxed);
            Ok("hello".into())
        }
        fn speak_local(&self, _: &str) -> keenu_platform::Result<()> {
            Ok(())
        }
    }

    impl MonitorProvider for Mock {
        fn monitors(&self) -> keenu_platform::Result<Vec<Monitor>> {
            let monitor = |name: &str, primary| Monitor {
                name: name.into(),
                is_primary: primary,
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                scale_percent: 100,
            };
            Ok(vec![monitor("A", true), monitor("B", false)])
        }
    }

    impl UsageProvider for Mock {
        fn availability(&self) -> Availability {
            Availability::Available
        }
        fn foreground_app(&self) -> Option<String> {
            self.0.foreground_asks.fetch_add(1, Ordering::Relaxed);
            Some("code.exe".into())
        }
    }

    impl WindowLifecycleProvider for Mock {
        fn set_start_at_login(&self, enabled: bool) -> keenu_platform::Result<()> {
            self.0.start_at_login.store(enabled, Ordering::Relaxed);
            Ok(())
        }
        fn start_at_login(&self) -> keenu_platform::Result<bool> {
            Ok(self.0.start_at_login.load(Ordering::Relaxed))
        }
        fn fullscreen_active(&self) -> bool {
            false
        }
    }

    impl UpdateProvider for Mock {
        fn availability(&self) -> Availability {
            Availability::NotConfigured("This build has no update address.".into())
        }
        fn installable_kinds(&self) -> Vec<&'static str> {
            vec!["msi"]
        }
        fn verify_platform_signature(&self, _: &Path) -> keenu_platform::Result<()> {
            Err("unsigned".into())
        }
        fn install(&self, _: &Path) -> keenu_platform::Result<()> {
            panic!("nothing may be installed in these tests");
        }
    }

    struct NoWaker;
    impl Waker for NoWaker {
        fn wake(&self) {}
    }

    fn temp(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("keenu-app-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Paths { root }
    }

    fn platform(mock: &Mock) -> Platform {
        Platform {
            media: Arc::new(mock.clone()),
            notifier: Arc::new(mock.clone()),
            clipboard: Arc::new(mock.clone()),
            shelf: Arc::new(mock.clone()),
            voice: Arc::new(mock.clone()),
            monitors: Arc::new(mock.clone()),
            usage: Arc::new(mock.clone()),
            lifecycle: Arc::new(mock.clone()),
            updates: Arc::new(mock.clone()),
            credentials: Arc::new(MemoryStore::default()),
            open_url: |_| Ok(()),
            play_wav: |_| Ok(()),
        }
    }

    fn app(name: &str) -> (App, Mock) {
        let mock = Mock::default();
        let paths = temp(name);
        let settings = Settings {
            onboarded: true,
            ..Settings::default()
        };
        json_file::save(&paths.settings(), &settings).unwrap();
        (App::new(platform(&mock), paths, Arc::new(NoWaker)), mock)
    }

    fn request(id: &str) -> ApprovalRequest {
        ApprovalRequest {
            id: id.into(),
            agent: Agent::Claude,
            tool: "Bash".into(),
            detail: "npm test".into(),
            truncated: false,
            cwd: "C:\\code\\app".into(),
            project: "app".into(),
            description: String::new(),
            suggested_permissions: Vec::new(),
        }
    }

    type Answers = Arc<Mutex<Vec<Answer>>>;

    fn ask(app: &mut App, id: &str) -> Answers {
        ask_with(app, id, Box::new(|| false))
    }

    fn ask_with(app: &mut App, id: &str, abandoned: Box<dyn Fn() -> bool + Send>) -> Answers {
        let answers: Answers = Arc::default();
        let sink = answers.clone();
        app.handle().approval(
            request(id),
            Box::new(move |answer| sink.lock().unwrap().push(answer)),
            abandoned,
        );
        app.pump();
        answers
    }

    /// As if the request on screen has been there long enough to read.
    fn time_to_read(app: &mut App) {
        if let Some((_, since)) = app.shown_approval.as_mut() {
            *since -= ALLOW_ARMS_AFTER_MS;
        }
    }

    #[test]
    fn with_clipboard_history_off_the_clipboard_is_never_read() {
        let (mut app, mock) = app("clip-off");
        *mock.0.clipboard.lock().unwrap() = Some(ClipboardContent::Text("secret".into()));
        app.clipboard_changed();
        assert_eq!(mock.0.clipboard_reads.load(Ordering::Relaxed), 0);
        assert!(app.clips.items().is_empty());
        assert!(app.view().clips_note.contains("off"));
    }

    #[test]
    fn with_it_on_copies_are_kept_and_paused_stops_it() {
        let (mut app, mock) = app("clip-on");
        app.act(Action::Toggle(Setting::ClipboardHistory));
        *mock.0.clipboard.lock().unwrap() = Some(ClipboardContent::Text("first".into()));
        app.clipboard_changed();
        assert_eq!(app.clips.items()[0].clip.text(), Some("first"));

        app.act(Action::Toggle(Setting::ClipboardPaused));
        *mock.0.clipboard.lock().unwrap() = Some(ClipboardContent::Text("second".into()));
        app.clipboard_changed();
        assert_eq!(app.clips.items().len(), 1);
        assert_eq!(mock.0.clipboard_reads.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn what_an_app_marks_private_is_not_kept() {
        let (mut app, mock) = app("clip-private");
        app.act(Action::Toggle(Setting::ClipboardHistory));
        // The platform's read returns None for content marked private.
        *mock.0.clipboard.lock().unwrap() = None;
        app.clipboard_changed();
        assert!(app.clips.items().is_empty());
    }

    #[test]
    fn the_history_limit_and_retention_are_applied() {
        let (mut app, mock) = app("clip-limit");
        app.settings.clipboard.enabled = true;
        app.settings.clipboard.max_items = 3;
        for i in 0..5 {
            *mock.0.clipboard.lock().unwrap() = Some(ClipboardContent::Text(format!("clip {i}")));
            app.clipboard_changed();
        }
        assert_eq!(app.clips.items().len(), 3);
        app.settings.clipboard.retention_days = 1;
        app.maintain(now_ms() + 2 * 86_400_000);
        assert!(app.clips.items().is_empty());
    }

    #[test]
    fn copying_back_writes_to_the_clipboard_and_a_search_narrows_the_list() {
        let (mut app, mock) = app("clip-copy");
        app.settings.clipboard.enabled = true;
        for text in ["apple", "banana"] {
            *mock.0.clipboard.lock().unwrap() = Some(ClipboardContent::Text(text.into()));
            app.clipboard_changed();
        }
        let banana = app.clips.items()[0].id;
        app.act(Action::CopyClip(banana));
        assert_eq!(
            mock.0.written.lock().unwrap()[0],
            ClipboardContent::Text("banana".into())
        );

        app.act(Action::StartInput(Field::ClipboardSearch));
        for c in "APP".chars() {
            app.key(Key::Char(c));
        }
        assert_eq!(app.view().clips.len(), 1);
        assert_eq!(app.view().clips[0].preview, "apple");
        app.key(Key::Escape);
        assert_eq!(app.view().clips.len(), 2);
    }

    #[test]
    fn usage_is_not_looked_at_until_turned_on() {
        let (app, mock) = app("usage-off");
        app.start_workers();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(mock.0.foreground_asks.load(Ordering::Relaxed), 0);
        app.stopping.store(true, Ordering::Relaxed);
    }

    #[test]
    fn usage_stretches_are_dropped_if_turned_off_meanwhile() {
        let (mut app, _mock) = app("usage-late");
        app.handle()
            .send(Event::UsageStretch("code.exe".into(), 0, 5_000));
        app.pump();
        assert!(app.usage.sessions().is_empty());
        app.settings.usage.enabled = true;
        let now = now_ms();
        app.handle()
            .send(Event::UsageStretch("code.exe".into(), now - 5_000, now));
        app.pump();
        assert_eq!(app.usage.sessions().len(), 1);
        assert_eq!(app.view().usage[0].app, "code.exe");
    }

    #[test]
    fn voice_cannot_start_while_off_and_only_on_the_users_action() {
        let (mut app, mock) = app("voice");
        app.act(Action::VoiceToggle);
        assert_eq!(mock.0.recording_starts.load(Ordering::Relaxed), 0);
        assert!(!app.view().voice.enabled);

        app.act(Action::Toggle(Setting::VoiceEnabled));
        // Turning it on opens nothing either.
        assert_eq!(mock.0.recording_starts.load(Ordering::Relaxed), 0);

        app.settings.voice.stt_engine = SpeechEngine::Remote;
        app.act(Action::VoiceToggle);
        assert_eq!(mock.0.recording_starts.load(Ordering::Relaxed), 1);
        assert!(app.view().voice.recording);

        app.act(Action::VoiceCancel);
        assert!(!app.view().voice.recording);
    }

    #[test]
    fn turning_voice_off_stops_a_recording() {
        let (mut app, mock) = app("voice-off");
        app.settings.voice.enabled = true;
        app.settings.voice.stt_engine = SpeechEngine::Remote;
        app.act(Action::VoiceToggle);
        assert!(mock.0.recording.load(Ordering::Relaxed));
        app.act(Action::Toggle(Setting::VoiceEnabled));
        assert!(!mock.0.recording.load(Ordering::Relaxed));
    }

    #[test]
    fn an_approval_opens_the_island_and_allow_answers_once() {
        let (mut app, _mock) = app("approve");
        let answers = ask(&mut app, "r1");
        assert_eq!(app.view().approval.as_ref().unwrap().detail, "npm test");
        assert!(app.holds_open());
        time_to_read(&mut app);
        app.act(Action::ApprovalAllow("r1".into()));
        app.act(Action::ApprovalAllow("r1".into()));
        assert_eq!(*answers.lock().unwrap(), [Answer::Allow]);
        assert!(app.view().approval.is_none());
    }

    #[test]
    fn a_click_landing_as_the_request_appears_does_not_allow_it() {
        let (mut app, _mock) = app("arming");
        let answers = ask(&mut app, "r1");
        app.act(Action::ApprovalAllow("r1".into()));
        assert!(
            answers.lock().unwrap().is_empty(),
            "too soon to have read it"
        );
        assert!(app.view().approval.is_some(), "still asking");
        time_to_read(&mut app);
        app.act(Action::ApprovalAllow("r1".into()));
        assert_eq!(*answers.lock().unwrap(), [Answer::Allow]);
    }

    #[test]
    fn denying_needs_no_wait() {
        let (mut app, _mock) = app("deny-now");
        let answers = ask(&mut app, "r1");
        app.act(Action::ApprovalDeny("r1".into()));
        assert_eq!(*answers.lock().unwrap(), [Answer::Deny]);
    }

    #[test]
    fn a_new_request_does_not_inherit_keyboard_focus() {
        let (mut app, _mock) = app("focus-reset");
        app.act(Action::Expand);
        app.key(Key::Tab);
        assert!(app.view().focus.is_some());
        let answers = ask(&mut app, "r1");
        assert_eq!(app.view().focus, None);
        time_to_read(&mut app);
        app.key(Key::Enter);
        assert!(
            answers.lock().unwrap().is_empty(),
            "Enter alone answers nothing"
        );
    }

    #[test]
    fn an_allow_for_a_request_not_on_screen_does_nothing() {
        let (mut app, _mock) = app("wrong-id");
        let answers = ask(&mut app, "r1");
        time_to_read(&mut app);
        app.act(Action::ApprovalAllow("other".into()));
        assert!(answers.lock().unwrap().is_empty());
    }

    #[test]
    fn deny_and_the_terminal_are_their_own_answers() {
        let (mut app, _mock) = app("deny");
        let denied = ask(&mut app, "r1");
        app.act(Action::ApprovalDeny("r1".into()));
        assert_eq!(*denied.lock().unwrap(), [Answer::Deny]);
        let terminal = ask(&mut app, "r2");
        app.act(Action::ApprovalInTerminal("r2".into()));
        assert_eq!(*terminal.lock().unwrap(), [Answer::NoDecision]);
    }

    #[test]
    fn a_replayed_request_is_not_asked_again() {
        let (mut app, _mock) = app("replay");
        let first = ask(&mut app, "same");
        time_to_read(&mut app);
        app.act(Action::ApprovalAllow("same".into()));
        assert_eq!(*first.lock().unwrap(), [Answer::Allow]);
        let replayed = ask(&mut app, "same");
        assert_eq!(
            *replayed.lock().unwrap(),
            [Answer::NoDecision],
            "never reuses the allow"
        );
        assert!(app.view().approval.is_none());
    }

    #[test]
    fn no_answer_in_time_goes_back_to_the_terminal_never_allows() {
        let (mut app, _mock) = app("timeout");
        let answers = ask(&mut app, "r1");
        app.approvals[0].deadline_ms = 0;
        app.tick();
        assert_eq!(*answers.lock().unwrap(), [Answer::NoDecision]);
        assert!(app.view().approval.is_none());
    }

    #[test]
    fn a_request_answered_in_the_terminal_goes_away() {
        let (mut app, _mock) = app("abandoned");
        let answers = ask_with(&mut app, "r1", Box::new(|| true));
        app.tick();
        assert_eq!(*answers.lock().unwrap(), [Answer::NoDecision]);
    }

    #[test]
    fn escape_on_an_approval_is_not_an_allow() {
        let (mut app, _mock) = app("escape");
        let answers = ask(&mut app, "r1");
        app.key(Key::Escape);
        assert_eq!(*answers.lock().unwrap(), [Answer::NoDecision]);
    }

    #[test]
    fn requests_wait_their_turn() {
        let (mut app, _mock) = app("queue");
        let first = ask(&mut app, "r1");
        let second = ask(&mut app, "r2");
        assert_eq!(app.view().approval.as_ref().unwrap().id, "r1");
        app.act(Action::ApprovalDeny("r1".into()));
        assert_eq!(app.view().approval.as_ref().unwrap().id, "r2");
        time_to_read(&mut app);
        app.act(Action::ApprovalAllow("r2".into()));
        assert_eq!(*first.lock().unwrap(), [Answer::Deny]);
        assert_eq!(*second.lock().unwrap(), [Answer::Allow]);
    }

    #[test]
    fn shutting_down_sends_waiting_hooks_back_to_their_terminals() {
        let (mut app, _mock) = app("shutdown");
        let answers = ask(&mut app, "r1");
        app.shut_down();
        assert_eq!(*answers.lock().unwrap(), [Answer::NoDecision]);
    }

    #[test]
    fn a_due_reminder_notifies_once_and_is_gone() {
        let (mut app, mock) = app("reminder");
        app.reminders.add("Call back", 1000);
        app.tick();
        app.tick();
        let notes = mock.0.notifications.lock().unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].body, "Call back");
        assert!(app.reminders.items().is_empty());
    }

    #[test]
    fn a_failed_notification_is_tried_again_then_dropped() {
        let (mut app, mock) = app("notify-retry");
        mock.0.notify_fails.store(true, Ordering::Relaxed);
        app.handle().notify("k", "Title", "Body");
        for _ in 0..5 {
            app.tick();
        }
        assert!(app.notifications.is_empty());
        mock.0.notify_fails.store(false, Ordering::Relaxed);
    }

    #[test]
    fn repeated_notifications_show_once() {
        let (mut app, mock) = app("notify-dedupe");
        for _ in 0..3 {
            app.handle().notify("same", "Title", "Body");
        }
        app.tick();
        assert_eq!(mock.0.notifications.lock().unwrap().len(), 1);
    }

    #[test]
    fn dropped_files_go_on_the_shelf_and_open() {
        let (mut app, mock) = app("shelf");
        let file = app.paths.root.join("report.txt");
        std::fs::write(&file, "hello").unwrap();
        app.files_dropped(vec![file.clone(), PathBuf::from("relative.txt")]);
        assert_eq!(app.view().page, Page::Shelf);
        assert_eq!(app.view().shelf.len(), 1);
        assert!(app.view().status.contains("relative.txt"));
        app.act(Action::OpenShelf(0));
        assert_eq!(mock.0.opened.lock().unwrap()[0], file);
        std::fs::remove_file(&file).unwrap();
        app.maintain(now_ms());
        app.refresh_view();
        assert!(app.view().shelf[0].missing);
    }

    #[test]
    fn local_lyrics_are_found_and_follow_the_position() {
        let (mut app, _mock) = app("lyrics");
        let folder = app.paths.root.join("lrc");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("Artist - Song.lrc"),
            "[00:01.00]one\n[00:02.00]two\n",
        )
        .unwrap();
        app.settings.lyrics.folder = folder.display().to_string();
        app.on_sessions(vec![MediaSession {
            id: "app".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            state: PlaybackState::Playing,
            position_ms: 1_500,
            duration_ms: 10_000,
            ..MediaSession::default()
        }]);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while app.lyrics.is_none() && std::time::Instant::now() < deadline {
            app.pump();
            std::thread::sleep(Duration::from_millis(10));
        }
        app.refresh_view();
        assert_eq!(app.view().lyrics.active, Some(0));
        assert_eq!(app.view().lyrics.lines[0].1, "one");
    }

    #[test]
    fn lyrics_from_an_old_track_are_ignored() {
        let (mut app, _mock) = app("lyrics-stale");
        app.track = "current".into();
        let line = LyricLine {
            time_ms: 0,
            text: "old".into(),
        };
        app.on_event(Event::Lyrics(
            "old-track".into(),
            Some(vec![line]),
            String::new(),
        ));
        assert!(app.lyrics.is_none());
    }

    #[test]
    fn with_no_online_lyrics_nothing_is_looked_up() {
        let (app, _mock) = app("lyrics-off");
        assert!(!app.settings.lyrics.online);
    }

    #[test]
    fn the_first_run_starts_with_everything_off_and_can_turn_things_on() {
        let mock = Mock::default();
        let mut app = App::new(platform(&mock), temp("onboard"), Arc::new(NoWaker));
        assert!(app.view().onboarding.is_some());
        assert!(!app.settings.clipboard.enabled && !app.settings.usage.enabled);
        app.act(Action::OnboardingNext);
        app.act(Action::Toggle(Setting::ClipboardHistory));
        assert!(app.view().onboarding.as_ref().unwrap().switches[0].on);
        app.act(Action::OnboardingNext);
        app.act(Action::OnboardingNext);
        assert!(app.view().onboarding.is_none());
        let saved = load_settings(&app.paths);
        assert!(saved.onboarded && saved.clipboard.enabled);
    }

    #[test]
    fn settings_survive_a_restart() {
        let mock = Mock::default();
        let paths = temp("restart");
        let mut app = App::new(platform(&mock), paths.clone(), Arc::new(NoWaker));
        app.act(Action::OnboardingSkip);
        app.act(Action::Toggle(Setting::ClipboardHistory));
        app.shut_down();
        let again = App::new(platform(&mock), paths, Arc::new(NoWaker));
        assert!(again.settings.clipboard.enabled);
        assert!(again.view().onboarding.is_none());
    }

    #[test]
    fn start_at_login_goes_through_the_system() {
        let (mut app, mock) = app("login");
        app.act(Action::Toggle(Setting::StartAtLogin));
        assert!(mock.0.start_at_login.load(Ordering::Relaxed));
        assert!(app.settings.window.start_at_login);
    }

    #[test]
    fn switches_that_cannot_work_say_why_rather_than_pretending() {
        let (app, _mock) = app("unavailable");
        let updates = app
            .view()
            .settings
            .switches
            .iter()
            .find(|row| row.setting == Setting::UpdateChecks)
            .unwrap();
        assert!(
            updates
                .unavailable
                .as_deref()
                .unwrap()
                .contains("no update address")
        );
    }

    #[test]
    fn secrets_go_to_the_credential_store_and_never_to_settings() {
        let (mut app, _mock) = app("secrets");
        app.act(Action::StartInput(Field::VoiceEndpoint));
        for c in "https://voice.example/v1".chars() {
            app.key(Key::Char(c));
        }
        app.key(Key::Enter);
        assert_eq!(app.view().input.as_ref().unwrap().field, Field::VoiceApiKey);
        for c in "sk-very-secret".chars() {
            app.key(Key::Char(c));
        }
        app.key(Key::Enter);
        let saved = std::fs::read_to_string(app.paths.settings()).unwrap();
        assert!(saved.contains("voice.example"));
        assert!(!saved.contains("sk-very-secret"));
        assert_eq!(
            app.platform
                .credentials
                .get(CREDENTIAL_VOICE)
                .unwrap()
                .as_deref(),
            Some("sk-very-secret")
        );
    }

    #[test]
    fn an_insecure_caldav_or_voice_address_is_refused() {
        let (mut app, _mock) = app("insecure");
        app.act(Action::AddCalDavAccount);
        for c in "http://dav.example.org".chars() {
            app.key(Key::Char(c));
        }
        app.key(Key::Enter);
        assert!(app.view().input.is_none());
        assert!(app.view().status.contains("https://"));
        app.act(Action::StartInput(Field::VoiceEndpoint));
        for c in "http://voice.example.org".chars() {
            app.key(Key::Char(c));
        }
        app.key(Key::Enter);
        assert!(app.settings.voice.endpoint.is_empty());
    }

    #[test]
    fn deleting_everything_needs_a_second_click_and_removes_secrets_too() {
        let (mut app, _mock) = app("delete");
        app.settings.usage.enabled = true;
        app.usage.record("code.exe", 0, 5_000);
        app.platform
            .credentials
            .set(CREDENTIAL_VOICE, "key")
            .unwrap();
        app.act(Action::DeleteAllData);
        assert_eq!(app.usage.sessions().len(), 1, "not yet");
        app.act(Action::ConfirmDeleteAllData);
        assert!(app.usage.sessions().is_empty());
        assert_eq!(
            app.platform.credentials.get(CREDENTIAL_VOICE).unwrap(),
            None
        );
    }

    #[test]
    fn without_oauth_clients_signing_in_says_what_is_missing() {
        let (mut app, _mock) = app("oauth");
        if google::provider().is_none() {
            app.act(Action::AddGoogleAccount);
            assert!(app.view().status.contains("KEENU_GOOGLE_CLIENT_ID"));
        }
    }

    #[test]
    fn choosing_a_display_goes_round_them() {
        let (mut app, _mock) = app("monitor");
        app.act(Action::SelectMonitor);
        assert_eq!(app.monitor_name(), "B");
        app.act(Action::SelectMonitor);
        assert_eq!(app.monitor_name(), "A");
    }

    #[test]
    fn the_keyboard_reaches_every_button_and_presses_it() {
        let (mut app, _mock) = app("keyboard");
        app.act(Action::Expand);
        app.key(Key::Tab);
        assert_eq!(app.view().focus, Some(0));
        app.key(Key::ShiftTab);
        let last = app.scene().hits.len() - 1;
        assert_eq!(app.view().focus, Some(last));
        // The second tab, by keyboard alone.
        app.key(Key::Tab);
        app.key(Key::Tab);
        app.key(Key::Enter);
        assert_eq!(app.view().page, Page::Calendar);
        app.key(Key::Escape);
        assert!(!app.view().expanded);
    }

    #[test]
    fn it_tells_a_host_to_quit() {
        let (mut app, _mock) = app("quit");
        assert_eq!(app.act(Action::Quit), Effect::Quit);
    }

    #[test]
    fn hooks_are_installed_where_the_environment_says() {
        // Only through what this test sets; a developer's real settings are never touched.
        let (mut app, _mock) = app("hooks-view");
        app.refresh_view();
        assert_eq!(app.view().agents.len(), 2);
    }

    #[test]
    fn uris_round_trip() {
        let path = if cfg!(windows) {
            PathBuf::from("C:\\Users\\me\\a b+c.txt")
        } else {
            PathBuf::from("/home/me/a b+c.txt")
        };
        assert_eq!(uri_to_path(&path_to_uri(&path)).unwrap(), path);
    }
}
