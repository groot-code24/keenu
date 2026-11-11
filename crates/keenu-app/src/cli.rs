// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line, for scripts and support: everything here works without the island running.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use keenu_core::clipboard::ClipboardHistory;
use keenu_core::file_shelf::{self, FileShelf};
use keenu_core::json_file;
use keenu_core::lyrics::Lyrics;
use keenu_core::reminders::Reminders;
use keenu_core::settings::Settings;
use keenu_core::todo::TodoList;
use keenu_core::usage_log::UsageLog;
use keenu_integrations::agents::{self, Agent};
use keenu_integrations::calendar::Event;
use keenu_integrations::calendar::engine::AccountCache;
use keenu_integrations::ical;
use keenu_platform::{MediaCommand, NativeNotification, PlaybackState};

use crate::app::Platform;
use crate::data::{self, Paths, load_or_default, load_settings, now_ms};

pub const HELP: &str = "Keenu: a notch-style desktop island

Usage: keenu [command]

With no command, starts the island (or shows the one already running).

  --version                     print the version
  --help                        print this
  --capabilities                what works on this computer, and why not where it does not
  --diagnostics                 versions, paths and capabilities, for a bug report; holds no secrets
  --monitors                    list the displays
  --enable-startup              start with Windows
  --disable-startup             do not
  --media-status                what is playing, as JSON
  --media-playpause | --media-next | --media-previous
  --notify <title> <text>       show a notification
  --lyrics <file.lrc> <ms>      the line sung at this time in a file
  --clipboard                   print the clipboard's text
  --clipboard-history-enable    start recording (off until you say so)
  --clipboard-history-disable   stop recording
  --clipboard-history-list      print the history
  --clipboard-history-copy <n>  copy entry n back
  --clipboard-history-clear     delete the history
  --shelf-list | --shelf-clear  the file shelf; the files themselves are untouched
  --shelf-open <n>              open file n
  --usage-enable                start recording the app in front (off until you say so)
  --usage-disable               stop
  --usage-report                time per app today
  --usage-export <file.csv>     everything recorded, as CSV
  --usage-clear                 delete it
  --updates-enable | --updates-disable    the daily look for a new version
  --todo-add <title> | --todo-list | --todo-done <id>
  --reminder-add <rfc3339> <text> | --reminder-list | --reminder-dismiss <id>
  --calendar-import <file.ics> | --calendar-export <file.ics> | --calendar-list
  --caldav-test <url> <user> <password>   list a server's calendars and each calendar's events, without saving
  --agent-install | --agent-uninstall | --agent-status    hooks for Claude Code and Codex
  --windows-list                the windows that are open
  --window-read <title>         what is in a window, as the assistant sees it
  --window-click <title> <name> press a control
  --window-type <title> <name> <text>    put text in a field; never a password field
  --window-keys <title> <keys>  press keys such as ctrl+s
  --window-close <title>        ask a window to close
  --system <json>               one of the assistant's system actions without the AI, such as
                                {\"type\":\"volume\",\"level\":30}, {\"type\":\"system_info\"} or
                                {\"type\":\"check\",\"command\":\"ipconfig\"}. It does not ask first.
  --assistant-log [count]       what the assistant did, newest first
  --export-data <file.json>     everything of yours except secrets
  --migration-status            what was brought over from an earlier version
  --migration-rollback          set Keenu's data folder aside so it is migrated again

Set KEENU_DATA_DIR to use another data folder.
";

/// Runs a command if `args` holds one. `None` means there is none and the island should start.
pub fn run(args: &[String], platform: &Platform, paths: &Paths) -> Option<Result<(), String>> {
    let command = args.first()?;
    Some(dispatch(command, &args[1..], platform, paths))
}

/// This system, as the assistant's read-only checks know it.
fn platform_name() -> &'static str {
    if cfg!(windows) {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else {
        "Linux"
    }
}

fn required<'a>(args: &'a [String], at: usize, usage: &str) -> Result<&'a str, String> {
    args.get(at)
        .map(String::as_str)
        .ok_or_else(|| format!("Usage: keenu {usage}"))
}

