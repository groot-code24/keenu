// SPDX-License-Identifier: GPL-3.0-or-later

//! What should be on the island, and how that turns into a [`Scene`].

use crate::icons::Icon;
use crate::scene::palette::*;
use crate::scene::{Align, Hit, Op, Rect, Scene, TextStyle, format_clock, format_duration};

pub const COMPACT_W: f32 = 232.0;
pub const COMPACT_H: f32 = 34.0;
pub const EXPANDED_W: f32 = 640.0;
pub const EXPANDED_H: f32 = 340.0;
const COMPACT_RADIUS: f32 = 14.0;
const EXPANDED_RADIUS: f32 = 32.0;
const PAD: f32 = 28.0;
const HEADER_H: f32 = 48.0;
const ROW_H: f32 = 40.0;

/// The island's pages, in tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Media,
    Calendar,
    Clipboard,
    Shelf,
    Voice,
    Agents,
    Usage,
    Settings,
}

impl Page {
    pub const ALL: [Self; 8] = [
        Self::Media,
        Self::Calendar,
        Self::Clipboard,
        Self::Shelf,
        Self::Voice,
        Self::Agents,
        Self::Usage,
        Self::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Media => "Music",
            Self::Calendar => "Calendar",
            Self::Clipboard => "Clipboard",
            Self::Shelf => "Shelf",
            Self::Voice => "Voice",
            Self::Agents => "Agents",
            Self::Usage => "Usage",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::Media => Icon::Music,
            Self::Calendar => Icon::Calendar,
            Self::Clipboard => Icon::Clipboard,
            Self::Shelf => Icon::Shelf,
            Self::Voice => Icon::Mic,
            Self::Agents => Icon::Agent,
            Self::Usage => Icon::Usage,
            Self::Settings => Icon::Settings,
        }
    }
}

/// A setting with a switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    ClipboardHistory,
    ClipboardPaused,
    UsageTracking,
    UsagePaused,
    VoiceEnabled,
    OnlineLyrics,
    UpdateChecks,
    UpdateAutoInstall,
    PrereleaseChannel,
    StartAtLogin,
    HideInFullscreen,
    Notifications,
    ClaudeHooks,
    CodexHooks,
    Assistant,
    AssistantAskAll,
    AssistantSpeak,
    AssistantWakeWord,
    AssistantMemory,
}

/// Text the user can type, one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    ClipboardSearch,
    NewEvent,
    CalDavServer,
    CalDavUser,
    CalDavPassword,
    VoiceEndpoint,
    VoiceApiKey,
    UsageCategory,
    AssistantRequest,
    AssistantApiBase,
    AssistantApiModel,
    AssistantApiKey,
    XaiKey,
    MemoryAdd,
    MemoryImport,
    ProjectName,
    Briefing,
    ConnectorName,
    ConnectorTarget,
    ConnectorToken,
}

impl Field {
    pub fn label(self) -> &'static str {
        match self {
            Self::ClipboardSearch => "Search",
            Self::NewEvent => "New event, such as Lunch 1:00 pm",
            Self::CalDavServer => "CalDAV server address (https://...)",
            Self::CalDavUser => "User name",
            Self::CalDavPassword => "Password (kept in Credential Manager)",
            Self::VoiceEndpoint => "OpenAI-compatible address (https://.../v1)",
            Self::VoiceApiKey => "API key (kept in Credential Manager)",
            Self::UsageCategory => "Category for this app",
            Self::AssistantRequest => "Ask Keenu, or start with research for a report",
            Self::AssistantApiBase => {
                "openai, anthropic, xai, ollama, or an address (https://.../v1)"
            }
            Self::AssistantApiModel => "Model name",
            Self::AssistantApiKey => "API key (kept in the system's credential store)",
            Self::XaiKey => "xAI API key for live search (kept in the credential store)",
            Self::MemoryAdd => "Something to remember",
            Self::MemoryImport => "File to bring in: a text file, or ChatGPT's conversations.json",
            Self::ProjectName => "Project name, then its notes after a colon; empty for none",
            Self::Briefing => "Briefing time, such as 7:30; empty for none",
            Self::ConnectorName => "Connector's name",
            Self::ConnectorTarget => "Its address (https://...), or the command that starts it",
            Self::ConnectorToken => "Its token, if it needs one (kept in the credential store)",
        }
    }

    /// Shown as dots.
    pub fn secret(self) -> bool {
        matches!(
            self,
            Self::CalDavPassword
                | Self::VoiceApiKey
                | Self::AssistantApiKey
                | Self::XaiKey
                | Self::ConnectorToken
        )
    }
}

/// What a click or key can do. The host carries each one out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Expand,
    Collapse,
    SelectPage(Page),
    Dismiss,
    PlayPause,
    NextTrack,
    PreviousTrack,
    /// Thousandths of the way through the track.
    SeekFraction(u32),
    SeekToLine(usize),
    SelectSession(usize),
    CopyClip(u64),
    PinClip(u64, bool),
    DeleteClip(u64),
    ClearClips,
    OpenShelf(usize),
    RevealShelf(usize),
    PinShelf(usize, bool),
    RemoveShelf(usize),
    ClearShelf,
    StartInput(Field),
    AddCalDavAccount,
    AddGoogleAccount,
    AddMicrosoftAccount,
    RemoveAccount(usize),
    DeleteEvent(usize),
    SyncCalendars,
    VoiceToggle,
    VoiceCancel,
    NextMicrophone,
    ApprovalAllow(String),
    ApprovalDeny(String),
    ApprovalInTerminal(String),
    ClearUsage,
    ExportUsage,
    DeleteUsageApp(usize),
    CategorizeUsageApp(usize),
    Toggle(Setting),
    CheckUpdates,
    InstallUpdate,
    ExportData,
    DeleteAllData,
    ConfirmDeleteAllData,
    ResetSettings,
    OpenDataFolder,
    AssistantStop,
    NextAssistantRoute,
    ConnectChatGpt,
    DisconnectChatGpt,
    NextAssistantTone,
    NextProject,
    ForgetMemory,
    OpenActionLog,
    NextConnectorAccess(usize),
    RemoveConnector(usize),
    OnboardingNext,
    OnboardingSkip,
    SelectMonitor,
    Quit,
}

/// One media session's worth of what the music page shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaView {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_seek: bool,
    pub artwork_key: Option<String>,
}

