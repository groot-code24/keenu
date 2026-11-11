// SPDX-License-Identifier: GPL-3.0-or-later

//! Made-up content for each page, for looking at a page's design with `keenu --render-sample`.

use std::collections::HashMap;
use std::sync::Arc;

use keenu_ui::view::{
    AccountRow, AgendaRow, AgentRow, ApprovalView, ClipRow, LyricsView, MediaView, OnboardingView,
    Page, Setting, SettingsView, ShelfRow, SwitchRow, UsageRow, View, VoiceView,
};

/// The sample cover: a soft gradient, as a PNG.
pub fn images() -> HashMap<String, Arc<Vec<u8>>> {
    let (width, height) = (128u32, 128u32);
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (fx, fy) = (x as f32 / width as f32, y as f32 / height as f32);
            rgba.extend_from_slice(&[
                (90.0 + 165.0 * fx) as u8,
                (40.0 + 100.0 * fy) as u8,
                (120.0 - 60.0 * fx) as u8,
                255,
            ]);
        }
    }
    let mut png = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png);
    let _ = image::ImageEncoder::write_image(
        encoder,
        &rgba,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    );
    HashMap::from([("art".to_owned(), Arc::new(png))])
}

fn switch(title: &str, detail: &str, setting: Setting, on: bool) -> SwitchRow {
    SwitchRow {
        title: title.into(),
        detail: detail.into(),
        setting,
        on,
        unavailable: None,
    }
}

