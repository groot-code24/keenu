// SPDX-License-Identifier: GPL-3.0-or-later

//! Keenu's own files on this computer, and bringing in what earlier versions kept.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use keenu_core::clipboard::{Clip, ClipboardHistory};
use keenu_core::file_shelf::FileShelf;
use keenu_core::json_file;
use keenu_core::reminders::Reminders;
use keenu_core::settings::{self, Settings};
use keenu_core::todo::TodoList;
use keenu_core::usage_log::{self, UsageLog};
use keenu_integrations::calendar::engine::AccountCache;
use serde::Deserialize;

/// Where everything is. Tests point it at a temporary folder.
#[derive(Debug, Clone)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    pub fn system() -> Self {
        Self {
            root: keenu_core::paths::data_dir(),
        }
    }

    pub fn assistant_log(&self) -> PathBuf {
        self.root.join("assistant-actions.log")
    }
    pub fn assistant_memory(&self) -> PathBuf {
        self.root.join("assistant-memory.bin")
    }
    pub fn assistant_schedule(&self) -> PathBuf {
        self.root.join("assistant-schedule.json")
    }
    pub fn connectors(&self) -> PathBuf {
        self.root.join("assistant-connectors.json")
    }
    pub fn assistant_workspace(&self) -> PathBuf {
        self.root.join("assistant-workspace")
    }
    pub fn settings(&self) -> PathBuf {
        self.root.join(settings::FILE_NAME)
    }
    pub fn clipboard(&self) -> PathBuf {
        self.root.join("clipboard.json")
    }
    pub fn clip_images(&self) -> PathBuf {
        self.root.join("clips")
    }
    pub fn shelf(&self) -> PathBuf {
        self.root.join(keenu_core::file_shelf::FILE_NAME)
    }
    pub fn usage(&self) -> PathBuf {
        self.root.join(usage_log::FILE_NAME)
    }
    pub fn todo(&self) -> PathBuf {
        self.root.join("todo.json")
    }
    pub fn reminders(&self) -> PathBuf {
        self.root.join("reminders.json")
    }
    pub fn calendars(&self) -> PathBuf {
        self.root.join("calendars")
    }
    pub fn calendar_cache(&self, account: &str) -> PathBuf {
        self.calendars()
            .join(format!("{}.json", safe_name(account)))
    }
    pub fn local_calendar(&self) -> PathBuf {
        self.calendar_cache("local")
    }
    pub fn lyrics_cache(&self) -> PathBuf {
        keenu_core::paths::cache_dir().join("lyrics")
    }
    pub fn agent_token(&self) -> PathBuf {
        self.root.join("agent-token")
    }
    pub fn updates(&self) -> PathBuf {
        keenu_core::paths::cache_dir().join("updates")
    }
}

/// An account id as one file name, whatever it holds.
fn safe_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Loads a file; on a read error the default, with `read_only` telling the caller not to save over it.
pub fn load_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    match json_file::load(path) {
        Ok(value) => value,
        Err(err) => {
            log::warn!("could not read {}: {err}", path.display());
            T::default()
        }
    }
}

pub fn load_settings(paths: &Paths) -> Settings {
    load_or_default::<Settings>(&paths.settings()).sanitized()
}

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// What an old `neeru-data.json` holds.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyStore {
    tasks: Vec<LegacyTask>,
    reminders: Vec<LegacyReminder>,
    events: Vec<LegacyEvent>,
    clipboard_history: Vec<String>,
    shelf_files: Vec<String>,
    app_usage_seconds: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
struct LegacyTask {
    title: String,
    #[serde(default)]
    completed: bool,
}

#[derive(Debug, Deserialize)]
struct LegacyReminder {
    title: String,
    due_at: DateTime<Utc>,
    #[serde(default)]
    dismissed: bool,
}

#[derive(Debug, Deserialize)]
struct LegacyEvent {
    id: String,
    title: String,
    start: DateTime<Utc>,
    end: Option<DateTime<Utc>>,
}