/// Lyrics in reach of the current line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LyricsView {
    /// `(index in the song, text)`.
    pub lines: Vec<(usize, String)>,
    pub active: Option<usize>,
    /// Why there are none, when there are none.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipRow {
    pub id: u64,
    pub preview: String,
    pub kind: String,
    pub pinned: bool,
    pub image_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfRow {
    pub name: String,
    pub detail: String,
    pub pinned: bool,
    pub missing: bool,
    pub thumbnail_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaRow {
    pub deletable: bool,
    pub day: String,
    pub time: String,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    pub label: String,
    pub status: String,
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageRow {
    pub app: String,
    pub category: String,
    pub duration_ms: i64,
    /// Of the most used, for the bar.
    pub fraction: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalView {
    pub id: String,
    pub agent: String,
    pub project: String,
    pub tool: String,
    pub detail: String,
    pub truncated: bool,
    pub cwd: String,
    pub permissions: Vec<String>,
    pub seconds_left: u32,
    /// Keenu's own assistant asks: there is no terminal to leave it to.
    pub own: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    pub name: String,
    pub setting: Setting,
    pub installed: bool,
    pub note: String,
}

/// The state of the voice page. `recording` is what the microphone really is doing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VoiceView {
    pub enabled: bool,
    pub recording: bool,
    pub busy: bool,
    pub microphone: String,
    pub heard: String,
    pub answer: String,
    pub error: String,
    pub engine: String,
    /// The assistant is on, so what is said or typed goes to it.
    pub assistant: bool,
    /// What it is doing now, newest last.
    pub steps: Vec<String>,
    /// Which AI answers.
    pub route: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchRow {
    pub title: String,
    pub detail: String,
    pub setting: Setting,
    pub on: bool,
    /// Why it cannot be changed here; the switch is shown off and does nothing.
    pub unavailable: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsView {
    pub switches: Vec<SwitchRow>,
    pub version: String,
    pub update_status: String,
    pub update_ready: Option<String>,
    pub update_available: bool,
    pub monitor: String,
    pub confirming_delete: bool,
    /// The assistant's rows: words, and the buttons beside them.
    pub assistant_rows: Vec<ButtonRow>,
}

/// A row of Settings with words and buttons, drawn as the other rows are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonRow {
    pub text: String,
    pub buttons: Vec<(String, Action)>,
}

/// A line of typing in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputView {
    pub field: Field,
    pub text: String,
}

/// The first-run screens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingView {
    pub step: usize,
    pub steps: usize,
    pub title: String,
    pub body: String,
    pub switches: Vec<SwitchRow>,
}

/// Everything on the island right now.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub expanded: bool,
    pub page: Page,
    pub time: String,
    pub date: String,
    pub media: Option<MediaView>,
    pub session_count: usize,
    pub media_note: String,
    pub lyrics: LyricsView,
    pub clips: Vec<ClipRow>,
    pub clips_note: String,
    pub clip_query: String,
    pub shelf: Vec<ShelfRow>,
    pub agenda: Vec<AgendaRow>,
    pub accounts: Vec<AccountRow>,
    pub calendar_note: String,
    pub usage: Vec<UsageRow>,
    pub usage_note: String,
    pub voice: VoiceView,
    pub approval: Option<ApprovalView>,
    pub agents: Vec<AgentRow>,
    pub settings: SettingsView,
    pub input: Option<InputView>,
    pub onboarding: Option<OnboardingView>,
    /// A short message at the bottom of a page: what just happened or failed.
    pub status: String,
    /// Rows scrolled past on the current page.
    pub scroll: usize,
    /// Which clickable region has keyboard focus, in order of the scene's regions.
    pub focus: Option<usize>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            expanded: false,
            page: Page::Media,
            time: String::new(),
            date: String::new(),
            media: None,
            session_count: 0,
            media_note: String::new(),
            lyrics: LyricsView::default(),
            clips: Vec::new(),
            clips_note: String::new(),
            clip_query: String::new(),
            shelf: Vec::new(),
            agenda: Vec::new(),
            accounts: Vec::new(),
            calendar_note: String::new(),
            usage: Vec::new(),
            usage_note: String::new(),
            voice: VoiceView::default(),
            approval: None,
            agents: Vec::new(),
            settings: SettingsView::default(),
            input: None,
            onboarding: None,
            status: String::new(),
            scroll: 0,
            focus: None,
        }
    }
}

fn style(size: f32, color: crate::scene::Color) -> TextStyle {
    TextStyle {
        size,
        bold: false,
        color,
        align: Align::Left,
        wrap: false,
        mono: false,
    }
}

/// Builds a frame, drawing as it goes.
struct Frame {
    scene: Scene<Action>,
}

impl Frame {
    fn fill(&mut self, rect: Rect, radius: f32, color: crate::scene::Color) {
        self.scene.ops.push(Op::Fill {
            rect,
            radius,
            color,
        });
    }

    fn text(&mut self, rect: Rect, text: impl Into<String>, style: TextStyle) {
        self.scene.ops.push(Op::Text {
            rect,
            text: text.into(),
            style,
        });
    }

    fn icon(&mut self, rect: Rect, icon: Icon, color: crate::scene::Color) {
        self.scene.ops.push(Op::Icon { rect, icon, color });
    }

    fn hit(&mut self, rect: Rect, action: Action, label: impl Into<String>) {
        self.scene.hits.push(Hit {
            rect,
            action,
            label: label.into(),
        });
    }

    fn icon_button(
        &mut self,
        rect: Rect,
        icon: Icon,
        color: crate::scene::Color,
        action: Action,
        label: &str,
    ) {
        let inset = (rect.h - 20.0).max(0.0) / 2.0;
        self.icon(rect.inset(inset), icon, color);
        self.hit(rect, action, label);
    }

    /// A pill button, sized to fit its words. Returns where the next may go, to its left.
    fn pill_from_right(
        &mut self,
        right: f32,
        y: f32,
        text: &str,
        primary: bool,
        action: Action,
    ) -> f32 {
        let width = 26.0 + text.chars().count() as f32 * 7.4;
        let rect = Rect::new(right - width, y, width, 30.0);
        self.pill(rect, text, primary, action);
        rect.x - 8.0
    }

    fn pill(&mut self, rect: Rect, text: &str, primary: bool, action: Action) {
        let (back, front) = if primary {
            (TEXT, ISLAND)
        } else {
            (BUTTON, TEXT)
        };
        self.fill(rect, rect.h / 2.0, back);
        self.text(
            rect,
            text,
            TextStyle {
                bold: true,
                align: Align::Center,
                ..style(13.0, front)
            },
        );
        self.hit(rect, action, text);
    }

    fn note(&mut self, rect: Rect, text: &str) {
        self.text(
            rect,
            text,
            TextStyle {
                wrap: true,
                ..style(13.0, MUTED)
            },
        );
    }

    fn switch(&mut self, row: &SwitchRow, y: f32, width: f32) {
        let on = row.on && row.unavailable.is_none();
        let x = PAD;
        self.text(
            Rect::new(x, y, width - 60.0, 18.0),
            row.title.as_str(),
            style(
                13.0,
                if row.unavailable.is_some() {
                    MUTED
                } else {
                    TEXT
                },
            ),
        );
        let detail = row.unavailable.as_deref().unwrap_or(&row.detail);
        self.text(
            Rect::new(x, y + 18.0, width - 60.0, 16.0),
            detail,
            style(11.0, MUTED),
        );
        let track = Rect::new(x + width - 44.0, y + 6.0, 38.0, 22.0);
        self.fill(track, 11.0, if on { GOOD } else { BUTTON });
        let knob = Rect::new(
            if on { track.x + 18.0 } else { track.x + 2.0 },
            track.y + 2.0,
            18.0,
            18.0,
        );
        self.fill(
            knob,
            9.0,
            if row.unavailable.is_some() {
                MUTED
            } else {
                TEXT
            },
        );
        if row.unavailable.is_none() {
            let state = if on { "on" } else { "off" };
            self.hit(
                Rect::new(x, y, width, 34.0),
                Action::Toggle(row.setting),
                format!("{}: {state}", row.title),
            );
        }
    }
}

/// The size of the window for `view`.
pub fn size_of(view: &View) -> (f32, f32) {
    if view.expanded || view.approval.is_some() || view.onboarding.is_some() {
        (EXPANDED_W, EXPANDED_H)
    } else {
        (COMPACT_W, COMPACT_H)
    }
}

