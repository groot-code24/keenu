// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! AI agent alerts: Claude Code and Codex, once the user turns them on, run `keenud --agent-hook` when
//! they need the user or finish, and the island shows it. The hooks live in each AI's own config.

mod claude;
mod codex;
pub mod hook;
use keenu_integrations::agents::json_hooks;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use futures_util::StreamExt;
use inotify::{EventMask, Inotify, WatchMask};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio::time::{Instant, sleep_until};
use zbus::names::UniqueName;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::{fdo, interface};

use crate::assistant::Provider;

/// Longest texts the island gets, in characters.
const MAX_MESSAGE: usize = 160;
const MAX_PROJECT: usize = 60;
const MAX_SESSION: usize = 100;
const MAX_CWD: usize = 1024;
/// How many of the hook's parent processes come along: enough for a terminal or editor and their shells.
const MAX_PIDS: usize = 32;
/// Events of one kind from one AI session closer together than this are dropped.
const MIN_GAP: Duration = Duration::from_millis(300);
/// A permission waits this long for an answer on the island; then the AI asks in its own terminal
/// (its hook may wait ten minutes: this ends first).
const ANSWER_WITHIN: Duration = Duration::from_secs(9 * 60 + 30);
/// Asks waiting at once, at most; more ask in their own terminals (a flood is never shown).
const MAX_WAITING: usize = 8;
/// While a permission waits, how often keenud checks the hook asking is still there (the user may have
/// answered in the terminal, and the AI let its hook go).
const STILL_ASKING: Duration = Duration::from_secs(2);
const MAX_INPUT: usize = 300;
const MAX_TOOL: usize = 60;
/// A config file is looked at again once it has been quiet this long.
const SETTLE: Duration = Duration::from_millis(250);

const KINDS: [&str; 3] = ["permission", "waiting", "done"];

/// The AIs whose alerts Keenu can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ai {
    Claude,
    Codex,
}

impl Ai {
    const ALL: [Ai; 2] = [Ai::Claude, Ai::Codex];

    /// As Settings and the hooks name it.
    fn id(self) -> &'static str {
        match self {
            Ai::Claude => "claude",
            Ai::Codex => "codex",
        }
    }

    /// Settings' or a hook's name back to the AI; `None` for one Keenu has no hooks for.
    fn from_id(id: &str) -> Option<Ai> {
        Self::ALL.into_iter().find(|ai| ai.id() == id)
    }

    /// What the island calls it.
    fn name(self) -> &'static str {
        match self {
            Ai::Claude => "Claude Code",
            Ai::Codex => "Codex",
        }
    }

    /// The config file its hooks go in.
    fn config(self) -> PathBuf {
        match self {
            Ai::Claude => claude::settings_path(),
            Ai::Codex => codex::config_path(),
        }
    }

    /// Where Keenu's hooks are: Codex's hooks.json, or its config.toml's `notify` for an older Codex.
    fn hooks_file(self) -> PathBuf {
        match self {
            Ai::Codex if json_hooks::hooked(&codex::hooks_path(), &codex::SPEC) => {
                codex::hooks_path()
            }
            _ => self.config(),
        }
    }

    /// Whether its program is installed.
    fn found(self) -> bool {
        match self {
            Ai::Claude => Provider::Claude.program().is_some(),
            Ai::Codex => Provider::Codex.program().is_some(),
        }
    }

    /// Whether Keenu's hook is in its config now.
    fn hooked(self) -> bool {
        match self {
            Ai::Claude => claude::hooked(&self.config()),
            Ai::Codex => codex::hooked(&self.config()),
        }
    }

    /// Puts Keenu's hook in its config, pointing at this keenud.
    fn hook(self) -> anyhow::Result<()> {
        let keenud = keenud_path()?;
        let keenud = keenud
            .to_str()
            .with_context(|| format!("keenud's path {} is not UTF-8", keenud.display()))?;
        match self {
            Ai::Claude => claude::install(&self.config(), &claude::command(keenud, "claude")),
            Ai::Codex => codex::install(&self.config(), keenud, &claude::command(keenud, "codex")),
        }
    }

    /// Whether there was a hook to take out.
    fn unhook(self) -> anyhow::Result<bool> {
        match self {
            Ai::Claude => claude::remove(&self.config()),
            Ai::Codex => codex::remove(&self.config()),
        }
    }
}

