// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Linux's side of the agent loop (see keenu_assistant::agent): the tools keenud runs, and commands as systemd units.

use std::path::{Path, PathBuf};

use log::warn;
use serde_json::Value;

use keenu_assistant::agent::Toolbox;
pub use keenu_assistant::agent::{APPROVAL, Act, Ask, Desk, Out, TooLong, act, after_close};
use keenu_guard::{ActionLog, Decision, Entry, Outcome, Policy, level_of};

use super::answer::Answer;
use super::tasks::{self, Ended, Scope};
use super::{apps, check, files};

/// The actions keenud runs itself, for the AI, rather than the extension for the user.
const TOOLS: &[&str] = &[
    "find_files",
    "list_folder",
    "recent_files",
    "read_text",
    "find_apps",
    "top_processes",
    "chrome_profiles",
    "check",
];

/// One of `TOOLS`.
fn is_tool(action: &Value) -> bool {
    action["type"]
        .as_str()
        .is_some_and(|kind| TOOLS.contains(&kind))
}

/// The user's home folder, empty if it cannot be found.
fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// What the island says while a tool runs, in the user's words, e.g. "Searching Downloads for “ttd”…":
/// folders by name, and a check by what it looks at, never its command line.
fn describe(call: &Value) -> String {
    match call["type"].as_str() {
        Some("find_files") => {
            let terms = strings(&call["terms"]);
            let within = call["in"].as_str().unwrap_or_default();
            let place = folder_name(within).unwrap_or_else(|| "your files".into());
            match terms.first() {
                Some(term) => format!("Searching {place} for “{term}”…"),
                None => format!("Searching {place}…"),
            }
        }
        Some("list_folder") => match folder_name(path_of(call)) {
            Some(name) => format!("Looking in {name}…"),
            None => "Looking in your home folder…".into(),
        },
        Some("recent_files") => "Looking at your recent files…".into(),
        Some("read_text") => format!(
            "Reading {}…",
            path_of(call).rsplit('/').next().unwrap_or("the file")
        ),
        Some("find_apps") => match strings(&call["terms"]).first() {
            Some(term) => format!("Looking for an app for “{term}”…"),
            None => "Looking through your apps…".into(),
        },
        Some("top_processes") => "Checking what’s using the computer…".into(),
        Some("check") => check_words(call["command"].as_str().unwrap_or_default()).into(),
        Some("chrome_profiles") => "Looking at Chrome’s profiles…".into(),
        _ => "Working…".into(),
    }
}

/// A folder by its name ("Downloads"); `None` for the home folder itself.
fn folder_name(path: &str) -> Option<String> {
    let path = path.trim().trim_end_matches('/');
    let home = home();
    if path.is_empty() || path == "~" || std::path::Path::new(path) == home {
        return None;
    }
    path.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// What a read-only check looks at, in plain words.
fn check_words(command: &str) -> &'static str {
    let program = command.split_whitespace().next().unwrap_or_default();
    match program {
        "journalctl" => "Reading the system log…",
        "ping" | "iw" | "rfkill" | "nmcli" | "ip" | "resolvectl" => "Checking the network…",
        "apt-cache" | "apt" | "dpkg" | "dpkg-query" | "rpm" | "dnf" | "pacman" | "flatpak"
        | "snap" | "npm" | "pipx" | "which" | "dkms" | "ubuntu-drivers" | "fwupdmgr"
        | "xdg-mime" | "xdg-settings" => "Checking what’s installed…",
        _ if command.contains("--version") => "Checking what’s installed…",
        _ => "Checking this computer…",
    }
}

/// The `path` asked for, or ~.
fn path_of(call: &Value) -> &str {
    call["path"]
        .as_str()
        .filter(|p| !p.is_empty())
        .unwrap_or("~")
}

/// A list of words, or one word on its own, as AIs write either.
fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str().map(str::to_owned))
            .collect(),
        Value::String(text) => vec![text.clone()],
        _ => Vec::new(),
    }
}