/// Turns `view` into a frame.
pub fn build(view: &View) -> Scene<Action> {
    let (width, height) = size_of(view);
    let expanded = width > COMPACT_W;
    let mut frame = Frame {
        scene: Scene::new(width, height),
    };
    frame.scene.ops.push(Op::Island {
        rect: Rect::new(0.0, 0.0, width, height),
        radius: if expanded {
            EXPANDED_RADIUS
        } else {
            COMPACT_RADIUS
        },
        color: ISLAND,
    });
    if !expanded {
        compact(&mut frame, view);
    } else if let Some(approval) = &view.approval {
        approval_page(&mut frame, approval);
    } else if let Some(onboarding) = &view.onboarding {
        onboarding_page(&mut frame, onboarding);
    } else {
        header(&mut frame, view);
        let body = Rect::new(
            PAD,
            HEADER_H + 8.0,
            width - PAD * 2.0,
            height - HEADER_H - 22.0,
        );
        match view.page {
            Page::Media => media_page(&mut frame, view, body),
            Page::Calendar => calendar_page(&mut frame, view, body),
            Page::Clipboard => clipboard_page(&mut frame, view, body),
            Page::Shelf => shelf_page(&mut frame, view, body),
            Page::Voice => voice_page(&mut frame, view, body),
            Page::Agents => agents_page(&mut frame, view, body),
            Page::Usage => usage_page(&mut frame, view, body),
            Page::Settings => settings_page(&mut frame, view, body),
        }
        if !view.status.is_empty() {
            frame.text(
                Rect::new(PAD, height - 26.0, width - PAD * 2.0, 18.0),
                view.status.as_str(),
                style(11.0, WARN),
            );
        }
        if let Some(input) = &view.input {
            input_bar(&mut frame, input, width);
        }
    }
    if let Some(hit) = view.focus.and_then(|focus| frame.scene.hits.get(focus)) {
        let rect = hit.rect;
        frame.scene.ops.push(Op::Outline {
            rect,
            radius: 8.0,
            color: FOCUS,
            width: 2.0,
        });
    }
    frame.scene
}

fn compact(frame: &mut Frame, view: &View) {
    let whole = Rect::new(0.0, 0.0, COMPACT_W, COMPACT_H);
    let time_w = 66.0;
    let body = Rect::new(14.0, 7.0, COMPACT_W - time_w - 28.0, 20.0);
    if let Some(media) = &view.media {
        frame.icon(
            Rect::new(body.x, 9.0, 16.0, 16.0),
            if media.playing {
                Icon::Music
            } else {
                Icon::Pause
            },
            TEXT,
        );
        frame.text(
            Rect::new(body.x + 22.0, body.y, body.w - 22.0, body.h),
            media.title.as_str(),
            style(12.0, TEXT),
        );
    } else {
        frame.text(body, view.date.as_str(), style(12.0, MUTED));
    }
    frame.text(
        Rect::new(COMPACT_W - time_w - 12.0, 7.0, time_w, 20.0),
        view.time.as_str(),
        TextStyle {
            bold: true,
            align: Align::Right,
            ..style(13.0, TEXT)
        },
    );
    frame.hit(whole, Action::Expand, "Open Keenu");
}

fn header(frame: &mut Frame, view: &View) {
    let mut x = PAD - 4.0;
    for page in Page::ALL {
        let selected = view.page == page;
        let label_w = if selected {
            page.title().chars().count() as f32 * 7.6 + 6.0
        } else {
            0.0
        };
        let rect = Rect::new(x, 10.0, 34.0 + label_w, 30.0);
        if selected {
            frame.fill(rect, 15.0, BUTTON);
            frame.text(
                Rect::new(x + 30.0, 10.0, label_w, 30.0),
                page.title(),
                TextStyle {
                    bold: true,
                    ..style(13.0, TEXT)
                },
            );
        }
        frame.icon(
            Rect::new(x + 8.0, 16.0, 18.0, 18.0),
            page.icon(),
            if selected { TEXT } else { MUTED },
        );
        frame.hit(rect, Action::SelectPage(page), page.title());
        x += rect.w + 2.0;
    }
    let right = EXPANDED_W - PAD;
    frame.text(
        Rect::new(right - 110.0, 14.0, 110.0, 22.0),
        format!("{}  {}", view.date, view.time),
        TextStyle {
            align: Align::Right,
            ..style(12.0, MUTED)
        },
    );
}

fn media_page(frame: &mut Frame, view: &View, body: Rect) {
    let Some(media) = &view.media else {
        let message = if view.media_note.is_empty() {
            "Nothing is playing. Start something in Spotify, a browser or any media app."
        } else {
            view.media_note.as_str()
        };
        frame.note(Rect::new(body.x, body.y + 40.0, body.w, 60.0), message);
        return;
    };
    let art = Rect::new(body.x, body.y, 120.0, 120.0);
    match &media.artwork_key {
        Some(key) => frame.scene.ops.push(Op::Image {
            rect: art,
            radius: 14.0,
            key: key.clone(),
        }),
        None => {
            frame.fill(art, 14.0, BUTTON);
            frame.icon(art.inset(36.0), Icon::Music, MUTED);
        }
    }
    let text_x = art.right() + 20.0;
    let text_w = body.right() - text_x;
    frame.text(
        Rect::new(text_x, body.y, text_w, 24.0),
        media.title.as_str(),
        TextStyle {
            bold: true,
            ..style(17.0, TEXT)
        },
    );
    let by = if media.album.is_empty() {
        media.artist.clone()
    } else {
        format!("{} — {}", media.artist, media.album)
    };
    frame.text(
        Rect::new(text_x, body.y + 26.0, text_w, 18.0),
        by,
        style(13.0, MUTED),
    );
    // A scrubber for what can be scrubbed; a session that cannot seek shows only where it is.
    let rail = Rect::new(text_x + 44.0, body.y + 58.0, text_w - 92.0, 6.0);
    frame.text(
        Rect::new(text_x, body.y + 52.0, 42.0, 18.0),
        format_clock(media.position_ms),
        style(11.0, MUTED),
    );
    frame.text(
        Rect::new(rail.right() + 6.0, body.y + 52.0, 42.0, 18.0),
        format_clock(media.duration_ms),
        TextStyle {
            align: Align::Right,
            ..style(11.0, MUTED)
        },
    );
    frame.fill(rail, 3.0, RAIL);
    if media.duration_ms > 0 {
        let fraction = (media.position_ms as f32 / media.duration_ms as f32).clamp(0.0, 1.0);
        frame.fill(
            Rect::new(rail.x, rail.y, rail.w * fraction, rail.h),
            3.0,
            TEXT,
        );
        if media.can_seek {
            // Ten slices of the rail to click on.
            for step in 0..10 {
                let slice = Rect::new(
                    rail.x + rail.w * step as f32 / 10.0,
                    rail.y - 8.0,
                    rail.w / 10.0,
                    22.0,
                );
                frame.hit(
                    slice,
                    Action::SeekFraction((step * 100 + 50) as u32),
                    format!("Seek to {}%", step * 10 + 5),
                );
            }
        }
    }
    let controls_y = body.y + 76.0;
    let centre = text_x + text_w / 2.0;
    let dim = |on: bool| if on { TEXT } else { FAINT };
    frame.icon_button(
        Rect::new(centre - 74.0, controls_y, 40.0, 40.0),
        Icon::Previous,
        dim(media.can_previous),
        Action::PreviousTrack,
        "Previous",
    );
    frame.icon_button(
        Rect::new(centre - 20.0, controls_y, 40.0, 40.0),
        if media.playing {
            Icon::Pause
        } else {
            Icon::Play
        },
        TEXT,
        Action::PlayPause,
        if media.playing { "Pause" } else { "Play" },
    );
    frame.icon_button(
        Rect::new(centre + 34.0, controls_y, 40.0, 40.0),
        Icon::Next,
        dim(media.can_next),
        Action::NextTrack,
        "Next",
    );

    if view.session_count > 1 {
        frame.pill(
            Rect::new(body.right() - 96.0, controls_y + 6.0, 96.0, 28.0),
            &format!("{} sessions", view.session_count),
            false,
            Action::SelectSession(1),
        );
    }
    lyrics_block(
        frame,
        &view.lyrics,
        Rect::new(
            body.x,
            art.bottom() + 12.0,
            body.w,
            body.bottom() - art.bottom() - 12.0,
        ),
    );
}