/// This keenud, as hooks run it. Replaced while running (an update), the kernel adds " (deleted)".
fn keenud_path() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("could not find keenud's own path")?;
    let exe = exe
        .to_str()
        .and_then(|path| path.strip_suffix(" (deleted)"))
        .map_or(exe.clone(), PathBuf::from);
    Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// One AI as Settings shows it: installed, and hooked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
struct Status {
    found: bool,
    hooked: bool,
}

/// Both AIs' status, for the `Agents` property.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
struct Agents {
    claude: Status,
    codex: Status,
}

impl Agents {
    /// Read from disk each time, so a hook added or removed by hand shows.
    fn read() -> Self {
        let status = |ai: Ai| Status {
            found: ai.found(),
            hooked: ai.hooked(),
        };
        Self {
            claude: status(Ai::Claude),
            codex: status(Ai::Codex),
        }
    }
}

/// What an AI hook reports, and the island shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    pub provider: String,
    pub kind: String,
    pub project: String,
    pub cwd: String,
    pub message: String,
    pub session: String,
    /// The hook's parent processes, nearest first: the agent's terminal or editor is among them, so the
    /// island can tell its window from others that merely name the project.
    pub pids: Vec<u32>,
}

impl Event {
    /// Checked, on one line each and cut to size: the project is the folder's name unless given.
    pub fn checked(mut self) -> Result<Self, String> {
        // Grok has no hooks of its own: it runs Claude's, and the hook says so (see hook.rs `ai_of`).
        if Ai::from_id(&self.provider).is_none() && self.provider != "grok" {
            return Err(format!("unknown provider `{}`", cut(&self.provider, 40)));
        }
        if !KINDS.contains(&self.kind.as_str()) {
            return Err(format!("unknown kind `{}`", cut(&self.kind, 40)));
        }
        let cwd = self.cwd.trim();
        self.cwd = if cwd.starts_with('/')
            && cwd.chars().count() <= MAX_CWD
            && !cwd.contains(char::is_control)
        {
            cwd.to_owned()
        } else {
            String::new()
        };
        if self.project.trim().is_empty() {
            self.project = Path::new(&self.cwd)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        self.project = cut(&self.project, MAX_PROJECT);
        self.message = cut(&self.message, MAX_MESSAGE);
        self.session = cut(&self.session, MAX_SESSION);
        self.pids.retain(|&pid| pid > 1);
        self.pids.truncate(MAX_PIDS);
        Ok(self)
    }
}

/// Why `ai`'s alerts couldn't be turned on or off, in a line for the island, from the `error` logged.
fn refusal(ai: Ai, error: &str) -> String {
    let name = ai.name();
    let said = |words: &[&str]| words.iter().any(|word| error.contains(word));
    if said(&["already sends its notifications"]) {
        format!("{name} already sends its alerts to another app.")
    } else if said(&[
        "not valid JSON",
        "not valid TOML",
        "not a JSON object",
        "not a list",
    ]) {
        format!("{name}’s settings file has a mistake. Fix it, then turn this on again.")
    } else {
        format!("Keenu couldn’t change {name}’s settings. Try again.")
    }
}

/// `text` on one line, its spaces collapsed, at most `max` characters (an ellipsis ends a cut one).
fn cut(text: &str, max: usize) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect())
        .filter(|word: &String| !word.is_empty())
        .collect();
    clip(&words.join(" "), max)
}

/// `text` as it is, at most `max` characters (an ellipsis ends a cut one).
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut short: String = text.chars().take(max.saturating_sub(1)).collect();
    short.truncate(short.trim_end().len());
    short.push('…');
    short
}