const LEGACY_DATA: &str = "neeru-data.json";

/// Copies an earlier version's folder over, then turns its old file into Keenu's. The originals stay
/// as they were, and running again changes nothing.
pub fn migrate_legacy(paths: &Paths) -> Vec<String> {
    let mut notes = Vec::new();
    for old in keenu_core::paths::legacy_data_dirs() {
        match keenu_core::migration::migrate(&old, &paths.root, &[]) {
            Ok(report) if report.skipped => {}
            Ok(report) => notes.push(format!(
                "Brought over {} file(s) from {}; {} kept, {} failed.",
                report.copied,
                old.display(),
                report.kept_existing,
                report.failed.len()
            )),
            Err(err) => notes.push(format!("Could not bring over {}: {err}", old.display())),
        }
    }
    match import_legacy_store(paths) {
        Ok(true) => notes.push(
            "Imported tasks, reminders, clipboard, shelf and usage from an earlier version.".into(),
        ),
        Ok(false) => {}
        Err(err) => notes.push(format!("Could not import earlier data: {err}")),
    }
    notes
}

/// Turns an old `neeru-data.json` and its switch files into Keenu's files; true if it did. A file Keenu
/// already has is not touched.
pub fn import_legacy_store(paths: &Paths) -> Result<bool, String> {
    let source = paths.root.join(LEGACY_DATA);
    let marker = paths.root.join(format!("{LEGACY_DATA}.imported"));
    if !source.exists() || marker.exists() {
        return Ok(false);
    }
    let bytes = std::fs::read(&source).map_err(|err| err.to_string())?;
    let old: LegacyStore =
        serde_json::from_slice(&bytes).map_err(|err| format!("{LEGACY_DATA} is damaged: {err}"))?;
    let now = now_ms();

    let save_new =
        |path: PathBuf, value: &dyn Fn() -> Result<(), std::io::Error>| -> Result<(), String> {
            if path.exists() {
                return Ok(());
            }
            value().map_err(|err| err.to_string())
        };

    let todo_path = paths.todo();
    save_new(todo_path.clone(), &|| {
        let mut list = TodoList::default();
        for task in &old.tasks {
            if let Some(id) = list.add(&task.title) {
                list.set_done(id, task.completed, now);
            }
        }
        json_file::save(&todo_path, &list)
    })?;

    let reminders_path = paths.reminders();
    save_new(reminders_path.clone(), &|| {
        let mut list = Reminders::default();
        for reminder in old.reminders.iter().filter(|reminder| !reminder.dismissed) {
            list.add(&reminder.title, reminder.due_at.timestamp_millis());
        }
        json_file::save(&reminders_path, &list)
    })?;

    let clipboard_path = paths.clipboard();
    save_new(clipboard_path.clone(), &|| {
        let mut history = ClipboardHistory::default();
        // Oldest first in the old file; the history puts the newest on top.
        for text in &old.clipboard_history {
            if let Ok(clip) = Clip::from_text(text) {
                history.add_keeping(clip, "", now, 100);
            }
        }
        json_file::save(&clipboard_path, &history)
    })?;

    let shelf_path = paths.shelf();
    save_new(shelf_path.clone(), &|| {
        let mut shelf = FileShelf::default();
        for file in &old.shelf_files {
            // Those no longer there are left out rather than shown broken for ever.
            let _ = shelf.add(Path::new(file), now);
        }
        json_file::save(&shelf_path, &shelf)
    })?;

    let usage_path = paths.usage();
    save_new(usage_path.clone(), &|| {
        // The old file has totals only: each becomes one stretch ending at the import.
        let mut log = UsageLog::default();
        for (app, seconds) in &old.app_usage_seconds {
            log.record(app, now - *seconds as i64 * 1000, now);
        }
        json_file::save(&usage_path, &log)
    })?;

    let calendar_path = paths.local_calendar();
    save_new(calendar_path.clone(), &|| {
        let mut cache = AccountCache::default();
        let state = cache.calendars.entry("local".into()).or_default();
        for event in &old.events {
            let start_ms = event.start.timestamp_millis();
            state.events.insert(
                event.id.clone(),
                keenu_integrations::calendar::Event {
                    id: event.id.clone(),
                    etag: String::new(),
                    title: event.title.clone(),
                    location: String::new(),
                    start_ms,
                    end_ms: event.end.map_or(start_ms, |end| end.timestamp_millis()),
                    all_day: false,
                    ics: None,
                },
            );
        }
        json_file::save(&calendar_path, &cache)
    })?;

    // The old switches were empty files.
    if !paths.settings().exists() {
        let mut settings = Settings::default();
        settings.clipboard.enabled = paths.root.join("clipboard-history.enabled").exists();
        settings.usage.enabled = paths.root.join("usage.enabled").exists();
        settings.updates.check = paths.root.join("updates.enabled").exists();
        json_file::save(&paths.settings(), &settings).map_err(|err| err.to_string())?;
    }

    std::fs::write(marker, "imported").map_err(|err| err.to_string())?;
    Ok(true)
}