/// Runs one tool, off the async threads, and returns its result as text for the AI.
async fn run(call: Value) -> String {
    if call["type"] == "check" {
        return check::run(call["command"].as_str().unwrap_or_default()).await;
    }
    tokio::task::spawn_blocking(move || run_now(&call, &home()))
        .await
        .unwrap_or_else(|err| format!("the search failed: {err}"))
}

/// The file and app tools, blocking.
fn run_now(call: &Value, home: &Path) -> String {
    match call["type"].as_str() {
        Some("find_files") => match files::root(call["in"].as_str().unwrap_or(""), home) {
            Ok(root) => files::find(
                &strings(&call["terms"]),
                &root,
                files::Kind::parse(call["kind"].as_str().unwrap_or("")),
                home,
            ),
            Err(why) => format!("find_files: {why}."),
        },
        Some("list_folder") => match files::root(path_of(call), home) {
            Ok(folder) => files::list_folder(&folder, home),
            Err(why) => format!("list_folder: {why}."),
        },
        Some("recent_files") => files::recent_files(home),
        Some("read_text") => match files::root(path_of(call), home) {
            Ok(path) => files::read_text(&path, home),
            Err(why) => format!("read_text: {why}."),
        },
        Some("find_apps") => apps::find_apps(&strings(&call["terms"]), home),
        Some("top_processes") => apps::top_processes(),
        Some("chrome_profiles") => files::chrome_profiles(home),
        _ => String::new(),
    }
}

/// Linux's tools and commands.
pub struct LinuxTools {
    log: ActionLog,
}

impl LinuxTools {
    pub fn new() -> Self {
        Self {
            log: ActionLog::new(crate::store::data_dir().join("assistant-actions.log")),
        }
    }
}

impl Toolbox for LinuxTools {
    fn is_tool(&self, action: &Value) -> bool {
        is_tool(action)
    }

    fn describe(&self, call: &Value) -> String {
        describe(call)
    }

    async fn run(&self, call: Value) -> String {
        run(call).await
    }

    async fn run_command(
        &self,
        desk: &Desk,
        out: &Out<'_>,
        action: &Value,
        ran: &mut Vec<(String, String)>,
    ) -> String {
        run_command(desk, out, action, ran).await
    }

    /// Refuses what only the user may do, and logs the rest. Commands have their card already, and GNOME's own
    /// actions are limited to the list in the instructions, so nothing new asks here.
    async fn gate(&self, action: &Value) -> Option<String> {
        let refused = match Policy::default().decide(action) {
            Decision::Refuse(why) => Some(why),
            Decision::Run | Decision::Ask => None,
        };
        let entry = Entry {
            at_ms: crate::clock::now_ms(),
            request: String::new(),
            level: level_of(action),
            action: keenu_guard::describe(action),
            outcome: if refused.is_some() {
                Outcome::Refused
            } else {
                Outcome::Approved
            },
            detail: String::new(),
        };
        if !cfg!(test)
            && let Err(why) = self.log.add(&entry)
        {
            warn!("{why}");
        }
        refused
    }
}

/// Asks the AI and runs each step until it answers, with Linux's tools.
pub async fn think(
    brain: &mut impl Ask,
    prompt: &str,
    out: &Out<'_>,
    desk: &Desk,
) -> anyhow::Result<Answer> {
    keenu_assistant::agent::think(brain, prompt, out, desk, &LinuxTools::new()).await
}