/// A permission an AI agent asks for, as its hook sends it and the island shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub id: String,
    pub provider: String,
    pub project: String,
    pub cwd: String,
    pub session: String,
    /// The tool (Bash, Edit…) and what it would run or touch, on one line.
    pub tool: String,
    pub input: String,
    /// Whether the AI offers to keep it for next time (Claude's "always").
    pub always: bool,
    /// The hook's parent processes (see `Event::pids`).
    pub pids: Vec<u32>,
}

impl Request {
    /// Checked like an `Event`, with the tool cut to size; the input stays exactly as the AI would run it.
    fn checked(mut self) -> Result<Self, String> {
        // A hook sends only what shows whole (see hook.rs `request`): anything else asks in the terminal.
        if !crate::assistant::shows_whole(&self.input, usize::MAX) {
            return Err("the input has line breaks or hidden characters".into());
        }
        let event = Event {
            provider: self.provider.clone(),
            kind: "permission".into(),
            project: self.project.clone(),
            cwd: self.cwd.clone(),
            message: String::new(),
            session: self.session.clone(),
            pids: std::mem::take(&mut self.pids),
        }
        .checked()?;
        self.project = event.project;
        self.cwd = event.cwd;
        self.session = event.session;
        self.pids = event.pids;
        self.tool = cut(&self.tool, MAX_TOOL);
        self.input = clip(&self.input, MAX_INPUT);
        Ok(self)
    }
}

/// Drops an event that follows the last of its kind from the same AI session too closely.
#[derive(Default)]
struct Limiter {
    last: HashMap<(String, String, String), Instant>,
}

impl Limiter {
    /// False for an event too soon after the last of its kind.
    fn allow(&mut self, event: &Event, now: Instant) -> bool {
        let key = (
            event.provider.clone(),
            event.kind.clone(),
            event.session.clone(),
        );
        if self
            .last
            .get(&key)
            .is_some_and(|at| now.duration_since(*at) < MIN_GAP)
        {
            return false;
        }
        // Only the recent are kept: each AI session adds its own, which over weeks would pile up.
        self.last.retain(|_, at| now.duration_since(*at) < MIN_GAP);
        self.last.insert(key, now);
        true
    }
}

/// Whether GNOME's screen is locked (its ScreenSaver says so within a second); unknown counts as not.
async fn screen_locked(bus: &zbus::Connection) -> bool {
    let call = bus.call_method(
        Some("org.gnome.ScreenSaver"),
        "/org/gnome/ScreenSaver",
        Some("org.gnome.ScreenSaver"),
        "GetActive",
        &(),
    );
    match tokio::time::timeout(Duration::from_secs(1), call).await {
        Ok(Ok(reply)) => reply.body().deserialize::<bool>().unwrap_or(false),
        _ => false,
    }
}

/// Whether the island is there to answer asks, as the extension last said (see `SetPresent`).
#[derive(Debug, Clone, Default, PartialEq)]
enum Island {
    /// Never told (an older extension never says): taken as there, as before.
    #[default]
    Unknown,
    /// There, in GNOME Shell on this bus name: the name leaving the bus takes the island with it.
    There(UniqueName<'static>),
    Gone,
}

impl Island {
    /// GNOME Shell's bus name, while it has the island.
    fn shell(&self) -> Option<UniqueName<'static>> {
        match self {
            Island::There(shell) => Some(shell.clone()),
            _ => None,
        }
    }
}

/// Permissions waiting for the user, by id, and where each answer goes; and whether anyone can answer.
#[derive(Default)]
struct Asks {
    waiting: HashMap<String, oneshot::Sender<String>>,
    island: Island,
}

impl Asks {
    /// The island came, in GNOME Shell on bus name `shell`, or went (`None`): then every ask waiting is
    /// let go, and the AIs ask in their terminals.
    fn island(&mut self, shell: Option<UniqueName<'static>>) {
        self.island = match shell {
            Some(shell) => Island::There(shell),
            None => {
                for (_, answer) in self.waiting.drain() {
                    answer.send(String::new()).ok();
                }
                Island::Gone
            }
        };
    }