fn lyrics_block(frame: &mut Frame, lyrics: &LyricsView, area: Rect) {
    if lyrics.lines.is_empty() {
        if !lyrics.note.is_empty() {
            frame.text(area, lyrics.note.as_str(), style(12.0, FAINT));
        }
        return;
    }
    let mut y = area.y;
    for (index, line) in &lyrics.lines {
        let height = if Some(*index) == lyrics.active {
            26.0
        } else {
            20.0
        };
        if y + height > area.bottom() {
            break;
        }
        let active = Some(*index) == lyrics.active;
        let rect = Rect::new(area.x, y, area.w, height);
        frame.text(
            rect,
            line.as_str(),
            TextStyle {
                bold: active,
                ..style(
                    if active { 16.0 } else { 13.0 },
                    if active { TEXT } else { FAINT },
                )
            },
        );
        frame.hit(rect, Action::SeekToLine(*index), format!("Jump to: {line}"));
        y += height;
    }
}

fn calendar_page(frame: &mut Frame, view: &View, body: Rect) {
    let mut right = body.right();
    right = frame.pill_from_right(right, body.y, "Sync", false, Action::SyncCalendars);
    right = frame.pill_from_right(
        right,
        body.y,
        "Microsoft",
        false,
        Action::AddMicrosoftAccount,
    );
    right = frame.pill_from_right(right, body.y, "Google", false, Action::AddGoogleAccount);
    right = frame.pill_from_right(right, body.y, "CalDAV", false, Action::AddCalDavAccount);
    frame.pill_from_right(
        right,
        body.y,
        "Add",
        true,
        Action::StartInput(Field::NewEvent),
    );
    if view.accounts.is_empty() {
        frame.text(
            Rect::new(body.x, body.y + 6.0, 170.0, 18.0),
            "No account connected",
            style(12.0, MUTED),
        );
    }
    // Each account, its state, and how to take it off.
    let mut list_y = body.y + 36.0;
    for (index, account) in view.accounts.iter().enumerate() {
        frame.text(
            Rect::new(body.x, list_y, body.w - 36.0, 20.0),
            format!("{}  ·  {}", account.label, account.status),
            style(12.0, if account.ok { MUTED } else { WARN }),
        );
        frame.icon_button(
            Rect::new(body.right() - 24.0, list_y - 2.0, 24.0, 24.0),
            Icon::Close,
            FAINT,
            Action::RemoveAccount(index),
            &format!("Remove {}", account.label),
        );
        list_y += 22.0;
    }
    list_y += 4.0;
    if !view.calendar_note.is_empty() {
        frame.text(
            Rect::new(body.x, list_y - 4.0, body.w, 16.0),
            view.calendar_note.as_str(),
            style(11.0, WARN),
        );
    }
    if view.agenda.is_empty() {
        frame.note(
            Rect::new(body.x, list_y + 20.0, body.w, 60.0),
            if view.accounts.is_empty() {
                "Connect a calendar to see what is coming up. Nothing is shown until an account is connected and has synced."
            } else {
                "Nothing coming up in the next two weeks."
            },
        );
        return;
    }
    let mut y = list_y + 14.0;
    for (index, row) in view.agenda.iter().enumerate().skip(view.scroll) {
        if y + ROW_H > body.bottom() {
            break;
        }
        frame.text(
            Rect::new(body.x, y, 110.0, 18.0),
            row.day.as_str(),
            style(12.0, MUTED),
        );
        frame.text(
            Rect::new(body.x, y + 18.0, 110.0, 16.0),
            row.time.as_str(),
            style(11.0, FAINT),
        );
        frame.text(
            Rect::new(body.x + 116.0, y, body.w - 152.0, 18.0),
            row.title.as_str(),
            style(13.0, TEXT),
        );
        frame.text(
            Rect::new(body.x + 116.0, y + 18.0, body.w - 152.0, 16.0),
            row.detail.as_str(),
            style(11.0, MUTED),
        );
        if row.deletable {
            frame.icon_button(
                Rect::new(body.right() - 32.0, y, 32.0, 32.0),
                Icon::Trash,
                FAINT,
                Action::DeleteEvent(index),
                "Delete this event",
            );
        }
        y += ROW_H;
    }
}