/// Offers a command to the user (the card: Run or Deny) and, run, follows it to its end; what came of it,
/// as text for the AI. `ran` keeps it for the conversation.
pub async fn run_command(
    desk: &Desk,
    out: &Out<'_>,
    action: &Value,
    ran: &mut Vec<(String, String)>,
) -> String {
    let asked = action["command"].as_str().unwrap_or_default().trim();
    let (command, root) = match tasks::without_sudo(
        asked,
        action["root"].as_bool().unwrap_or(false),
    ) {
        Ok(found) => found,
        Err(why) => {
            return format!(
                "run `{asked}` was not offered: {why}. Offer the terminal (typed in for the user) instead."
            );
        }
    };
    if command.is_empty() {
        return "run: no command was given.".into();
    }
    // The card shows the whole command, so the user sees all they run: one line, and not too long.
    if !tasks::shows_whole(&command, tasks::MAX_COMMAND) {
        return format!(
            "run `{asked}` was not offered: a command must be one line of at most {} characters, without \
             hidden characters. Join steps with && or offer them one at a time.",
            tasks::MAX_COMMAND
        );
    }
    if let Some(why) = tasks::refused(&command) {
        ran.push((asked.to_owned(), "denied".into()));
        return format!(
            "run `{asked}` was refused by Keenu without asking the user: {why}. Never offer it again."
        );
    }
    let why = action["why"]
        .as_str()
        .filter(|w| !w.trim().is_empty())
        .unwrap_or("Run a command")
        .trim();
    let detail = action["detail"].as_str().unwrap_or_default().trim();
    let scope = if root { Scope::System } else { Scope::User };
    let task = match tasks::prepare(&command, why, scope) {
        Ok(task) => task,
        Err(err) => return format!("run `{asked}` could not start: {err}."),
    };
    let risks = tasks::risks(&command, root);
    let card = |status: &str, progress: Option<&tasks::Progress>| {
        tasks::card(&task, detail, &risks, status, progress)
    };
    out.cards.send(card("waiting", None)).ok();
    let mut offered = card("waiting", None);
    offered["type"] = "run".into();
    offered["spawn"] = serde_json::json!(task.spawn());
    let report = act(desk, vec![offered]).await;
    let shown = card("waiting", None)["command"]
        .as_str()
        .unwrap_or(asked)
        .to_owned();
    if !report.ok {
        // The Deny button (the extension's own word), or the password dialog closed ("Access denied" from
        // polkit says so too, so this goes first): either way the user chose not to.
        let no_password = report.text.contains("password was not given");
        let denied = !no_password && report.text.contains("run failed: denied");
        // Nobody pressed Run or Deny in time: not run, and not refused either.
        let unanswered = report.text.contains("run failed: no answer in time");
        let status = if denied || no_password {
            "denied"
        } else if unanswered {
            "stopped"
        } else {
            "failed"
        };
        out.cards.send(card(status, None)).ok();
        tasks::clean(&task);
        ran.push((shown.clone(), status.into()));
        return if denied {
            format!("run `{shown}`: the user denied it. Do not run it; offer another way, or stop.")
        } else if unanswered {
            format!(
                "run `{shown}` was not run: nobody pressed Run or Deny within 10 minutes. Say so, and offer \
                 it again if they still want it."
            )
        } else if no_password {
            format!(
                "run `{shown}` did not start: the password dialog was closed without the password ({}). \
                 The user chose not to; say so.",
                report.text
            )
        } else {
            format!(
                "run `{shown}` did not start ({}). Say plainly that it could not start and why; do not \
                 guess another reason.",
                report.text
            )
        };
    }
    tasks::remember(&task);
    let (ended, output) = tasks::follow(&task, |progress| {
        let status = match progress.ended {
            None => "running",
            Some(Ended::Exited(0)) => "done",
            Some(Ended::Exited(_)) => "failed",
            Some(Ended::Stopped) => "stopped",
        };
        out.cards.send(card(status, Some(&progress))).ok();
    })
    .await;
    tasks::forget(&task);
    tasks::clean(&task);
    let output = if output.is_empty() {
        "(no output)".to_owned()
    } else {
        output
    };
    match ended {
        Ended::Exited(0) => {
            ran.push((shown.clone(), "done".into()));
            format!(
                "run `{shown}` finished (exit 0). The end of its output:\n{output}\nCheck that it worked (a check) before saying so."
            )
        }
        Ended::Exited(code) => {
            ran.push((shown.clone(), "failed".into()));
            format!(
                "run `{shown}` failed (exit {code}). The end of its output:\n{output}\nExplain why plainly; try another way if there is one."
            )
        }
        Ended::Stopped => {
            ran.push((shown.clone(), "stopped".into()));
            format!(
                "run `{shown}` was stopped before it finished (by the user, or its time ran out). Output so far:\n{output}"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use keenu_assistant::agent::{MAX_STEPS, NUDGE, OUT_OF_STEPS, act_timeout, excuse};
    use serde_json::json;
    use tokio::sync::mpsc::UnboundedSender;

    use super::*;

    #[test]
    fn describes_each_step() {
        let find =
            json!({"type": "find_files", "terms": ["ttd", "signature"], "in": "~/Downloads"});
        assert_eq!(describe(&find), "Searching Downloads for “ttd”…");
        assert_eq!(
            describe(&json!({"type": "find_files", "terms": "cv"})),
            "Searching your files for “cv”…"
        );
        let list = json!({"type": "list_folder", "path": "~/Documents/Work/"});
        assert_eq!(describe(&list), "Looking in Work…");
        let home = json!({"type": "list_folder", "path": "~"});
        assert_eq!(describe(&home), "Looking in your home folder…");
        let check = |command| describe(&json!({"type": "check", "command": command}));
        assert_eq!(check("dpkg -l vlc"), "Checking what’s installed…");
        assert_eq!(check("code --version"), "Checking what’s installed…");
        assert_eq!(
            check("journalctl --user -b -p err"),
            "Reading the system log…"
        );
        assert_eq!(check("uname -a"), "Checking this computer…");
        assert!(is_tool(&find) && !is_tool(&json!({"type": "open_url"})));
    }

    /// The steps, drafts and cards of a test, all to one channel nobody reads.
    macro_rules! out {
        ($activity:expr) => {
            Out {
                activity: &$activity,
                draft: &$activity,
                cards: &unbounded_cards(),
            }
        };
    }

    fn unbounded_cards() -> UnboundedSender<Value> {
        tokio::sync::mpsc::unbounded_channel().0
    }

    /// Answers from a script, keeping each prompt it was given.
    struct Scripted {
        answers: std::collections::VecDeque<Value>,
        prompts: Vec<String>,
    }

    impl Scripted {
        fn new(answers: impl IntoIterator<Item = Value>) -> Self {
            Self {
                answers: answers.into_iter().collect(),
                prompts: Vec::new(),
            }
        }
    }

    impl Ask for Scripted {
        async fn ask(
            &mut self,
            prompt: &str,
            _activity: &UnboundedSender<String>,
            _draft: &UnboundedSender<String>,
        ) -> anyhow::Result<Answer> {
            self.prompts.push(prompt.to_owned());
            let next = self
                .answers
                .pop_front()
                .unwrap_or(json!({"say": "ok", "actions": []}));
            Ok(serde::Deserialize::deserialize(next)?)
        }
    }

    /// A desk that reports every action done, as the extension would.
    fn desk() -> (Desk, tokio::task::JoinHandle<usize>) {
        desk_where(|_| true)
    }

    /// A desk where an action works when `works` says so.
    fn desk_where(works: fn(&Value) -> bool) -> (Desk, tokio::task::JoinHandle<usize>) {
        let (desk, mut acts) = tokio::sync::mpsc::unbounded_channel::<Act>();
        let handled = tokio::spawn(async move {
            let mut batches = 0;
            while let Some(act) = acts.recv().await {
                batches += 1;
                let results = act
                    .actions
                    .iter()
                    .map(|a| json!({"type": a["type"], "ok": works(a), "detail": "opened"}))
                    .collect();
                act.results.send(results).ok();
            }
            batches
        });
        (desk, handled)
    }

    #[tokio::test]
    async fn takes_one_step_after_another() {
        let mut brain = Scripted::new([
            json!({"say": "Opening Files first", "done": false,
                "actions": [{"type": "open_app", "app": "Files"}]}),
            json!({"say": "Done", "actions": [{"type": "window", "command": "left", "app": "Files"}]}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, handled) = desk();
        let answer = think(
            &mut brain,
            "open files then put it left",
            &out!(activity),
            &desk,
        )
        .await
        .unwrap();
        drop(desk);
        // Both steps ran, the last before it answered.
        assert_eq!(handled.await.unwrap(), 2);
        assert!(answer.checked && answer.actions[0]["command"] == "left");
        assert!(
            brain.prompts[1].contains("- open_app done: opened"),
            "{}",
            brain.prompts[1]
        );
    }

    #[tokio::test]
    async fn a_step_taken_puts_its_written_words_away() {
        let mut brain = Scripted::new([
            json!({"say": "Opening Files first", "done": false, "actions": [{"type": "open_app", "app": "Files"}]}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (draft, mut drafts) = tokio::sync::mpsc::unbounded_channel();
        let out = Out {
            activity: &activity,
            draft: &draft,
            cards: &unbounded_cards(),
        };
        let (desk, _handled) = desk();
        think(&mut brain, "x", &out, &desk).await.unwrap();
        assert_eq!(drafts.try_recv().unwrap(), "");
    }

    #[tokio::test]
    async fn a_desk_that_never_answers_counts_as_not_done() {
        let mut brain = Scripted::new([
            json!({"say": "", "done": false, "actions": [{"type": "open_app", "app": "Files"}]}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, acts) = tokio::sync::mpsc::unbounded_channel::<Act>();
        drop(acts);
        let answer = think(&mut brain, "x", &out!(activity), &desk)
            .await
            .unwrap();
        assert_eq!(answer.say, "ok");
        assert!(brain.prompts[1].contains("may not have run"));
    }

    #[tokio::test]
    async fn stops_after_its_steps_run_out() {
        let step = json!({"say": "", "done": false, "actions": [{"type": "open_app", "app": "X"}]});
        let mut brain = Scripted::new(std::iter::repeat_n(step, MAX_STEPS + 5));
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, handled) = desk();
        let answer = think(&mut brain, "x", &out!(activity), &desk)
            .await
            .unwrap();
        drop(desk);
        // Then asked once more to say where it got to, and nothing more runs.
        assert_eq!(brain.prompts.len(), MAX_STEPS + 2);
        assert_eq!(brain.prompts[MAX_STEPS + 1], OUT_OF_STEPS);
        assert_eq!(handled.await.unwrap(), MAX_STEPS);
        assert!(answer.done && answer.actions.is_empty());
    }

    #[tokio::test]
    async fn an_answer_after_the_last_step_offers_no_command() {
        let step = json!({"say": "", "done": false, "actions": [{"type": "open_app", "app": "X"}]});
        let last = json!({"say": "Installing", "actions": [
            {"type": "run", "command": "rm -rf ~", "why": "Clean", "spawn": ["systemd-run", "rm"]},
            {"type": "volume", "level": 20, "spawn": ["x"], "stop": ["y"]}]});
        let answers = std::iter::repeat_n(step, MAX_STEPS).chain([last]);
        let mut brain = Scripted::new(answers);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, _handled) = desk();
        let answer = think(&mut brain, "x", &out!(activity), &desk)
            .await
            .unwrap();
        assert_eq!(answer.actions, [json!({"type": "volume", "level": 20})]);
    }

    #[tokio::test]
    async fn the_answers_actions_run_and_are_checked() {
        let mut brain = Scripted::new([
            json!({"say": "Volume up", "actions": [{"type": "volume", "level": 50}]}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, handled) = desk();
        let answer = think(&mut brain, "x", &out!(activity), &desk)
            .await
            .unwrap();
        drop(desk);
        assert_eq!(handled.await.unwrap(), 1);
        assert_eq!(brain.prompts.len(), 1);
        assert!(answer.checked && answer.actions.len() == 1);
    }

    #[tokio::test]
    async fn a_failed_answer_goes_back_to_the_ai() {
        let mut brain = Scripted::new([
            json!({"say": "Playing", "actions": [{"type": "media", "command": "play"}]}),
            json!({"say": "Nothing is playing to resume.", "actions": []}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, handled) = desk_where(|action| action["type"] != "media");
        let answer = think(&mut brain, "play", &out!(activity), &desk)
            .await
            .unwrap();
        drop(desk);
        assert_eq!(handled.await.unwrap(), 1);
        assert!(
            brain.prompts[1].contains("- media failed"),
            "{}",
            brain.prompts[1]
        );
        assert_eq!(answer.say, "Nothing is playing to resume.");
    }

    #[tokio::test]
    async fn actions_after_the_island_closes_are_left_for_the_reply() {
        let mut brain = Scripted::new([json!({"say": "Typed it", "actions": [
            {"type": "write", "text": "hello"}, {"type": "volume", "level": 20}]})]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, handled) = desk();
        let answer = think(&mut brain, "x", &out!(activity), &desk)
            .await
            .unwrap();
        drop(desk);
        // Only the volume ran; both stay in the answer, and the reply keeps the write (see mod.rs).
        assert_eq!(handled.await.unwrap(), 1);
        assert!(answer.checked && answer.actions.len() == 2);
        assert!(after_close(&answer.actions[0]) && !after_close(&answer.actions[1]));
    }

    /// A desk that answers a command's card as the user would (`run`: Run, else Deny), and runs it as
    /// systemd would: its log ends with the exit line.
    fn desk_answering(run: bool, exit: i32) -> (Desk, tokio::task::JoinHandle<Vec<Value>>) {
        let (desk, mut acts) = tokio::sync::mpsc::unbounded_channel::<Act>();
        let handled = tokio::spawn(async move {
            let mut seen = Vec::new();
            while let Some(act) = acts.recv().await {
                let results = act
                    .actions
                    .iter()
                    .map(|a| {
                        seen.push(a.clone());
                        if a["type"] != "run" {
                            return json!({"type": a["type"], "ok": true});
                        }
                        if !run {
                            return json!({"type": "run", "ok": false, "detail": "denied"});
                        }
                        let log = a["spawn"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find_map(|arg| {
                                arg.as_str()
                                    .unwrap()
                                    .strip_prefix("--property=StandardOutput=append:")
                                    .map(str::to_owned)
                            })
                            // As root, the log is the last argument (see tasks.rs ROOT_WRAPPER).
                            .or_else(|| a["spawn"].as_array()?.last()?.as_str().map(str::to_owned));
                        std::fs::write(
                            log.unwrap(),
                            format!("Setting up vlc ...\n[keenu-exit {exit}]\n"),
                        )
                        .unwrap();
                        json!({"type": "run", "ok": true, "detail": "started"})
                    })
                    .collect();
                act.results.send(results).ok();
            }
            seen
        });
        (desk, handled)
    }

    #[tokio::test]
    async fn a_command_runs_once_the_user_says_so_and_its_end_goes_back() {
        let mut brain = Scripted::new([
            json!({"say": "", "done": false, "actions": [{"type": "run", "command": "sudo apt install -y vlc",
                "why": "Install VLC", "detail": "From Ubuntu's archive"}]}),
            json!({"say": "VLC is installed.", "actions": []}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (cards, mut shown) = tokio::sync::mpsc::unbounded_channel();
        let out = Out {
            activity: &activity,
            draft: &activity,
            cards: &cards,
        };
        let (desk, handled) = desk_answering(true, 0);
        let answer = think(&mut brain, "install vlc", &out, &desk).await.unwrap();
        drop(desk);
        let offered = &handled.await.unwrap()[0];
        // Offered as root, sudo taken off and shown back, with what it may do.
        assert_eq!(offered["command"], "sudo apt install -y vlc");
        assert_eq!(offered["risks"], json!(["root"]));
        assert!(
            !offered["spawn"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a == "--user")
        );
        assert!(
            brain.prompts[1].contains("finished (exit 0)")
                && brain.prompts[1].contains("Setting up vlc"),
            "{}",
            brain.prompts[1]
        );
        assert_eq!(
            answer.ran,
            [("sudo apt install -y vlc".to_owned(), "done".to_owned())]
        );
        let mut statuses = Vec::new();
        while let Ok(card) = shown.try_recv() {
            statuses.push(card["status"].as_str().unwrap().to_owned());
        }
        assert_eq!(statuses.first().unwrap(), "waiting");
        assert_eq!(statuses.last().unwrap(), "done");
    }

    #[tokio::test]
    async fn a_denied_command_never_runs() {
        let mut brain = Scripted::new([
            json!({"say": "", "done": false, "actions": [{"type": "run", "command": "flatpak install -y vlc", "why": "Install VLC"}]}),
            json!({"say": "Okay, I won't.", "actions": []}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (cards, _shown) = tokio::sync::mpsc::unbounded_channel();
        let out = Out {
            activity: &activity,
            draft: &activity,
            cards: &cards,
        };
        let (desk, _handled) = desk_answering(false, 0);
        let answer = think(&mut brain, "install vlc", &out, &desk).await.unwrap();
        assert!(
            brain.prompts[1].contains("the user denied it"),
            "{}",
            brain.prompts[1]
        );
        assert_eq!(
            answer.ran,
            [("flatpak install -y vlc".to_owned(), "denied".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_wrecking_command_is_never_offered() {
        let mut brain = Scripted::new([
            json!({"say": "", "done": false, "actions": [{"type": "run", "command": "rm -rf ~", "why": "Clean up"}]}),
            json!({"say": "I won't delete your home folder.", "actions": []}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (cards, _shown) = tokio::sync::mpsc::unbounded_channel();
        let out = Out {
            activity: &activity,
            draft: &activity,
            cards: &cards,
        };
        let (desk, handled) = desk_answering(true, 0);
        think(&mut brain, "clean my home", &out, &desk)
            .await
            .unwrap();
        drop(desk);
        assert!(handled.await.unwrap().is_empty(), "the user was asked");
        assert!(brain.prompts[1].contains("refused by Keenu"));
    }

    #[tokio::test]
    async fn checks_run_at_once_and_only_look() {
        let mut brain = Scripted::new([
            json!({"say": "", "done": false, "actions": [{"type": "check", "command": "uname -s"},
                {"type": "check", "command": "apt install x"}]}),
            json!({"say": "Linux.", "actions": []}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, handled) = desk();
        think(&mut brain, "which os", &out!(activity), &desk)
            .await
            .unwrap();
        drop(desk);
        assert_eq!(handled.await.unwrap(), 0);
        assert!(
            brain.prompts[1].contains("(exit 0):\nLinux")
                && brain.prompts[1].contains("did not run")
        );
    }

    #[tokio::test]
    async fn an_answer_that_will_not_look_is_told_it_can() {
        let mut brain = Scripted::new([
            json!({"say": "Bluetooth checks aren’t available to me right now.", "actions": []}),
            json!({"say": "", "done": false, "actions": [{"type": "check", "command": "uname -s"}]}),
            json!({"say": "Linux.", "actions": []}),
        ]);
        let (activity, _steps) = tokio::sync::mpsc::unbounded_channel();
        let (desk, _handled) = desk();
        let answer = think(
            &mut brain,
            "why is my bluetooth not working?",
            &out!(activity),
            &desk,
        )
        .await
        .unwrap();
        assert_eq!(brain.prompts[1], NUDGE);
        assert_eq!(answer.say, "Linux.");
        assert_eq!(answer.checks, ["uname -s"]);
        assert!(excuse("Aku belum bisa mengecek Bluetooth dari sini."));
        assert!(excuse("I couldn’t locate the file-search action just now."));
        assert!(!excuse("Bluetooth is on and paired with your headphones."));
        assert!(!excuse(
            "Kamu bisa melihat dan mengecek perangkatnya di Settings."
        ));
    }

    #[test]
    fn slow_actions_get_longer() {
        let actions = [
            json!({"type": "open_app"}),
            json!({"type": "play_music"}),
            json!({"type": "volume"}),
        ];
        assert_eq!(act_timeout(&actions), Duration::from_secs(20 + 6 + 13));
    }

    #[test]
    fn refuses_secret_folders() {
        let result = run_now(
            &json!({"type": "find_files", "terms": ["id"], "in": "~/.ssh"}),
            Path::new("/home/ana"),
        );
        assert!(result.contains("never searched"), "{result}");
    }
}