fn index(args: &[String], usage: &str) -> Result<usize, String> {
    required(args, 0, usage)?
        .parse::<usize>()
        .ok()
        .filter(|&n| n > 0)
        .map(|n| n - 1)
        .ok_or_else(|| format!("Usage: keenu {usage} (counting from 1)"))
}

fn save<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    json_file::save(path, value).map_err(|err| format!("Could not save {}: {err}", path.display()))
}

fn change_settings(paths: &Paths, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
    let mut settings = load_settings(paths);
    change(&mut settings);
    save(&paths.settings(), &settings)
}

fn media_command(platform: &Platform, command: MediaCommand) -> Result<(), String> {
    let sessions = platform.media.sessions()?;
    let session = sessions.first().ok_or("Nothing is playing.")?;
    platform.media.command(&session.id, command)
}

fn dispatch(
    command: &str,
    args: &[String],
    platform: &Platform,
    paths: &Paths,
) -> Result<(), String> {
    match command {
        "--version" => println!("{}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" => print!("{HELP}"),
        "--capabilities" => print!("{}", capabilities(platform)),
        "--diagnostics" => print!("{}", diagnostics(platform, paths)),
        "--monitors" => {
            for (number, monitor) in platform.monitors.monitors()?.iter().enumerate() {
                println!(
                    "{number}: {} x={} y={} {}x{} {}%{}",
                    monitor.name,
                    monitor.x,
                    monitor.y,
                    monitor.width,
                    monitor.height,
                    monitor.scale_percent,
                    if monitor.is_primary { " primary" } else { "" }
                );
            }
        }
        "--enable-startup" => {
            platform.lifecycle.set_start_at_login(true)?;
            change_settings(paths, |settings| settings.window.start_at_login = true)?;
        }
        "--disable-startup" => {
            platform.lifecycle.set_start_at_login(false)?;
            change_settings(paths, |settings| settings.window.start_at_login = false)?;
        }
        "--media-status" => {
            let sessions = platform.media.sessions()?;
            let list: Vec<_> = sessions
                .iter()
                .map(|session| {
                    serde_json::json!({
                        "app": session.app_name,
                        "id": session.id,
                        "title": session.title,
                        "artist": session.artist,
                        "album": session.album,
                        "playing": session.state == PlaybackState::Playing,
                        "positionMs": session.position_ms,
                        "durationMs": session.duration_ms,
                        "hasArtwork": session.has_artwork,
                    })
                })
                .collect();
            println!("{}", serde_json::Value::Array(list));
        }
        "--media-playpause" => media_command(platform, MediaCommand::PlayPause)?,
        "--media-next" => media_command(platform, MediaCommand::Next)?,
        "--media-previous" => media_command(platform, MediaCommand::Previous)?,
        "--notify" => {
            let title = required(args, 0, "--notify <title> <text>")?;
            platform.notifier.show(&NativeNotification {
                tag: "cli".into(),
                title: title.into(),
                body: args.get(1..).unwrap_or_default().join(" "),
                actions: Vec::new(),
            })?;
        }
        "--lyrics" => {
            let usage = "--lyrics <file.lrc> <position-ms>";
            let file = required(args, 0, usage)?;
            let position: i64 = required(args, 1, usage)?
                .parse()
                .map_err(|_| format!("Usage: keenu {usage}"))?;
            let bytes =
                std::fs::read(file).map_err(|err| format!("Could not read {file}: {err}"))?;
            let lyrics = Lyrics::from_lrc_bytes(&bytes).ok_or("That file has no timed lyrics.")?;
            if let Some(line) = lyrics.active(position) {
                println!("{}", lyrics.lines()[line].text);
            }
        }
        "--windows-list" => {
            for window in platform.control.windows()? {
                println!(
                    "{}\t{}\t{}{}",
                    window.id,
                    window.app,
                    window.title,
                    if window.focused { "\t(in front)" } else { "" }
                );
            }
        }
        "--window-read" => {
            let title = required(args, 0, "--window-read <title>")?;
            for element in platform.control.read_window(title, 500)? {
                println!(
                    "{}\t{}\t{}{}{}",
                    element.role,
                    element.name,
                    element.value,
                    if element.secret {
                        "\t(password field)"
                    } else {
                        ""
                    },
                    if element.enabled {
                        ""
                    } else {
                        "\t(greyed out)"
                    }
                );
            }
        }
        "--window-click" => {
            let usage = "--window-click <title> <name>";
            let said = platform
                .control
                .click(required(args, 0, usage)?, required(args, 1, usage)?)?;
            println!("{said}");
        }
        "--window-type" => {
            let usage = "--window-type <title> <name> <text>";
            let said = platform.control.set_text(
                required(args, 0, usage)?,
                required(args, 1, usage)?,
                required(args, 2, usage)?,
            )?;
            println!("{said}");
        }
        "--window-keys" => {
            let usage = "--window-keys <title> <keys>";
            platform
                .control
                .press_keys(required(args, 0, usage)?, required(args, 1, usage)?)?;
        }
        "--window-close" => {
            platform
                .control
                .close_window(required(args, 0, "--window-close <title>")?)?;
        }
        // One of the assistant's system actions or reports, as it would send it, without the AI: for trying them.
        "--system" => {
            let usage = r#"--system {"type":"volume","level":30}"#;
            let action: serde_json::Value = serde_json::from_str(required(args, 0, usage)?)
                .map_err(|err| format!("{usage}: {err}"))?;
            let kind = action["type"].as_str().unwrap_or_default();
            let said = if kind == "check" {
                let line = action["command"].as_str().unwrap_or_default();
                let (program, words) = keenu_assistant::system::check_line(platform_name(), line)?;
                keenu_assistant::system::run_check(line, &program, &words)
            } else if let Some(report) = keenu_assistant::system::report(kind) {
                platform.control.report(report)?
            } else {
                match keenu_assistant::system::command(&action) {
                    Some(command) => platform.control.system(&command?)?,
                    None => return Err(format!("“{kind}” is not a system action.")),
                }
            };
            println!("{said}");
        }
        "--assistant-log" => {
            let count = args
                .first()
                .and_then(|count| count.parse().ok())
                .unwrap_or(30);
            for entry in keenu_guard::ActionLog::new(paths.assistant_log()).newest(count) {
                let at = DateTime::<Utc>::from_timestamp_millis(entry.at_ms).unwrap_or_default();
                println!(
                    "{at}\t{:?}\t{}\t{}",
                    entry.outcome, entry.action, entry.detail
                );
            }
        }
        "--clipboard" => match platform.clipboard.read()? {
            Some(keenu_platform::ClipboardContent::Text(text))
            | Some(keenu_platform::ClipboardContent::Rich { text, .. }) => println!("{text}"),
            _ => println!("The clipboard holds no text."),
        },
        "--clipboard-history-enable" => {
            change_settings(paths, |settings| settings.clipboard.enabled = true)?;
            println!("Clipboard history is on. It stays on this computer.");
        }
        "--clipboard-history-disable" => {
            change_settings(paths, |settings| settings.clipboard.enabled = false)?;
            println!("Clipboard history is off. What it kept is still there until you clear it.");
        }
        "--clipboard-history-list" => {
            let history: ClipboardHistory = load_or_default(&paths.clipboard());
            for (number, item) in history.items().iter().enumerate() {
                println!(
                    "{}: [{}] {}",
                    number + 1,
                    item.clip.kind(),
                    item.clip.preview()
                );
            }
        }
        "--clipboard-history-copy" => {
            let at = index(args, "--clipboard-history-copy <n>")?;
            let history: ClipboardHistory = load_or_default(&paths.clipboard());
            let item = history.items().get(at).ok_or("No such entry.")?;
            let content = crate::app::clip_to_content(&item.clip, paths)
                .ok_or("That entry's file is gone.")?;
            platform.clipboard.write(&content)?;
        }
        "--clipboard-history-clear" => {
            save(&paths.clipboard(), &ClipboardHistory::default())?;
            let _ = std::fs::remove_dir_all(paths.clip_images());
            println!("Clipboard history cleared.");
        }
        "--shelf-list" => {
            let shelf: FileShelf = load_or_default(&paths.shelf());
            for (number, entry) in shelf.entries().iter().enumerate() {
                println!(
                    "{}: {} ({})",
                    number + 1,
                    entry.path.display(),
                    file_shelf::format_size(entry.size)
                );
            }
        }
        "--shelf-open" => {
            let at = index(args, "--shelf-open <n>")?;
            let shelf: FileShelf = load_or_default(&paths.shelf());
            let entry = shelf
                .entries()
                .get(at)
                .ok_or("No such file on the shelf.")?;
            platform.shelf.open(&entry.path)?;
        }
        "--shelf-clear" => {
            let mut shelf: FileShelf = load_or_default(&paths.shelf());
            shelf.clear_unpinned();
            save(&paths.shelf(), &shelf)?;
            println!(
                "Shelf cleared, except what is pinned. The files themselves were not touched."
            );
        }
        "--usage-enable" => {
            change_settings(paths, |settings| settings.usage.enabled = true)?;
            println!(
                "Recording the app in front: its name and for how long, on this computer only."
            );
        }
        "--usage-disable" => {
            change_settings(paths, |settings| settings.usage.enabled = false)?;
            println!("Usage recording is off.");
        }
        "--usage-report" => {
            let log: UsageLog = load_or_default(&paths.usage());
            let settings = load_settings(paths);
            let rows = log.summary(crate::app::start_of_today_ms(), &settings.usage.categories);
            if rows.is_empty() {
                println!("Nothing recorded today.");
            }
            for row in rows {
                println!(
                    "{}: {}",
                    row.app,
                    keenu_ui::scene::format_duration(row.duration_ms)
                );
            }
        }
        "--usage-export" => {
            let file = required(args, 0, "--usage-export <file.csv>")?;
            let log: UsageLog = load_or_default(&paths.usage());
            let settings = load_settings(paths);
            std::fs::write(file, log.to_csv(&settings.usage.categories))
                .map_err(|err| format!("Could not write {file}: {err}"))?;
        }
        "--usage-clear" => {
            save(&paths.usage(), &UsageLog::default())?;
            println!("Usage record deleted.");
        }
        "--updates-enable" => {
            change_settings(paths, |settings| settings.updates.check = true)?;
            println!("Keenu will look for a new version once a day.");
        }
        "--updates-disable" => {
            change_settings(paths, |settings| {
                settings.updates.check = false;
                settings.updates.auto_install = false;
            })?;
            println!("Keenu will not look for new versions.");
        }
        "--todo-add" => {
            let mut list: TodoList = load_or_default(&paths.todo());
            let title = args.join(" ");
            let id = list.add(&title).ok_or("A task needs a title.")?;
            save(&paths.todo(), &list)?;
            println!("Created task {id}");
        }
        "--todo-list" => {
            let list: TodoList = load_or_default(&paths.todo());
            for task in list.items() {
                println!(
                    "[{}] {}  {}",
                    if task.done { "x" } else { " " },
                    task.id,
                    task.title
                );
            }
        }
        "--todo-done" => {
            let id: u64 = required(args, 0, "--todo-done <id>")?
                .parse()
                .map_err(|_| "That is not a task id.")?;
            let mut list: TodoList = load_or_default(&paths.todo());
            if !list.set_done(id, true, now_ms()) {
                return Err("Task not found.".into());
            }
            save(&paths.todo(), &list)?;
        }
        "--reminder-add" => {
            let usage = "--reminder-add <rfc3339-time> <text>";
            let due = DateTime::parse_from_rfc3339(required(args, 0, usage)?)
                .map_err(|_| "The time must look like 2026-10-30T18:00:00+05:30.")?
                .with_timezone(&Utc);
            let mut list: Reminders = load_or_default(&paths.reminders());
            let text = args.get(1..).unwrap_or_default().join(" ");
            let id = list
                .add(&text, due.timestamp_millis())
                .ok_or("A reminder needs text.")?;
            save(&paths.reminders(), &list)?;
            println!("Created reminder {id}");
        }
        "--reminder-list" => {
            let list: Reminders = load_or_default(&paths.reminders());
            for reminder in list.items() {
                let due = DateTime::<Utc>::from_timestamp_millis(reminder.due_ms)
                    .map(|due| due.to_rfc3339())
                    .unwrap_or_default();
                println!("{}  {due}  {}", reminder.id, reminder.text);
            }
        }
        "--reminder-dismiss" => {
            let id: u64 = required(args, 0, "--reminder-dismiss <id>")?
                .parse()
                .map_err(|_| "That is not a reminder id.")?;
            let mut list: Reminders = load_or_default(&paths.reminders());
            if !list.dismiss(id) {
                return Err("Reminder not found.".into());
            }
            save(&paths.reminders(), &list)?;
        }
        "--calendar-import" => {
            let file = required(args, 0, "--calendar-import <file.ics>")?;
            let text = std::fs::read_to_string(file)
                .map_err(|err| format!("Could not read {file}: {err}"))?;
            let count = import_ics(paths, &text)?;
            println!("Imported {count} calendar event(s).");
        }
        "--calendar-export" => {
            let file = required(args, 0, "--calendar-export <file.ics>")?;
            let cache: AccountCache = load_or_default(&paths.local_calendar());
            let mut out = String::new();
            let mut count = 0;
            for state in cache.calendars.values() {
                for event in state.events.values() {
                    out.push_str(&event_ics(event));
                    count += 1;
                }
            }
            std::fs::write(file, out).map_err(|err| format!("Could not write {file}: {err}"))?;
            println!("Exported {count} calendar event(s).");
        }
        "--calendar-list" => {
            let cache: AccountCache = load_or_default(&paths.local_calendar());
            for item in cache.agenda(0, i64::MAX / 2) {
                let start = DateTime::<Utc>::from_timestamp_millis(item.start_ms)
                    .map(|time| time.to_rfc3339())
                    .unwrap_or_default();
                println!("{start}  {}", item.title);
            }
        }
        "--caldav-test" => caldav_test(args)?,
        "--agent-install" => {
            let exe = std::env::current_exe().map_err(|err| err.to_string())?;
            for agent in Agent::ALL {
                agents::install(agent, &agents::hook_command(&exe, agent))
                    .map_err(|err| format!("{}: {err:#}", agent.title()))?;
                println!("Hooks installed for {}.", agent.title());
                if let Some(note) = agent.after_install() {
                    println!("{note}");
                }
            }
        }
        "--agent-uninstall" => {
            for agent in Agent::ALL {
                agents::uninstall(agent).map_err(|err| format!("{}: {err:#}", agent.title()))?;
            }
            println!("Hooks removed.");
        }
        "--agent-status" => {
            for agent in Agent::ALL {
                println!(
                    "{}: {}",
                    agent.title(),
                    if agents::installed(agent) {
                        "installed"
                    } else {
                        "not installed"
                    }
                );
            }
        }
        "--export-data" => {
            let file = required(args, 0, "--export-data <file.json>")?;
            data::export_all(paths, Path::new(file))?;
            println!("Exported to {file}. It holds no passwords or tokens.");
        }
        "--migration-status" => {
            match std::fs::read_to_string(paths.root.join(keenu_core::migration::LOG_FILE)) {
                Ok(text) => print!("{text}"),
                Err(_) => println!("Nothing has been migrated."),
            }
        }
        "--migration-rollback" => match keenu_core::migration::roll_back(&paths.root) {
            Ok(Some(aside)) => {
                println!("Keenu's data folder was set aside as {}.", aside.display())
            }
            Ok(None) => println!("There is nothing to set aside."),
            Err(err) => return Err(err.to_string()),
        },
        other => return Err(format!("Unknown command: {other}. Try keenu --help.")),
    }
    Ok(())
}

/// Adds the events of iCalendar text to the local calendar; how many were new.
fn import_ics(paths: &Paths, text: &str) -> Result<usize, String> {
    let mut cache: AccountCache = load_or_default(&paths.local_calendar());
    let state = cache.calendars.entry("local".into()).or_default();
    let mut added = 0;
    for event in ical::parse(text) {
        let id = if event.uid.is_empty() {
            format!("{}", event.start.to_utc().timestamp())
        } else {
            event.uid.clone()
        };
        if state.events.contains_key(&id) {
            continue;
        }
        let start = event.start.to_utc();
        state.events.insert(
            id.clone(),
            Event {
                id,
                etag: String::new(),
                title: event.summary.clone(),
                location: event.location.clone(),
                start_ms: start.timestamp_millis(),
                end_ms: event
                    .end
                    .map_or(start, ical::When::to_utc)
                    .timestamp_millis(),
                all_day: event.start.is_date(),
                ics: Some(ical::to_ics(&event, None)),
            },
        );
        added += 1;
    }
    save(&paths.local_calendar(), &cache)?;
    Ok(added)
}

fn event_ics(event: &Event) -> String {
    if let Some(ics) = &event.ics {
        return ics.clone();
    }
    let start = DateTime::<Utc>::from_timestamp_millis(event.start_ms).unwrap_or_default();
    let end = DateTime::<Utc>::from_timestamp_millis(event.end_ms).unwrap_or(start);
    ical::to_ics(
        &ical::VEvent {
            uid: event.id.clone(),
            summary: event.title.clone(),
            description: String::new(),
            location: event.location.clone(),
            start: ical::When::Utc(start),
            end: Some(ical::When::Utc(end)),
            rule: None,
            exceptions: Vec::new(),
            recurrence_id: None,
            cancelled: false,
        },
        None,
    )
}

/// For checking a CalDAV server, from a script or by hand: nothing is saved.
fn caldav_test(args: &[String]) -> Result<(), String> {
    use keenu_integrations::calendar::caldav::CalDav;
    use keenu_integrations::calendar::{CalendarProvider, EventDraft};
    let usage = "--caldav-test <url> <user> <password> [--write]";
    let account = CalDav::new(
        required(args, 0, usage)?,
        required(args, 1, usage)?,
        required(args, 2, usage)?,
    )
    .map_err(|err| err.to_string())?;
    let calendars = account.calendars().map_err(|err| err.to_string())?;
    println!("{} calendar(s)", calendars.len());
    for calendar in &calendars {
        let first = account
            .sync(&calendar.id, None)
            .map_err(|err| err.to_string())?;
        println!(
            "{}: {} event(s), token {}",
            calendar.name,
            first.changed.len(),
            first.token.is_some()
        );
        if args.get(3).is_some_and(|flag| flag == "--write") {
            let now = now_ms();
            let draft = EventDraft {
                title: "Keenu interoperability test".into(),
                location: "Nowhere".into(),
                description: String::new(),
                start_ms: now + 3_600_000,
                end_ms: now + 7_200_000,
                all_day: false,
                time_zone: "UTC".into(),
                recurrence: Some("FREQ=DAILY;COUNT=3".into()),
            };
            let created = account
                .create(&calendar.id, &draft)
                .map_err(|err| err.to_string())?;
            println!("created {} etag {}", created.id, created.etag);
            let after_create = account
                .sync(&calendar.id, first.token.as_deref())
                .map_err(|err| err.to_string())?;
            println!(
                "incremental: {} changed, {} removed",
                after_create.changed.len(),
                after_create.removed.len()
            );
            let etag = after_create
                .changed
                .iter()
                .find(|event| event.id == created.id)
                .map_or(created.etag.clone(), |event| event.etag.clone());
            let mut changed = draft.clone();
            changed.title = "Keenu test, changed".into();
            let updated = account
                .update(&calendar.id, &created.id, &etag, &changed)
                .map_err(|err| err.to_string())?;
            println!("updated, etag {}", updated.etag);
            let stale = account.update(&calendar.id, &created.id, &etag, &draft);
            println!(
                "stale update refused as conflict: {}",
                stale == Err(keenu_integrations::calendar::CalendarError::Conflict)
            );
            let after_update = account
                .sync(&calendar.id, after_create.token.as_deref())
                .map_err(|err| err.to_string())?;
            let current = after_update
                .changed
                .iter()
                .find(|event| event.id == created.id)
                .map_or(updated.etag.clone(), |event| event.etag.clone());
            account
                .delete(&calendar.id, &created.id, &current)
                .map_err(|err| err.to_string())?;
            let after_delete = account
                .sync(&calendar.id, after_update.token.as_deref())
                .map_err(|err| err.to_string())?;
            println!(
                "deleted; incremental reports removed: {}",
                after_delete.removed.iter().any(|id| id == &created.id)
            );
        }
    }
    Ok(())
}

/// What works here, in words.
pub fn capabilities(platform: &Platform) -> String {
    let mut out = String::new();
    let line = |name: &str, availability: keenu_platform::Availability| -> String {
        match availability.reason() {
            None => format!("{name}: available\n"),
            Some(why) => format!("{name}: {why}\n"),
        }
    };
    out.push_str(&line("Media", platform.media.availability()));
    out.push_str(&line("Notifications", platform.notifier.availability()));
    out.push_str(&line("Clipboard", platform.clipboard.availability()));
    out.push_str(&line("Voice", platform.voice.availability()));
    out.push_str(&line("App usage", platform.usage.availability()));
    out.push_str(&line("Updates", platform.updates.availability()));
    let oauth = |name: &str, configured: bool, variable: &str| -> String {
        if configured {
            format!("{name}: available\n")
        } else {
            format!("{name}: not configured; set {variable} (see docs/calendar-setup.md)\n")
        }
    };
    out.push_str("CalDAV: available with your server's address and password\n");
    out.push_str(&oauth(
        "Google Calendar",
        keenu_integrations::calendar::google::provider().is_some(),
        "KEENU_GOOGLE_CLIENT_ID",
    ));
    out.push_str(&oauth(
        "Microsoft 365",
        keenu_integrations::calendar::graph::provider().is_some(),
        "KEENU_MICROSOFT_CLIENT_ID",
    ));
    out
}

/// Support information. Nothing secret goes in, and the lot goes through the redactor in case.
pub fn diagnostics(platform: &Platform, paths: &Paths) -> String {
    let settings = load_settings(paths);
    let mut out = format!(
        "Keenu {}\n{} {}\nData folder: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        paths.root.display(),
    );
    out.push_str(&capabilities(platform));
    if let Ok(monitors) = platform.monitors.monitors() {
        for monitor in monitors {
            out.push_str(&format!(
                "Display {}: {}x{} at {},{} {}%\n",
                monitor.name,
                monitor.width,
                monitor.height,
                monitor.x,
                monitor.y,
                monitor.scale_percent
            ));
        }
    }
    out.push_str(&format!(
        "Clipboard history: {}\nUsage recording: {}\nVoice: {}\nOnline lyrics: {}\nUpdate checks: {}\nCalendar accounts: {}\n",
        settings.clipboard.enabled,
        settings.usage.enabled,
        settings.voice.enabled,
        settings.lyrics.online,
        settings.updates.check,
        settings.calendar.accounts.len(),
    ));
    for agent in Agent::ALL {
        out.push_str(&format!(
            "{} hooks: {}\n",
            agent.title(),
            agents::installed(agent)
        ));
    }
    keenu_core::redact::redact(&out)
}

/// How long a hook waits for the island, from the user's setting.
pub fn hook_timeout(paths: &Paths) -> Duration {
    Duration::from_secs(u64::from(load_settings(paths).agents.answer_timeout_secs))
}