fn clipboard_page(frame: &mut Frame, view: &View, body: Rect) {
    let search = Rect::new(body.x, body.y, 240.0, 30.0);
    frame.fill(search, 15.0, BUTTON);
    let shown = if view.clip_query.is_empty() {
        "Search"
    } else {
        view.clip_query.as_str()
    };
    frame.text(search.inset(7.0), shown, style(12.0, MUTED));
    frame.hit(
        search,
        Action::StartInput(Field::ClipboardSearch),
        "Search the history",
    );
    if !view.clips.is_empty() {
        frame.pill_from_right(body.right(), body.y, "Clear", false, Action::ClearClips);
    }
    if !view.clips_note.is_empty() {
        frame.note(
            Rect::new(body.x, body.y + 44.0, body.w, 60.0),
            &view.clips_note,
        );
        if view.clips.is_empty() {
            return;
        }
    }
    let mut y = body.y + 40.0;
    for row in view.clips.iter().skip(view.scroll) {
        if y + ROW_H > body.bottom() {
            break;
        }
        let actions = 72.0;
        let content = Rect::new(body.x, y, body.w - actions, ROW_H - 4.0);
        if let Some(key) = &row.image_key {
            frame.scene.ops.push(Op::Image {
                rect: Rect::new(body.x, y, 48.0, 34.0),
                radius: 6.0,
                key: key.clone(),
            });
        }
        let text_x = body.x + if row.image_key.is_some() { 56.0 } else { 0.0 };
        frame.text(
            Rect::new(text_x, y + 2.0, content.right() - text_x, 18.0),
            first_line(&row.preview),
            style(13.0, TEXT),
        );
        frame.text(
            Rect::new(text_x, y + 20.0, 120.0, 14.0),
            row.kind.as_str(),
            style(10.0, FAINT),
        );
        frame.hit(
            content,
            Action::CopyClip(row.id),
            format!("Copy: {}", first_line(&row.preview)),
        );
        frame.icon_button(
            Rect::new(content.right() + 4.0, y + 2.0, 32.0, 32.0),
            Icon::Pin,
            if row.pinned { TEXT } else { FAINT },
            Action::PinClip(row.id, !row.pinned),
            if row.pinned { "Unpin" } else { "Pin" },
        );
        frame.icon_button(
            Rect::new(content.right() + 36.0, y + 2.0, 32.0, 32.0),
            Icon::Trash,
            FAINT,
            Action::DeleteClip(row.id),
            "Delete",
        );
        y += ROW_H;
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

fn shelf_page(frame: &mut Frame, view: &View, body: Rect) {
    if view.shelf.is_empty() {
        frame.note(
            Rect::new(body.x, body.y + 40.0, body.w, 60.0),
            "Drop files here to park them. Keenu keeps a reference: the files stay where they are.",
        );
        return;
    }
    frame.pill_from_right(body.right(), body.y, "Clear", false, Action::ClearShelf);
    frame.text(
        Rect::new(body.x, body.y + 6.0, 300.0, 18.0),
        "Drop more files anywhere on the island",
        style(12.0, MUTED),
    );
    let mut y = body.y + 40.0;
    for (index, row) in view.shelf.iter().enumerate().skip(view.scroll) {
        if y + ROW_H > body.bottom() {
            break;
        }
        let actions = 104.0;
        let content = Rect::new(body.x, y, body.w - actions, ROW_H - 4.0);
        if let Some(key) = &row.thumbnail_key {
            frame.scene.ops.push(Op::Image {
                rect: Rect::new(body.x, y, 34.0, 34.0),
                radius: 6.0,
                key: key.clone(),
            });
        } else {
            frame.icon(
                Rect::new(body.x + 6.0, y + 6.0, 22.0, 22.0),
                Icon::Folder,
                FAINT,
            );
        }
        let color = if row.missing { DANGER } else { TEXT };
        frame.text(
            Rect::new(body.x + 42.0, y + 2.0, content.w - 42.0, 18.0),
            row.name.as_str(),
            style(13.0, color),
        );
        let detail = if row.missing {
            "Missing: not found where it was"
        } else {
            row.detail.as_str()
        };
        frame.text(
            Rect::new(body.x + 42.0, y + 20.0, content.w - 42.0, 14.0),
            detail,
            style(10.0, MUTED),
        );
        if !row.missing {
            frame.hit(
                content,
                Action::OpenShelf(index),
                format!("Open {}", row.name),
            );
        }
        let mut x = content.right() + 4.0;
        if !row.missing {
            frame.icon_button(
                Rect::new(x, y + 2.0, 32.0, 32.0),
                Icon::Open,
                FAINT,
                Action::RevealShelf(index),
                "Show in folder",
            );
        }
        x += 34.0;
        frame.icon_button(
            Rect::new(x, y + 2.0, 32.0, 32.0),
            Icon::Pin,
            if row.pinned { TEXT } else { FAINT },
            Action::PinShelf(index, !row.pinned),
            if row.pinned { "Unpin" } else { "Pin" },
        );
        x += 34.0;
        frame.icon_button(
            Rect::new(x, y + 2.0, 32.0, 32.0),
            Icon::Close,
            FAINT,
            Action::RemoveShelf(index),
            "Take off the shelf",
        );
        y += ROW_H;
    }
}

fn voice_page(frame: &mut Frame, view: &View, body: Rect) {
    let voice = &view.voice;
    if voice.assistant {
        // The same pills and lines as the rest of the page, for typing and for stopping.
        frame.pill(
            Rect::new(body.x, body.y, 96.0, 28.0),
            "Type",
            false,
            Action::StartInput(Field::AssistantRequest),
        );
        if voice.busy {
            frame.pill(
                Rect::new(body.x + 104.0, body.y, 72.0, 28.0),
                "Stop",
                false,
                Action::AssistantStop,
            );
        }
    }
    if !voice.enabled && !voice.assistant {
        frame.note(
            Rect::new(body.x, body.y + 30.0, body.w, 40.0),
            "Voice is off. Turn it on in Settings. The microphone only ever opens while you are recording.",
        );
        frame.pill(
            Rect::new(body.x, body.y + 76.0, 150.0, 30.0),
            "Open Settings",
            true,
            Action::SelectPage(Page::Settings),
        );
        return;
    }
    if !voice.enabled {
        voice_output(frame, voice, body, body.y + 38.0);
        return;
    }
    frame.pill(
        Rect::new(body.right() - 96.0, body.y, 96.0, 28.0),
        "Endpoint",
        false,
        Action::StartInput(Field::VoiceEndpoint),
    );
    let button = Rect::new(body.x + body.w / 2.0 - 36.0, body.y + 10.0, 72.0, 72.0);
    frame.fill(button, 36.0, if voice.recording { DANGER } else { BUTTON });
    frame.icon(
        button.inset(20.0),
        if voice.recording {
            Icon::Stop
        } else {
            Icon::Mic
        },
        TEXT,
    );
    frame.hit(
        button,
        Action::VoiceToggle,
        if voice.recording {
            "Stop recording"
        } else {
            "Start recording"
        },
    );
    if voice.recording {
        frame.pill(
            Rect::new(button.right() + 16.0, button.y + 21.0, 84.0, 30.0),
            "Cancel",
            false,
            Action::VoiceCancel,
        );
    }
    let state = if voice.recording {
        "Recording. Click to stop."
    } else if voice.busy {
        "Working..."
    } else {
        "Click to speak"
    };
    frame.text(
        Rect::new(body.x, button.bottom() + 6.0, body.w, 18.0),
        state,
        TextStyle {
            align: Align::Center,
            ..style(13.0, if voice.recording { DANGER } else { MUTED })
        },
    );
    let mic = Rect::new(body.x, button.bottom() + 28.0, body.w, 24.0);
    frame.text(
        mic,
        format!("Microphone: {}   ·   {}", voice.microphone, voice.engine),
        TextStyle {
            align: Align::Center,
            ..style(11.0, FAINT)
        },
    );
    frame.hit(mic, Action::NextMicrophone, "Use the next microphone");
    voice_output(frame, voice, body, mic.bottom() + 4.0);
}

/// What was said, what the assistant is doing, and its answer, in the page's own text styles.
fn voice_output(frame: &mut Frame, voice: &VoiceView, body: Rect, top: f32) {
    let mut y = top;
    if !voice.error.is_empty() {
        frame.text(
            Rect::new(body.x, y, body.w, 34.0),
            voice.error.as_str(),
            TextStyle {
                wrap: true,
                ..style(12.0, DANGER)
            },
        );
        return;
    }
    if !voice.heard.is_empty() {
        frame.text(
            Rect::new(body.x, y, body.w, 18.0),
            format!("You: {}", voice.heard),
            style(12.0, MUTED),
        );
        y += 20.0;
    }
    for step in &voice.steps {
        frame.text(
            Rect::new(body.x, y, body.w, 16.0),
            step.as_str(),
            style(11.0, FAINT),
        );
        y += 16.0;
    }
    if !voice.answer.is_empty() {
        frame.text(
            Rect::new(body.x, y, body.w, body.bottom() - y),
            voice.answer.as_str(),
            TextStyle {
                wrap: true,
                ..style(13.0, TEXT)
            },
        );
    }
}

fn agents_page(frame: &mut Frame, view: &View, body: Rect) {
    frame.text(
        Rect::new(body.x, body.y, body.w, 34.0),
        "When an agent needs permission Keenu asks here. If Keenu is closed or you do not answer, the agent asks in its own terminal as usual.",
        TextStyle { wrap: true, ..style(12.0, MUTED) },
    );
    let mut y = body.y + 44.0;
    for row in &view.agents {
        frame.switch(
            &SwitchRow {
                title: row.name.clone(),
                detail: row.note.clone(),
                setting: row.setting,
                on: row.installed,
                unavailable: None,
            },
            y,
            body.w,
        );
        y += 42.0;
    }
}

fn usage_page(frame: &mut Frame, view: &View, body: Rect) {
    if !view.usage_note.is_empty() {
        frame.note(
            Rect::new(body.x, body.y + 30.0, body.w, 50.0),
            &view.usage_note,
        );
        if view.usage.is_empty() {
            frame.pill(
                Rect::new(body.x, body.y + 84.0, 150.0, 30.0),
                "Open Settings",
                true,
                Action::SelectPage(Page::Settings),
            );
            return;
        }
    }
    let mut right = body.right();
    right = frame.pill_from_right(right, body.y, "Delete all", false, Action::ClearUsage);
    frame.pill_from_right(right, body.y, "Export CSV", false, Action::ExportUsage);
    frame.text(
        Rect::new(body.x, body.y + 6.0, 240.0, 18.0),
        "Today, on this computer only",
        style(12.0, MUTED),
    );
    let mut y = body.y + 40.0;
    for (index, row) in view.usage.iter().enumerate().skip(view.scroll) {
        if y + ROW_H > body.bottom() {
            break;
        }
        let name = if row.category.is_empty() {
            row.app.clone()
        } else {
            format!("{}  ·  {}", row.app, row.category)
        };
        frame.text(
            Rect::new(body.x, y + 2.0, body.w - 180.0, 18.0),
            name,
            style(13.0, TEXT),
        );
        frame.text(
            Rect::new(body.right() - 170.0, y + 2.0, 100.0, 18.0),
            format_duration(row.duration_ms),
            TextStyle {
                align: Align::Right,
                ..style(12.0, MUTED)
            },
        );
        let bar = Rect::new(body.x, y + 24.0, body.w - 76.0, 4.0);
        frame.fill(bar, 2.0, RAIL);
        frame.fill(
            Rect::new(bar.x, bar.y, bar.w * row.fraction.clamp(0.0, 1.0), bar.h),
            2.0,
            TEXT,
        );
        frame.icon_button(
            Rect::new(body.right() - 68.0, y, 32.0, 32.0),
            Icon::Pin,
            FAINT,
            Action::CategorizeUsageApp(index),
            "Set a category",
        );
        frame.icon_button(
            Rect::new(body.right() - 34.0, y, 32.0, 32.0),
            Icon::Trash,
            FAINT,
            Action::DeleteUsageApp(index),
            "Delete this app's record",
        );
        y += ROW_H;
    }
}

/// How many rows the settings page has, for scrolling.
pub fn settings_rows(view: &View) -> usize {
    view.settings.switches.len() + view.settings.assistant_rows.len() + 4
}

fn settings_page(frame: &mut Frame, view: &View, body: Rect) {
    let settings = &view.settings;
    let mut y = body.y;
    let row_h = 38.0;
    let mut row = 0;
    let mut visible = |height: f32, y: &mut f32| -> bool {
        let shown = row >= view.scroll && *y + height <= body.bottom();
        row += 1;
        shown
    };
    for switch in &settings.switches {
        if visible(row_h, &mut y) {
            frame.switch(switch, y, body.w);
            y += row_h;
        }
    }
    for row in &settings.assistant_rows {
        if visible(row_h, &mut y) {
            let mut right = body.right();
            for (label, action) in row.buttons.iter().rev() {
                right = frame.pill_from_right(right, y, label, false, action.clone());
            }
            frame.text(
                Rect::new(body.x, y + 6.0, right - body.x - 8.0, 18.0),
                row.text.as_str(),
                style(13.0, TEXT),
            );
            y += row_h;
        }
    }
    if visible(row_h, &mut y) {
        frame.text(
            Rect::new(body.x, y + 6.0, 300.0, 18.0),
            format!("Display: {}", settings.monitor),
            style(13.0, TEXT),
        );
        frame.pill_from_right(
            body.right(),
            y,
            "Next display",
            false,
            Action::SelectMonitor,
        );
        y += row_h;
    }
    if visible(row_h, &mut y) {
        frame.text(
            Rect::new(body.x, y + 6.0, 330.0, 18.0),
            format!("Keenu {}  ·  {}", settings.version, settings.update_status),
            style(12.0, MUTED),
        );
        let right = body.right();
        if let Some(version) = &settings.update_ready {
            frame.pill_from_right(
                right,
                y,
                &format!("Install {version}"),
                true,
                Action::InstallUpdate,
            );
        } else if settings.update_available {
            frame.pill_from_right(right, y, "Check now", false, Action::CheckUpdates);
        }
        y += row_h;
    }
    if visible(row_h, &mut y) {
        let mut right = body.right();
        right = frame.pill_from_right(right, y, "Reset settings", false, Action::ResetSettings);
        right = frame.pill_from_right(right, y, "Open data folder", false, Action::OpenDataFolder);
        frame.pill_from_right(right, y, "Export data", false, Action::ExportData);
        y += row_h;
    }
    if visible(row_h, &mut y) {
        let mut right = body.right();
        if settings.confirming_delete {
            right = frame.pill_from_right(
                right,
                y,
                "Yes, delete everything",
                true,
                Action::ConfirmDeleteAllData,
            );
            frame.text(
                Rect::new(body.x, y + 6.0, right - body.x, 18.0),
                "Clipboard, usage, shelf and calendars here. Sure?",
                style(12.0, DANGER),
            );
        } else {
            right = frame.pill_from_right(right, y, "Delete my data", false, Action::DeleteAllData);
            right = frame.pill_from_right(right, y, "Quit Keenu", false, Action::Quit);
            let _ = right;
        }
    }
}

fn approval_page(frame: &mut Frame, approval: &ApprovalView) {
    let x = PAD + 8.0;
    let width = EXPANDED_W - x - PAD;
    frame.icon(Rect::new(PAD - 2.0, 24.0, 26.0, 26.0), Icon::Warning, WARN);
    let title = if approval.project.is_empty() {
        format!("{} needs permission", approval.agent)
    } else {
        format!("{} needs permission", approval.project)
    };
    frame.text(
        Rect::new(x + 24.0, 18.0, width - 24.0, 24.0),
        title,
        TextStyle {
            bold: true,
            ..style(17.0, TEXT)
        },
    );
    frame.text(
        Rect::new(x + 24.0, 42.0, width - 24.0, 18.0),
        format!(
            "{}  ·  {}  ·  answer within {} s",
            approval.agent, approval.tool, approval.seconds_left
        ),
        style(12.0, MUTED),
    );
    let block = Rect::new(x, 68.0, width, 128.0);
    frame.fill(block, 10.0, BUTTON);
    frame.text(
        block.inset(10.0),
        approval.detail.as_str(),
        TextStyle {
            wrap: true,
            mono: true,
            ..style(12.0, TEXT)
        },
    );
    let mut y = block.bottom() + 8.0;
    if approval.truncated {
        frame.text(
            Rect::new(x, y, width, 16.0),
            "Too long to show whole. Check it in the terminal before allowing.",
            style(11.0, WARN),
        );
        y += 18.0;
    }
    if !approval.cwd.is_empty() {
        frame.text(
            Rect::new(x, y, width, 16.0),
            format!("In {}", approval.cwd),
            style(11.0, MUTED),
        );
        y += 18.0;
    }
    if !approval.permissions.is_empty() {
        frame.text(
            Rect::new(x, y, width, 16.0),
            format!(
                "Could remember for later: {}",
                approval.permissions.join(", ")
            ),
            style(11.0, FAINT),
        );
    }
    let buttons_y = EXPANDED_H - 48.0;
    let mut right = EXPANDED_W - PAD;
    // Allow is not first in reading order: Tab and Enter reach Deny before it.
    if !approval.own {
        right = frame.pill_from_right(
            right,
            buttons_y,
            "Ask in terminal",
            false,
            Action::ApprovalInTerminal(approval.id.clone()),
        );
    }
    right = frame.pill_from_right(
        right,
        buttons_y,
        "Deny",
        false,
        Action::ApprovalDeny(approval.id.clone()),
    );
    frame.pill_from_right(
        right,
        buttons_y,
        "Allow",
        true,
        Action::ApprovalAllow(approval.id.clone()),
    );
}

fn onboarding_page(frame: &mut Frame, onboarding: &OnboardingView) {
    let x = PAD + 8.0;
    let width = EXPANDED_W - x * 2.0;
    frame.text(
        Rect::new(x, 22.0, width, 26.0),
        onboarding.title.as_str(),
        TextStyle {
            bold: true,
            ..style(19.0, TEXT)
        },
    );
    frame.text(
        Rect::new(x, 52.0, width, 58.0),
        onboarding.body.as_str(),
        TextStyle {
            wrap: true,
            ..style(13.0, MUTED)
        },
    );
    let mut y = 116.0;
    for switch in &onboarding.switches {
        frame.switch(switch, y, EXPANDED_W - PAD * 2.0);
        y += 38.0;
    }
    let buttons_y = EXPANDED_H - 48.0;
    let last = onboarding.step + 1 == onboarding.steps;
    let right = frame.pill_from_right(
        EXPANDED_W - PAD,
        buttons_y,
        if last { "Done" } else { "Next" },
        true,
        Action::OnboardingNext,
    );
    if !last {
        frame.pill_from_right(right, buttons_y, "Skip", false, Action::OnboardingSkip);
    }
    frame.text(
        Rect::new(x, buttons_y + 6.0, 120.0, 18.0),
        format!("{} of {}", onboarding.step + 1, onboarding.steps),
        style(12.0, FAINT),
    );
}

fn input_bar(frame: &mut Frame, input: &InputView, width: f32) {
    let rect = Rect::new(PAD, HEADER_H + 4.0, width - PAD * 2.0, 58.0);
    frame.fill(rect, 12.0, BUTTON_HOVER);
    frame.text(
        Rect::new(rect.x + 12.0, rect.y + 6.0, rect.w - 24.0, 16.0),
        input.field.label(),
        style(11.0, MUTED),
    );
    let shown = if input.field.secret() {
        "\u{2022}".repeat(input.text.chars().count())
    } else {
        input.text.clone()
    };
    frame.text(
        Rect::new(rect.x + 12.0, rect.y + 26.0, rect.w - 24.0, 24.0),
        format!("{shown}\u{2502}"),
        style(14.0, TEXT),
    );
}

/// Keeps a scroll position inside a list of `rows`.
pub fn clamp_scroll(scroll: i64, rows: usize) -> usize {
    scroll.clamp(0, rows.saturating_sub(1) as i64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_text(scene: &Scene<Action>, wanted: &str) -> bool {
        scene.texts().any(|text| text.contains(wanted))
    }

    fn has_action(scene: &Scene<Action>, wanted: &Action) -> bool {
        scene.hits.iter().any(|hit| &hit.action == wanted)
    }

    fn song() -> MediaView {
        MediaView {
            title: "Monument Valley".into(),
            artist: "Lumen Coast".into(),
            album: "Desert Light".into(),
            app: "Spotify".into(),
            playing: true,
            position_ms: 73_000,
            duration_ms: 214_000,
            can_next: true,
            can_previous: true,
            can_seek: true,
            artwork_key: Some("art".into()),
        }
    }

    fn approval() -> ApprovalView {
        ApprovalView {
            id: "r1".into(),
            agent: "Claude Code".into(),
            project: "korus".into(),
            tool: "Bash".into(),
            detail: "npm run build".into(),
            truncated: false,
            cwd: "D:\\code\\korus".into(),
            permissions: vec![],
            seconds_left: 300,
            own: false,
        }
    }

    #[test]
    fn the_closed_island_shows_time_and_opens_on_click() {
        let view = View {
            time: "9:41".into(),
            date: "Mon 26 Oct".into(),
            ..View::default()
        };
        let scene = build(&view);
        assert_eq!((scene.width, scene.height), (COMPACT_W, COMPACT_H));
        assert!(has_text(&scene, "9:41"));
        assert_eq!(scene.hit_at(10.0, 10.0).unwrap().action, Action::Expand);
    }

    #[test]
    fn the_closed_island_names_what_is_playing() {
        let view = View {
            media: Some(song()),
            ..View::default()
        };
        assert!(has_text(&build(&view), "Monument Valley"));
    }

    #[test]
    fn the_open_island_has_a_tab_for_every_page() {
        let view = View {
            expanded: true,
            ..View::default()
        };
        let scene = build(&view);
        for page in Page::ALL {
            assert!(has_action(&scene, &Action::SelectPage(page)), "{page:?}");
        }
    }

    #[test]
    fn music_has_its_controls_and_only_seeks_when_it_can() {
        let mut media = song();
        let mut view = View {
            expanded: true,
            media: Some(media.clone()),
            session_count: 1,
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_text(&scene, "Monument Valley"));
        assert!(has_text(&scene, "1:13"));
        assert!(has_action(&scene, &Action::PlayPause));
        assert!(
            scene
                .hits
                .iter()
                .any(|hit| matches!(hit.action, Action::SeekFraction(_)))
        );
        media.can_seek = false;
        view.media = Some(media);
        assert!(
            !build(&view)
                .hits
                .iter()
                .any(|hit| matches!(hit.action, Action::SeekFraction(_)))
        );
    }

    #[test]
    fn with_nothing_playing_it_says_so_instead_of_showing_controls() {
        let view = View {
            expanded: true,
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_text(&scene, "Nothing is playing"));
        assert!(!has_action(&scene, &Action::PlayPause));
    }

    #[test]
    fn a_media_system_failure_is_shown_as_such() {
        let view = View {
            expanded: true,
            media_note: "Windows media sessions are unavailable: error".into(),
            ..View::default()
        };
        assert!(has_text(&build(&view), "unavailable"));
    }

    #[test]
    fn lyrics_highlight_the_current_line_and_can_be_clicked() {
        let view = View {
            expanded: true,
            media: Some(song()),
            lyrics: LyricsView {
                lines: vec![(4, "before".into()), (5, "now".into())],
                active: Some(5),
                note: String::new(),
            },
            ..View::default()
        };
        let scene = build(&view);
        let bold_now = scene
            .ops
            .iter()
            .any(|op| matches!(op, Op::Text { text, style, .. } if text == "now" && style.bold));
        let plain_before = scene.ops.iter().any(
            |op| matches!(op, Op::Text { text, style, .. } if text == "before" && !style.bold),
        );
        assert!(bold_now && plain_before);
        assert!(has_action(&scene, &Action::SeekToLine(5)));
    }

    #[test]
    fn an_approval_takes_over_even_a_closed_island_with_the_command_and_folder() {
        let view = View {
            approval: Some(approval()),
            ..View::default()
        };
        let scene = build(&view);
        assert_eq!(scene.width, EXPANDED_W);
        assert!(has_text(&scene, "korus needs permission"));
        assert!(has_text(&scene, "npm run build"));
        assert!(has_text(&scene, "D:\\code\\korus"));
        assert!(has_action(&scene, &Action::ApprovalAllow("r1".into())));
        assert!(has_action(&scene, &Action::ApprovalDeny("r1".into())));
        assert!(has_action(&scene, &Action::ApprovalInTerminal("r1".into())));
    }

    #[test]
    fn a_command_too_long_to_show_warns_before_allowing() {
        let mut request = approval();
        request.truncated = true;
        let view = View {
            approval: Some(request),
            ..View::default()
        };
        assert!(has_text(&build(&view), "Too long to show whole"));
    }

    #[test]
    fn keyboard_focus_reaches_deny_before_allow() {
        let view = View {
            approval: Some(approval()),
            ..View::default()
        };
        let scene = build(&view);
        let at = |wanted: &Action| {
            scene
                .hits
                .iter()
                .position(|hit| &hit.action == wanted)
                .unwrap()
        };
        assert!(at(&Action::ApprovalDeny("r1".into())) < at(&Action::ApprovalAllow("r1".into())));
    }

    #[test]
    fn voice_off_has_no_way_to_start_recording() {
        let view = View {
            expanded: true,
            page: Page::Voice,
            ..View::default()
        };
        let scene = build(&view);
        assert!(!has_action(&scene, &Action::VoiceToggle));
        assert!(has_text(&scene, "Voice is off"));
    }

    #[test]
    fn recording_says_so_and_can_be_cancelled() {
        let view = View {
            expanded: true,
            page: Page::Voice,
            voice: VoiceView {
                enabled: true,
                recording: true,
                microphone: "Built-in".into(),
                ..VoiceView::default()
            },
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_text(&scene, "Recording"));
        assert!(has_action(&scene, &Action::VoiceCancel));
    }

    #[test]
    fn an_unavailable_switch_says_why_and_cannot_be_flipped() {
        let view = View {
            expanded: true,
            page: Page::Settings,
            settings: SettingsView {
                switches: vec![SwitchRow {
                    title: "Check for updates".into(),
                    detail: "Once a day".into(),
                    setting: Setting::UpdateChecks,
                    on: true,
                    unavailable: Some("This build has no update address.".into()),
                }],
                ..SettingsView::default()
            },
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_text(&scene, "no update address"));
        assert!(!has_action(&scene, &Action::Toggle(Setting::UpdateChecks)));
    }

    #[test]
    fn each_calendar_account_can_be_removed() {
        let view = View {
            expanded: true,
            page: Page::Calendar,
            accounts: vec![AccountRow {
                label: "me on dav.example.org".into(),
                status: "synced".into(),
                ok: true,
            }],
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_text(&scene, "me on dav.example.org"));
        assert!(has_action(&scene, &Action::RemoveAccount(0)));
    }

    #[test]
    fn deleting_everything_asks_first() {
        let mut view = View {
            expanded: true,
            page: Page::Settings,
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_action(&scene, &Action::DeleteAllData));
        assert!(!has_action(&scene, &Action::ConfirmDeleteAllData));
        view.settings.confirming_delete = true;
        assert!(has_action(&build(&view), &Action::ConfirmDeleteAllData));
    }

    #[test]
    fn secrets_are_dots_on_screen() {
        let view = View {
            expanded: true,
            input: Some(InputView {
                field: Field::VoiceApiKey,
                text: "sk-secret".into(),
            }),
            ..View::default()
        };
        let scene = build(&view);
        assert!(!has_text(&scene, "sk-secret"));
        assert!(has_text(&scene, "\u{2022}\u{2022}\u{2022}"));
    }

    #[test]
    fn the_clipboard_page_explains_itself_while_off() {
        let view = View {
            expanded: true,
            page: Page::Clipboard,
            clips_note: "Clipboard history is off.".into(),
            ..View::default()
        };
        assert!(has_text(&build(&view), "history is off"));
    }

    #[test]
    fn scrolling_moves_past_rows() {
        let row = |id: u64| ClipRow {
            id,
            preview: format!("clip {id}"),
            kind: "text".into(),
            pinned: false,
            image_key: None,
        };
        let view = View {
            expanded: true,
            page: Page::Clipboard,
            clips: (0..20).map(row).collect(),
            scroll: 5,
            ..View::default()
        };
        let scene = build(&view);
        assert!(!has_text(&scene, "clip 4"));
        assert!(has_text(&scene, "clip 5"));
        assert_eq!(clamp_scroll(99, 20), 19);
        assert_eq!(clamp_scroll(-1, 20), 0);
    }

    #[test]
    fn a_missing_shelf_file_says_so_and_cannot_be_opened() {
        let view = View {
            expanded: true,
            page: Page::Shelf,
            shelf: vec![ShelfRow {
                name: "report.pdf".into(),
                detail: "1 MB".into(),
                pinned: false,
                missing: true,
                thumbnail_key: None,
            }],
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_text(&scene, "Missing"));
        assert!(!has_action(&scene, &Action::OpenShelf(0)));
        assert!(has_action(&scene, &Action::RemoveShelf(0)));
    }

    #[test]
    fn nothing_is_drawn_outside_the_island() {
        let view = View {
            expanded: true,
            page: Page::Settings,
            settings: SettingsView {
                switches: (0..20)
                    .map(|i| SwitchRow {
                        title: format!("s{i}"),
                        detail: String::new(),
                        setting: Setting::Notifications,
                        on: false,
                        unavailable: None,
                    })
                    .collect(),
                ..SettingsView::default()
            },
            ..View::default()
        };
        let scene = build(&view);
        for hit in &scene.hits {
            assert!(hit.rect.bottom() <= scene.height + 0.5, "{hit:?}");
            assert!(hit.rect.right() <= scene.width + 0.5, "{hit:?}");
        }
    }

    #[test]
    fn the_first_run_can_be_skipped_and_ends_with_done() {
        let mut view = View {
            onboarding: Some(OnboardingView {
                step: 0,
                steps: 3,
                title: "Welcome to Keenu".into(),
                body: "Everything that watches or listens starts off.".into(),
                switches: vec![],
            }),
            ..View::default()
        };
        let scene = build(&view);
        assert!(has_text(&scene, "Welcome to Keenu"));
        assert!(has_action(&scene, &Action::OnboardingSkip));
        view.onboarding.as_mut().unwrap().step = 2;
        let scene = build(&view);
        assert!(has_text(&scene, "Done"));
        assert!(!has_action(&scene, &Action::OnboardingSkip));
    }
}