/// The view for a page's name; `None` for a name that is not one.
pub fn view(page: &str) -> Option<View> {
    let mut view = View {
        expanded: true,
        time: "9:41".into(),
        date: "Sat 31 Oct".into(),
        ..View::default()
    };
    let music = MediaView {
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
    };
    match page {
        "compact" => {
            view.expanded = false;
            view.media = Some(music);
        }
        "compact-idle" => view.expanded = false,
        "media" => {
            view.media = Some(music);
            view.session_count = 2;
            view.lyrics = LyricsView {
                lines: vec![
                    (4, "Across the salt, the morning breaks".into()),
                    (5, "Every road I never took".into()),
                    (6, "Leads me back to you".into()),
                    (7, "And the desert keeps our names".into()),
                ],
                active: Some(5),
                note: String::new(),
            };
        }
        "media-empty" => {}
        "calendar" => {
            view.page = Page::Calendar;
            view.accounts = vec![AccountRow {
                label: "me on dav.example.org".into(),
                status: "synced".into(),
                ok: true,
            }];
            view.agenda = vec![
                AgendaRow {
                    deletable: true,
                    day: "Sat 31 Oct".into(),
                    time: "10:00 - 10:15".into(),
                    title: "Standup".into(),
                    detail: "Meet  ·  Work".into(),
                },
                AgendaRow {
                    deletable: true,
                    day: "Sat 31 Oct".into(),
                    time: "13:00 - 14:00".into(),
                    title: "Lunch with Ana".into(),
                    detail: "Personal".into(),
                },
                AgendaRow {
                    deletable: false,
                    day: "Sun 1 Nov".into(),
                    time: "All day".into(),
                    title: "Holiday".into(),
                    detail: "Calendar".into(),
                },
            ];
        }
        "clipboard" => {
            view.page = Page::Clipboard;
            view.clips = vec![
                ClipRow {
                    id: 3,
                    preview: "cargo build --release -p keenu-windows".into(),
                    kind: "text".into(),
                    pinned: true,
                    image_key: None,
                },
                ClipRow {
                    id: 2,
                    preview: "https://lrclib.net".into(),
                    kind: "link".into(),
                    pinned: false,
                    image_key: None,
                },
                ClipRow {
                    id: 1,
                    preview: "Image".into(),
                    kind: "image".into(),
                    pinned: false,
                    image_key: Some("art".into()),
                },
            ];
        }
        "clipboard-off" => {
            view.page = Page::Clipboard;
            view.clips_note = "Clipboard history is off. Nothing you copy is read or kept. Turn it on in Settings.".into();
        }
        "shelf" => {
            view.page = Page::Shelf;
            view.shelf = vec![
                ShelfRow {
                    name: "Quarterly report.pdf".into(),
                    detail: "2.4 MB  ·  C:\\Users\\me\\Documents".into(),
                    pinned: true,
                    missing: false,
                    thumbnail_key: Some("art".into()),
                },
                ShelfRow {
                    name: "old-draft.docx".into(),
                    detail: String::new(),
                    pinned: false,
                    missing: true,
                    thumbnail_key: None,
                },
            ];
        }
        "voice" => {
            view.page = Page::Voice;
            view.voice = VoiceView {
                enabled: true,
                recording: true,
                microphone: "System default".into(),
                engine: "On this computer".into(),
                heard: "What is on my calendar today?".into(),
                ..VoiceView::default()
            };
        }
        "agents" => {
            view.page = Page::Agents;
            view.agents = vec![
                AgentRow {
                    name: "Claude Code".into(),
                    setting: Setting::ClaudeHooks,
                    installed: true,
                    note: "Hooks installed.".into(),
                },
                AgentRow {
                    name: "Codex".into(),
                    setting: Setting::CodexHooks,
                    installed: false,
                    note: "Codex runs a new hook only once you have trusted it: open Codex and review it with /hooks.".into(),
                },
            ];
        }
        "usage" => {
            view.page = Page::Usage;
            view.usage = vec![
                UsageRow {
                    app: "code.exe".into(),
                    category: "Work".into(),
                    duration_ms: 3 * 3_600_000,
                    fraction: 1.0,
                },
                UsageRow {
                    app: "chrome.exe".into(),
                    category: String::new(),
                    duration_ms: 3_600_000,
                    fraction: 0.33,
                },
            ];
        }
        "settings" => {
            view.page = Page::Settings;
            view.settings = SettingsView {
                switches: vec![
                    switch(
                        "Clipboard history",
                        "Keeps what you copy, on this computer",
                        Setting::ClipboardHistory,
                        true,
                    ),
                    switch(
                        "App usage",
                        "Records the app in front and for how long",
                        Setting::UsageTracking,
                        false,
                    ),
                    switch(
                        "Voice",
                        "The microphone opens only while you record",
                        Setting::VoiceEnabled,
                        false,
                    ),
                    SwitchRow {
                        unavailable: Some(
                            "This build has no update address or key, so it cannot update itself."
                                .into(),
                        ),
                        ..switch("Look for updates", "", Setting::UpdateChecks, false)
                    },
                ],
                version: env!("CARGO_PKG_VERSION").into(),
                update_status: "Not checked yet.".into(),
                monitor: "Primary".into(),
                ..SettingsView::default()
            };
        }
        "approval" => {
            view.expanded = false;
            view.approval = Some(ApprovalView {
                id: "sample".into(),
                agent: "Claude Code".into(),
                project: "korus".into(),
                tool: "Bash".into(),
                detail: "npm run build && npm test -- --coverage".into(),
                truncated: false,
                cwd: "D:\\code\\korus".into(),
                permissions: vec!["Bash npm run build".into()],
                seconds_left: 287,
                own: false,
            });
        }
        "onboarding" => {
            view.onboarding = Some(OnboardingView {
                step: 1,
                steps: 3,
                title: "What may Keenu keep?".into(),
                body: "All of it stays on this computer, and you can pause, export or delete it at any time in Settings.".into(),
                switches: vec![
                    switch("Clipboard history", "Keeps what you copy; apps can mark things as private", Setting::ClipboardHistory, false),
                    switch("App usage", "Only an app's name and for how long", Setting::UsageTracking, false),
                    switch("Voice", "The microphone opens only while you record", Setting::VoiceEnabled, false),
                ],
            });
        }
        _ => return None,
    }
    Some(view)
}