    /// GNOME Shell's `shell` left the bus: the island it had is gone, not one a newer shell has.
    fn left(&mut self, shell: &UniqueName<'_>) {
        if matches!(&self.island, Island::There(there) if there == shell) {
            self.island(None);
        }
    }

    /// Takes ask `id` to wait for `answer`; false with no island there, or too many waiting already.
    fn wait(&mut self, id: String, answer: oneshot::Sender<String>) -> bool {
        if self.island == Island::Gone || self.waiting.len() >= MAX_WAITING {
            return false;
        }
        self.waiting.insert(id, answer);
        true
    }
}

/// Agent alerts and permission asks, between the hooks and the island.
#[derive(Default)]
pub struct AgentsService {
    limiter: std::sync::Mutex<Limiter>,
    asks: std::sync::Mutex<Asks>,
    asked: std::sync::atomic::AtomicU64,
}

impl AgentsService {
    /// The asks waiting, and the island.
    fn asks(&self) -> std::sync::MutexGuard<'_, Asks> {
        self.asks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[interface(name = "io.github.zickrian.isle.Agents1")]
impl AgentsService {
    /// Puts Keenu's hook in `provider`'s config, or takes it out. GNOME Shell's to ask, no other program's.
    async fn set_hooked(
        &self,
        provider: &str,
        on: bool,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        crate::caller::shell_only(connection, &header).await?;
        let ai = Ai::from_id(provider)
            .ok_or_else(|| fdo::Error::InvalidArgs(format!("no AI called `{provider}`")))?;
        let done =
            tokio::task::spawn_blocking(move || if on { ai.hook() } else { ai.unhook().map(drop) })
                .await
                .map_err(|err| fdo::Error::Failed(err.to_string()))?;
        if let Err(err) = self.agents_changed(&emitter).await {
            warn!("could not publish the AI alerts: {err}");
        }
        // The details go to the log; the island says it in a line anyone reads.
        done.map_err(|err| {
            warn!("could not change {}'s hooks: {err:#}", ai.name());
            fdo::Error::Failed(refusal(ai, &format!("{err:#}")))
        })
    }

    /// From `keenud --agent-hook`: JSON `{provider, kind, project, cwd, message, session}`.
    async fn report(
        &self,
        json: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let event: Event =
            serde_json::from_str(json).map_err(|err| fdo::Error::InvalidArgs(err.to_string()))?;
        let event = event.checked().map_err(fdo::Error::InvalidArgs)?;
        let allowed = self
            .limiter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .allow(&event, Instant::now());
        if !allowed {
            debug!(
                "dropped a {} {} event: too soon",
                event.provider, event.kind
            );
            return Ok(());
        }
        let json = serde_json::to_string(&event).unwrap_or_default();
        Ok(Self::event(&emitter, &json).await?)
    }