/// A single file with everything of the user's except secrets, for their own keeping.
pub fn export_all(paths: &Paths, to: &Path) -> Result<(), String> {
    let settings = load_settings(paths);
    let usage: UsageLog = load_or_default(&paths.usage());
    let document = serde_json::json!({
        "product": "Keenu",
        "exported_at": Utc::now().to_rfc3339(),
        "settings": settings,
        "clipboard": load_or_default::<ClipboardHistory>(&paths.clipboard()),
        "shelf": load_or_default::<FileShelf>(&paths.shelf()),
        "usage": usage.sessions(),
        "todo": load_or_default::<TodoList>(&paths.todo()),
        "reminders": load_or_default::<Reminders>(&paths.reminders()),
    });
    let bytes = serde_json::to_vec_pretty(&document).map_err(|err| err.to_string())?;
    std::fs::write(to, bytes).map_err(|err| err.to_string())
}

/// Deletes what Keenu recorded about the user, but not their settings or the legacy originals.
pub fn delete_all_data(paths: &Paths) -> Result<(), String> {
    for file in [
        paths.clipboard(),
        paths.shelf(),
        paths.usage(),
        paths.todo(),
        paths.reminders(),
    ] {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("{}: {err}", file.display())),
        }
    }
    for dir in [paths.clip_images(), paths.calendars()] {
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(format!("{}: {err}", dir.display())),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> Paths {
        let root = std::env::temp_dir().join(format!("keenu-data-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Paths { root }
    }

    const OLD: &str = r#"{
        "tasks": [{"id": "1", "title": "Buy milk", "completed": false, "created_at": "2026-10-30T10:00:00Z"},
                  {"id": "2", "title": "Done one", "completed": true, "created_at": "2026-10-30T10:00:00Z"}],
        "reminders": [{"id": "3", "title": "Call", "due_at": "2099-01-01T10:00:00Z", "dismissed": false},
                      {"id": "4", "title": "Gone", "due_at": "2020-01-01T10:00:00Z", "dismissed": true}],
        "events": [{"id": "e1", "title": "Lunch", "start": "2026-10-31T10:00:00Z", "end": null}],
        "clipboard_history": ["older", "newer"],
        "shelf_files": ["C:\\nowhere\\gone.txt"],
        "app_usage_seconds": {"code.exe": 3600}
    }"#;

    #[test]
    fn an_old_store_becomes_keenus_files() {
        let paths = temp("import");
        std::fs::write(paths.root.join(LEGACY_DATA), OLD).unwrap();
        std::fs::write(paths.root.join("clipboard-history.enabled"), "enabled").unwrap();

        assert_eq!(import_legacy_store(&paths), Ok(true));

        let todo: TodoList = json_file::load(&paths.todo()).unwrap();
        assert_eq!(todo.items().len(), 2);
        let reminders: Reminders = json_file::load(&paths.reminders()).unwrap();
        assert_eq!(
            reminders.items().len(),
            1,
            "a dismissed one stays dismissed"
        );
        let clips: ClipboardHistory = json_file::load(&paths.clipboard()).unwrap();
        assert_eq!(clips.items()[0].clip.text(), Some("newer"));
        let usage: UsageLog = json_file::load(&paths.usage()).unwrap();
        assert_eq!(usage.sessions()[0].duration_ms(), 3_600_000);
        let calendar: AccountCache = json_file::load(&paths.local_calendar()).unwrap();
        assert_eq!(calendar.calendars["local"].events["e1"].title, "Lunch");
        let settings = load_settings(&paths);
        assert!(settings.clipboard.enabled, "the choice made before is kept");
        assert!(!settings.usage.enabled);
        assert!(paths.root.join(LEGACY_DATA).exists(), "the original stays");
    }

    #[test]
    fn it_imports_once_and_never_over_newer_data() {
        let paths = temp("once");
        std::fs::write(paths.root.join(LEGACY_DATA), OLD).unwrap();
        let mut mine = TodoList::default();
        mine.add("Newer");
        json_file::save(&paths.todo(), &mine).unwrap();

        assert_eq!(import_legacy_store(&paths), Ok(true));
        let todo: TodoList = json_file::load(&paths.todo()).unwrap();
        assert_eq!(todo.items().len(), 1, "what Keenu had stays");

        assert_eq!(import_legacy_store(&paths), Ok(false), "only once");
    }

    #[test]
    fn a_damaged_old_file_is_reported_and_left_alone() {
        let paths = temp("damaged");
        std::fs::write(paths.root.join(LEGACY_DATA), "{ nope").unwrap();
        assert!(import_legacy_store(&paths).is_err());
        assert_eq!(
            std::fs::read_to_string(paths.root.join(LEGACY_DATA)).unwrap(),
            "{ nope"
        );
        assert!(!paths.root.join(format!("{LEGACY_DATA}.imported")).exists());
    }

    #[test]
    fn exporting_gives_all_of_it_and_no_secret_is_there_to_give() {
        let paths = temp("export");
        let mut settings = Settings::default();
        settings.voice.endpoint = "https://voice.example/v1".into();
        json_file::save(&paths.settings(), &settings).unwrap();
        let mut log = UsageLog::default();
        log.record("code.exe", 0, 5_000);
        json_file::save(&paths.usage(), &log).unwrap();
        let out = paths.root.join("export.json");
        export_all(&paths, &out).unwrap();
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(text.contains("code.exe"));
        assert!(text.contains("voice.example"));
        assert!(!text.to_lowercase().contains("api_key"));
    }

    #[test]
    fn deleting_removes_records_and_keeps_settings() {
        let paths = temp("delete");
        json_file::save(&paths.settings(), &Settings::default()).unwrap();
        let mut log = UsageLog::default();
        log.record("code.exe", 0, 5_000);
        json_file::save(&paths.usage(), &log).unwrap();
        std::fs::create_dir_all(paths.clip_images()).unwrap();
        std::fs::write(paths.clip_images().join("a.png"), "png").unwrap();
        delete_all_data(&paths).unwrap();
        assert!(!paths.usage().exists());
        assert!(!paths.clip_images().exists());
        assert!(paths.settings().exists());
        delete_all_data(&paths).unwrap();
    }

    #[test]
    fn account_ids_cannot_leave_the_calendars_folder() {
        let paths = temp("names");
        let file = paths.calendar_cache("..\\..\\evil");
        assert_eq!(file.parent().unwrap(), paths.calendars());
        assert!(!file.to_string_lossy().contains(".."));
    }
}