    /// From `keenud --agent-hook`: a `Request` as JSON. Waits for the user's "allow", "always" or "deny";
    /// "" when the island will not answer, and the AI then asks in its own terminal.
    async fn decide(
        &self,
        json: &str,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<String> {
        let request: Request =
            serde_json::from_str(json).map_err(|err| fdo::Error::InvalidArgs(err.to_string()))?;
        let mut request = request.checked().map_err(fdo::Error::InvalidArgs)?;
        let number = self
            .asked
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        request.id = format!("{}-{number}", request.provider);
        // No island to show it (Keenu off, the screen locked): the AI asks in its terminal at once.
        let island = self.asks().island.clone();
        if island == Island::Gone {
            return Ok(String::new());
        }
        // Never told (keenud restarted meanwhile): the screen locked has no island either.
        if island == Island::Unknown && screen_locked(connection).await {
            return Ok(String::new());
        }
        let dbus = zbus::fdo::DBusProxy::new(connection).await.ok();
        // GNOME Shell gone from the bus (it stopped or restarted) took the island with it.
        if let Some(shell) = island.shell()
            && !on_bus(dbus.as_ref(), &shell).await
        {
            self.asks().left(&shell);
        }
        let (answer, answered) = oneshot::channel();
        let shell = {
            let mut asks = self.asks();
            if !asks.wait(request.id.clone(), answer) {
                return Ok(String::new());
            }
            asks.island.shell()
        };
        let shown = serde_json::to_string(&request).unwrap_or_default();
        Self::asking(&emitter, &shown).await?;
        let asker = header.sender().map(|name| name.to_owned());
        let watched: Vec<UniqueName<'static>> = asker.into_iter().chain(shell).collect();
        let decision = wait_for_answer(dbus.as_ref(), &watched, answered).await;
        // Over, one way or another: the island lets it go.
        let still = self.asks().waiting.remove(&request.id);
        if still.is_some() {
            Self::withdrawn(&emitter, &request.id).await.ok();
        }
        Ok(decision)
    }

    /// Whether the island is there to answer asks: the extension says so as it starts, and not as it goes
    /// (Keenu turned off, the screen locked). GNOME Shell's only.
    async fn set_present(
        &self,
        present: bool,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<()> {
        crate::caller::shell_only(connection, &header).await?;
        let shell = header
            .sender()
            .filter(|_| present)
            .map(|name| name.to_owned());
        self.asks().island(shell);
        Ok(())
    }

    /// The island's answer to permission `id`: "allow", "always" or "deny" ("" lets the AI ask itself).
    async fn resolve(
        &self,
        id: &str,
        decision: &str,
        #[zbus(connection)] connection: &zbus::Connection,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> fdo::Result<()> {
        crate::caller::shell_only(connection, &header).await?;
        if !["allow", "always", "deny", ""].contains(&decision) {
            return Err(fdo::Error::InvalidArgs(format!(
                "no such answer `{}`",
                cut(decision, 20)
            )));
        }
        let answer = self.asks().waiting.remove(id);
        if let Some(answer) = answer {
            answer.send(decision.to_owned()).ok();
        }
        Ok(())
    }

    /// An AI asks a permission: the JSON `Decide` got, with its `id` for `Resolve`.
    #[zbus(signal)]
    async fn asking(emitter: &SignalEmitter<'_>, json: &str) -> zbus::Result<()>;

    /// Permission `id` needs no answer any more (answered in the terminal, or it waited too long).
    #[zbus(signal)]
    async fn withdrawn(emitter: &SignalEmitter<'_>, id: &str) -> zbus::Result<()>;

    /// JSON `{claude: {found, hooked}, codex: {found, hooked}}`, read afresh.
    #[zbus(property)]
    fn agents(&self) -> String {
        serde_json::to_string(&Agents::read()).unwrap_or_default()
    }

    /// An AI needs the user or finished: the checked event as JSON.
    #[zbus(signal)]
    async fn event(emitter: &SignalEmitter<'_>, json: &str) -> zbus::Result<()>;
}

/// The user's answer, or "" if none comes in time or one of `watched` leaves the bus: the hook asking (the
/// AI moved on), or GNOME Shell with the island.
async fn wait_for_answer(
    dbus: Option<&fdo::DBusProxy<'_>>,
    watched: &[UniqueName<'static>],
    mut answered: oneshot::Receiver<String>,
) -> String {
    let deadline = Instant::now() + ANSWER_WITHIN;
    loop {
        tokio::select! {
            answer = &mut answered => return answer.unwrap_or_default(),
            () = tokio::time::sleep(STILL_ASKING) => {
                if Instant::now() >= deadline {
                    return String::new();
                }
                for name in watched {
                    if !on_bus(dbus, name).await {
                        return String::new();
                    }
                }
            }
        }
    }
}

/// Whether `name` is still on the bus; true when that can't be told.
async fn on_bus(dbus: Option<&fdo::DBusProxy<'_>>, name: &UniqueName<'_>) -> bool {
    match dbus {
        Some(dbus) => dbus
            .name_has_owner(name.as_ref().into())
            .await
            .unwrap_or(true),
        None => true,
    }
}

/// Publishes `Agents` again whenever an AI's config file changes (the user, or the AI, edited it).
pub async fn watch(iface: InterfaceRef<AgentsService>) {
    let mut published = Agents::read();
    loop {
        if let Err(err) = watch_configs(&iface, &mut published).await {
            debug!("stopped watching the AIs' configs: {err}");
            return;
        }
    }
}

/// Returns when a config folder came or went, to watch afresh.
async fn watch_configs(
    iface: &InterfaceRef<AgentsService>,
    published: &mut Agents,
) -> anyhow::Result<()> {
    let inotify = Inotify::init()?;
    let mut watches = inotify.watches();
    let files: Vec<PathBuf> = Ai::ALL
        .into_iter()
        .map(Ai::config)
        .chain([codex::hooks_path()])
        .collect();
    // Also where their programs land when installed (npm, nvm, volta, bun…), so a new one is found at once.
    // A folder not there yet is watched through its nearest parent until it appears.
    let home = dirs::home_dir().unwrap_or_default();
    let mut ahead: Vec<(inotify::WatchDescriptor, std::ffi::OsString)> = Vec::new();
    let mut watch_or_ahead = |dir: &Path, mask: WatchMask| {
        if watches.add(dir, mask | WatchMask::MASK_ADD).is_ok() {
            return;
        }
        let mut below = dir;
        while let Some(parent) = below.parent() {
            if parent.is_dir() {
                let appear = WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::MASK_ADD;
                if let (Ok(wd), Some(name)) = (watches.add(parent, appear), below.file_name()) {
                    ahead.push((wd, name.to_owned()));
                }
                break;
            }
            below = parent;
        }
    };
    for dir in crate::assistant::install_dirs(&home)
        .into_iter()
        .chain([PathBuf::from("/usr/bin")])
        .filter(|dir| dir.starts_with(&home) || dir.is_dir())
    {
        watch_or_ahead(
            &dir,
            WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::DELETE,
        );
    }
    let nvm_nodes = std::env::var_os("NVM_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| home.join(".nvm"), PathBuf::from)
        .join("versions/node");
    watch_or_ahead(&nvm_nodes, WatchMask::CREATE | WatchMask::MOVED_TO);
    let nodes = watches
        .add(
            &nvm_nodes,
            WatchMask::CREATE | WatchMask::MOVED_TO | WatchMask::MASK_ADD,
        )
        .ok();
    let mut watched = false;
    for file in &files {
        let Some(dir) = file.parent() else { continue };
        let changes = WatchMask::CLOSE_WRITE
            | WatchMask::MOVED_TO
            | WatchMask::MOVED_FROM
            | WatchMask::CREATE
            | WatchMask::DELETE
            | WatchMask::DELETE_SELF
            | WatchMask::MOVE_SELF;
        if watches.add(dir, changes).is_ok() {
            watched = true;
        } else if let Some(parent) = dir.parent()
            && watches
                .add(parent, WatchMask::CREATE | WatchMask::MOVED_TO)
                .is_ok()
        {
            // The folder appearing (the AI's first run).
            watched = true;
        }
    }
    if !watched {
        std::future::pending::<()>().await;
    }
    let programs = [
        std::ffi::OsStr::new("claude"),
        std::ffi::OsStr::new("codex"),
    ];
    let names: Vec<&std::ffi::OsStr> = files
        .iter()
        .flat_map(|file| [file.file_name(), file.parent().and_then(Path::file_name)])
        .flatten()
        .chain(programs)
        .collect();
    let mut events = inotify.into_event_stream([0u8; 1024])?;
    let mut due = None;
    let mut again = false;
    loop {
        tokio::select! {
            event = events.next() => {
                let event = event.context("inotify stopped")??;
                let gone = EventMask::DELETE_SELF | EventMask::MOVE_SELF | EventMask::IGNORED | EventMask::Q_OVERFLOW;
                let named = event.name.as_deref().is_some_and(|name| names.contains(&name));
                let new_node = nodes.as_ref() == Some(&event.wd);
                let appeared = ahead
                    .iter()
                    .any(|(wd, name)| *wd == event.wd && event.name.as_deref() == Some(name.as_os_str()));
                if event.mask.intersects(gone) || new_node || appeared || (named && event.mask.contains(EventMask::ISDIR)) {
                    again = true;
                } else if !named {
                    continue;
                }
                due = Some(Instant::now() + SETTLE);
            }
            () = sleep_until(due.unwrap_or_else(Instant::now)), if due.is_some() => {
                due = None;
                let now = Agents::read();
                if now != *published {
                    *published = now;
                    let service = iface.get().await;
                    if let Err(err) = service.agents_changed(iface.signal_emitter()).await {
                        warn!("could not publish the AI alerts: {err}");
                    }
                }
                if again {
                    return Ok(());
                }
            }
        }
    }
}

/// For `keenud --doctor`: whether each AI's alerts are on.
pub fn report() -> String {
    let lines: Vec<String> = Ai::ALL
        .into_iter()
        .map(|ai| {
            let state = match (ai.found(), ai.hooked()) {
                (_, true) => format!("on ({})", ai.hooks_file().display()),
                (true, false) => "off".to_owned(),
                (false, false) => "off (not installed)".to_owned(),
            };
            format!("  {}: {state}", ai.name())
        })
        .collect();
    lines.join("\n")
}

/// `keenud --unhook-all`: takes Keenu's hooks out of every AI's config, as Keenu is uninstalled.
pub fn unhook_all() {
    for ai in Ai::ALL {
        let name = ai.name();
        match ai.unhook() {
            Ok(true) => println!("{name}: alerts removed from {}", ai.config().display()),
            Ok(false) => println!("{name}: no alerts were set up"),
            Err(err) => println!("{name}: could not remove the alerts: {err:#}"),
        }
    }
}

/// A fresh, empty directory for one test.
#[cfg(test)]
pub fn test_scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("keenu-agents-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_reads_plainly() {
        let broken = "/home/u/.claude/settings.json is not valid JSON; fix it first: expected value at line 3";
        let said = refusal(Ai::Claude, broken);
        assert_eq!(
            said,
            "Claude Code’s settings file has a mistake. Fix it, then turn this on again."
        );
        let taken = refusal(
            Ai::Codex,
            "Codex already sends its notifications to another program",
        );
        assert_eq!(taken, "Codex already sends its alerts to another app.");
        let denied = refusal(
            Ai::Codex,
            "could not write /home/u/.codex/hooks.json: Permission denied",
        );
        assert!(!denied.contains('/') && denied.starts_with("Keenu couldn’t change Codex"));
        // The hooks file's own words for a file of the wrong shape.
        for broken in [
            serde_json::json!([]),
            serde_json::json!({"hooks": 1}),
            serde_json::json!({"hooks": {"Stop": 1}}),
        ] {
            let err = json_hooks::add_hooks(&mut broken.clone(), "keenud", &codex::SPEC)
                .context("in /home/u/.codex/hooks.json")
                .unwrap_err();
            let said = refusal(Ai::Codex, &format!("{err:#}"));
            assert!(said.contains("settings file has a mistake"), "{said}");
        }
    }

    #[test]
    fn a_command_asked_about_stays_as_it_runs() {
        let request = |input: &str| {
            Request {
                provider: "claude".into(),
                tool: "Bash".into(),
                input: input.into(),
                ..Request::default()
            }
            .checked()
        };
        assert_eq!(request("grep \"a  b\" f").unwrap().input, "grep \"a  b\" f");
        assert!(request("ls\nrm -rf ~").is_err());
        assert!(request("ls \u{202E}fdp.exe").is_err());
        let long = request(&"x ".repeat(MAX_INPUT)).unwrap().input;
        assert!(long.chars().count() <= MAX_INPUT && long.ends_with('…'));
    }

    #[test]
    fn asks_wait_only_while_the_island_is_there() {
        let shell = UniqueName::try_from(":1.12").unwrap();
        let mut asks = Asks::default();
        // Never told (an older extension): asks wait, as before.
        let (answer, mut first) = oneshot::channel();
        assert!(asks.wait("claude-0".into(), answer));
        asks.island(Some(shell.clone()));
        let (answer, mut second) = oneshot::channel();
        assert!(asks.wait("codex-1".into(), answer));
        // Another shell leaving the bus changes nothing; this one leaving lets every ask go at once.
        asks.left(&UniqueName::try_from(":1.40").unwrap());
        assert!(first.try_recv().is_err() && asks.waiting.len() == 2);
        asks.left(&shell);
        assert_eq!(first.try_recv().unwrap(), "");
        assert_eq!(second.try_recv().unwrap(), "");
        assert!(asks.waiting.is_empty());
        // Gone: a new ask doesn't wait, until the island is back.
        let (answer, _) = oneshot::channel();
        assert!(!asks.wait("codex-2".into(), answer));
        asks.island(Some(UniqueName::try_from(":1.77").unwrap()));
        let (answer, _) = oneshot::channel();
        assert!(asks.wait("codex-3".into(), answer));
        // Turned off: the same.
        asks.island(None);
        assert!(asks.waiting.is_empty() && asks.island == Island::Gone);
        // At most `MAX_WAITING` at once.
        asks.island(Some(shell));
        for number in 0..MAX_WAITING {
            assert!(asks.wait(format!("claude-{number}"), oneshot::channel().0));
        }
        assert!(!asks.wait("claude-x".into(), oneshot::channel().0));
    }

    fn event(json: &str) -> Result<Event, String> {
        serde_json::from_str::<Event>(json).unwrap().checked()
    }

    #[test]
    fn events_are_checked_and_cut_to_size() {
        let checked = event(
            r#"{"provider": "claude", "kind": "permission", "cwd": "/home/a/keenu",
                "message": "  Claude needs\n your permission\tto use Bash  ", "session": "abc"}"#,
        )
        .unwrap();
        assert_eq!(checked.project, "keenu");
        assert_eq!(checked.message, "Claude needs your permission to use Bash");
        assert_eq!(checked.session, "abc");

        let long = event(&format!(
            r#"{{"provider": "codex", "kind": "done", "cwd": "relative/path",
                 "project": "{}", "message": "{}\u0007"}}"#,
            "p".repeat(100),
            "word ".repeat(100)
        ))
        .unwrap();
        assert_eq!(long.cwd, "");
        assert_eq!(long.project.chars().count(), MAX_PROJECT);
        assert!(long.project.ends_with('…'));
        assert!(long.message.chars().count() <= MAX_MESSAGE);
        assert!(long.message.ends_with("word…"));

        assert!(event(r#"{"provider": "agy", "kind": "done"}"#).is_err());
        assert!(event(r#"{"provider": "claude", "kind": "exploded"}"#).is_err());
        assert!(event(r#"{"provider": "claude", "kind": "done"}"#).is_ok());
    }

    #[test]
    fn events_too_close_together_are_dropped() {
        let mut limiter = Limiter::default();
        let done = event(r#"{"provider": "claude", "kind": "done"}"#).unwrap();
        let waiting = event(r#"{"provider": "claude", "kind": "waiting"}"#).unwrap();
        let codex = event(r#"{"provider": "codex", "kind": "done"}"#).unwrap();
        let start = Instant::now();
        assert!(limiter.allow(&done, start));
        assert!(!limiter.allow(&done, start + Duration::from_millis(100)));
        assert!(limiter.allow(&waiting, start + Duration::from_millis(100)));
        assert!(limiter.allow(&codex, start + Duration::from_millis(100)));
        assert!(limiter.allow(&done, start + Duration::from_millis(400)));
        // Sessions long over are forgotten.
        assert_eq!(limiter.last.len(), 1);
    }

    #[test]
    fn agents_are_published_as_json() {
        let agents = Agents {
            claude: Status {
                found: true,
                hooked: false,
            },
            codex: Status::default(),
        };
        assert_eq!(
            serde_json::to_string(&agents).unwrap(),
            r#"{"claude":{"found":true,"hooked":false},"codex":{"found":false,"hooked":false}}"#
        );
    }
}
