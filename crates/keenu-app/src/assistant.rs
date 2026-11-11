// SPDX-License-Identifier: GPL-3.0-or-later

//! The assistant on hosts that draw the island themselves: one worker thread that keeps the conversation, runs a
//! request at a time through `keenu_assistant`, and tells the app how it is going.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keenu_assistant::agent::{self, Out};
use keenu_assistant::api::{ApiBrain, ApiRoute, Wire};
use keenu_assistant::brain::{Brain, Provider};
use keenu_assistant::chatgpt;
use keenu_assistant::connectors::{self, Access, Client, Connectors, Tool};
use keenu_assistant::desktop::{
    AppActions, Approver, Asking, DesktopTools, HostBrain, LiveSearch, serve_desk,
};
use keenu_assistant::instructions::{self, Tone};
use keenu_assistant::memory::Memory;
use keenu_guard::{ActionLog, Policy};
use keenu_platform::AppControlProvider;
use keenu_security::credentials::CredentialStore;
use tokio::sync::oneshot;

pub const CREDENTIAL_API: &str = "assistant/api-key";
pub const CREDENTIAL_XAI: &str = "assistant/xai-key";
/// The conversation goes, and any program behind it quits, this long after the last request.
const KEEP_WARM: Duration = Duration::from_secs(3 * 60);
/// A whole request, all its steps and approvals together.
const REQUEST_LIMIT: Duration = Duration::from_secs(45 * 60);

/// What a request reports back.
pub enum Progress {
    Step(String),
    Draft(String),
    Asked(Asking, oneshot::Sender<bool>),
    Done {
        say: String,
        listen: bool,
        unattended: bool,
    },
    Failed {
        why: String,
        unattended: bool,
    },
}

/// Which AI answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    ChatGpt,
    Cli(&'static str),
    Api {
        wire: WireName,
        base: String,
        model: String,
    },
}

/// `Wire`, for comparing routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireName {
    Chat,
    Anthropic,
    Responses,
}

impl Route {
    pub fn words(&self) -> String {
        match self {
            Self::ChatGpt => "ChatGPT".into(),
            Self::Cli(id) => {
                Provider::from_id(id).map_or_else(|| (*id).to_owned(), |p| p.name().to_owned())
            }
            Self::Api { model, .. } => format!("API: {model}"),
        }
    }
}

/// What a short name in Settings means as an address, and which API shape it has.
pub fn known_api(text: &str) -> (String, WireName, bool) {
    match text.trim().to_lowercase().as_str() {
        "anthropic" | "claude" => (
            "https://api.anthropic.com/v1".into(),
            WireName::Anthropic,
            true,
        ),
        "openai" | "chatgpt" => ("https://api.openai.com/v1".into(), WireName::Chat, true),
        "xai" | "grok" => ("https://api.x.ai/v1".into(), WireName::Chat, true),
        "ollama" | "local" => ("http://127.0.0.1:11434/v1".into(), WireName::Chat, false),
        _ => {
            let wire = if text.contains("anthropic.com") {
                WireName::Anthropic
            } else {
                WireName::Chat
            };
            (text.trim().to_owned(), wire, !is_local(text))
        }
    }
}

/// An address on this computer, which needs no key.
pub fn is_local(url: &str) -> bool {
    url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost")
}

/// Which AI to use, given what is there; or why none can be, and what to do about it.
pub fn resolve(
    wanted: &str,
    chatgpt_connected: bool,
    api: Option<(String, String)>,
    installed: &dyn Fn(&str) -> Option<&'static str>,
) -> Result<Route, String> {
    let api_route = || {
        api.as_ref().map(|(base, model)| Route::Api {
            wire: known_api(base).1,
            base: known_api(base).0,
            model: model.clone(),
        })
    };
    match wanted {
        "chatgpt" if chatgpt_connected => Ok(Route::ChatGpt),
        "chatgpt" => Err("ChatGPT is not connected: press Connect ChatGPT in Settings.".into()),
        "api" => api_route().ok_or_else(|| "No API is set up yet: press Set API in Settings.".to_owned()),
        "auto" | "" => {
            if chatgpt_connected {
                return Ok(Route::ChatGpt);
            }
            if let Some(id) = installed("auto") {
                return Ok(Route::Cli(id));
            }
            api_route().ok_or_else(|| {
                "No AI is connected yet. In Settings: connect ChatGPT, set an API, or install Claude Code, Codex or Grok and sign in to it.".to_owned()
            })
        }
        id => installed(id).map(Route::Cli).ok_or_else(|| {
            let name = Provider::from_id(id).map_or_else(|| id.to_owned(), |p| p.name().to_owned());
            format!("{name} is not installed. Install it and sign in to it with its own command, then try again.")
        }),
    }
}

/// What the system's own AI programs can do, as `resolve` asks.
pub fn installed_program(wanted: &str) -> Option<&'static str> {
    Provider::pick(wanted).map(Provider::id)
}

/// One request, and everything it needs that may have changed since the last.
pub struct Job {
    pub said: String,
    pub route: Route,
    pub tone: Tone,
    pub policy: Policy,
    pub project: String,
    pub project_notes: String,
    pub agenda: String,
    pub connectors: Connectors,
    pub unattended: bool,
    pub x_model: String,
}

/// The way to the worker.
#[derive(Clone)]
pub struct Assistant {
    jobs: Sender<Box<Job>>,
    stop: Arc<AtomicBool>,
}

impl Assistant {
    pub fn ask(&self, job: Job) {
        self.stop.store(false, Ordering::Relaxed);
        let _ = self.jobs.send(Box::new(job));
    }

    /// Ends what is running at its next step, and takes any program behind it along.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What stays the same for as long as Keenu runs.
pub struct Fixed {
    pub control: Arc<dyn AppControlProvider>,
    pub credentials: Arc<dyn CredentialStore>,
    pub memory: Arc<Mutex<Option<Arc<Mutex<Memory>>>>>,
    pub log: ActionLog,
    pub workspace: PathBuf,
    pub reports: PathBuf,
    pub system: &'static str,
    pub app_actions: AppActions,
    pub sink: Arc<dyn Fn(Progress) + Send + Sync>,
}

struct Warm {
    route: Route,
    instructions: String,
    brain: HostBrain,
}

/// Starts the worker.
pub fn start(fixed: Fixed) -> Assistant {
    let (jobs, inbox) = mpsc::channel::<Box<Job>>();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let _ = std::thread::Builder::new()
        .name("keenu-assistant".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                (fixed.sink)(Progress::Failed {
                    why: "The assistant could not start.".into(),
                    unattended: false,
                });
                return;
            };
            let mut warm: Option<Warm> = None;
            loop {
                let job = match inbox.recv_timeout(KEEP_WARM) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        warm = None;
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                let unattended = job.unattended;
                let result = runtime.block_on(run_job(&fixed, &mut warm, *job, &stopping));
                (fixed.sink)(match result {
                    Ok((say, listen)) => Progress::Done {
                        say,
                        listen,
                        unattended,
                    },
                    Err(why) => Progress::Failed { why, unattended },
                });
            }
        });
    Assistant { jobs, stop }
}

fn api_route(fixed: &Fixed, wire: WireName, base: &str, model: &str) -> ApiRoute {
    let store = fixed.credentials.clone();
    ApiRoute {
        wire: match wire {
            WireName::Chat => Wire::Chat,
            WireName::Anthropic => Wire::Anthropic,
            WireName::Responses => Wire::Responses,
        },
        base: base.to_owned(),
        model: model.to_owned(),
        token: Arc::new(move || {
            store
                .get(CREDENTIAL_API)
                .map(Option::unwrap_or_default)
                .map_err(|err| err.to_string())
        }),
    }
}

/// The AI for this job: the one already warm if nothing about it changed.
async fn brain_for(fixed: &Fixed, route: &Route, instructions: &str) -> Result<HostBrain, String> {
    match route {
        Route::ChatGpt => {
            let store = fixed.credentials.clone();
            let route = tokio::task::spawn_blocking(move || {
                chatgpt::route(store, chatgpt::Endpoints::default(), "")
            })
            .await
            .map_err(|err| err.to_string())??;
            Ok(HostBrain::Api(ApiBrain::new(route, instructions)))
        }
        Route::Cli(id) => {
            let provider = Provider::from_id(id).ok_or("That AI is not known here.")?;
            let brain = Brain::start(provider, "", None, &fixed.workspace, instructions)
                .await
                .map_err(|err| format!("{err:#}"))?;
            Ok(HostBrain::Cli(Box::new(brain)))
        }
        Route::Api { wire, base, model } => Ok(HostBrain::Api(ApiBrain::new(
            api_route(fixed, *wire, base, model),
            instructions,
        ))),
    }
}

/// What the connectors the user has turned on offer. One that cannot be reached is left out, and says so in the log.
fn connector_tools(fixed: &Fixed, connectors: &Connectors) -> BTreeMap<String, Vec<Tool>> {
    let mut tools = BTreeMap::new();
    for connector in connectors.list.iter().filter(|c| c.access != Access::Off) {
        let token = connector_token(&*fixed.credentials, &connector.name);
        match Client::connect(connector, &token)
            .and_then(|mut client| client.tools(&connector.name))
        {
            Ok(list) => {
                tools.insert(connector.name.clone(), list);
            }
            Err(why) => log::warn!("{}: {why}", connector.name),
        }
    }
    tools
}

pub fn connector_key(name: &str) -> String {
    format!("assistant/connector/{}", name.to_lowercase())
}

fn connector_token(store: &dyn CredentialStore, name: &str) -> String {
    store
        .get(&connector_key(name))
        .ok()
        .flatten()
        .unwrap_or_default()
}

/// Runs one request to its end.
async fn run_job(
    fixed: &Fixed,
    warm: &mut Option<Warm>,
    job: Job,
    stop: &Arc<AtomicBool>,
) -> Result<(String, bool), String> {
    let Job {
        said,
        route,
        tone,
        policy,
        project,
        project_notes,
        agenda,
        connectors,
        unattended,
        x_model,
    } = job;
    let connector_list = connector_tools(fixed, &connectors);
    let project_pair = (!project.is_empty()).then_some((project.as_str(), project_notes.as_str()));
    let job_text = instructions::job(
        fixed.system,
        tone,
        project_pair,
        &connectors::tools_for_prompt(&connector_list),
    );
    // A scheduled request stands alone: it neither reads nor adds to the user's conversation.
    let mut own = None;
    let brain = if unattended {
        own = Some(brain_for(fixed, &route, &job_text).await?);
        own.as_mut().ok_or("The AI did not start.")?
    } else {
        let same = warm
            .as_ref()
            .is_some_and(|w| w.route == route && w.instructions == job_text);
        if !same {
            *warm = None;
            let brain = brain_for(fixed, &route, &job_text).await?;
            *warm = Some(Warm {
                route: route.clone(),
                instructions: job_text.clone(),
                brain,
            });
        }
        &mut warm.as_mut().ok_or("The AI did not start.")?.brain
    };

    let memory = fixed.memory.lock().ok().and_then(|memory| memory.clone());
    let memory_text = memory
        .as_ref()
        .and_then(|memory| memory.lock().ok().map(|m| m.for_prompt(&said, &project)))
        .unwrap_or_default();
    let now = chrono::Local::now().format("%A %Y-%m-%d %H:%M").to_string();
    let wanted = match said.strip_prefix("research ") {
        Some(topic) => instructions::research_request(topic),
        None => said.clone(),
    };
    let prompt = instructions::request(&wanted, &memory_text, &agenda, &now);

    let sink = fixed.sink.clone();
    let approve: Approver = Arc::new(move |asking| {
        let (answer, answered) = oneshot::channel();
        sink(Progress::Asked(asking, answer));
        answered
    });
    let store = fixed.credentials.clone();
    let live_search = fixed
        .credentials
        .get(CREDENTIAL_XAI)
        .ok()
        .flatten()
        .filter(|key| !key.is_empty())
        .map(|key| LiveSearch {
            base: "https://api.x.ai/v1".into(),
            key,
            model: x_model,
        });
    let tools = Arc::new(DesktopTools {
        control: fixed.control.clone(),
        policy,
        log: fixed.log.clone(),
        home: dirs_home(),
        memory,
        project,
        connectors,
        connector_tools: connector_list,
        connector_token: Arc::new(move |name| connector_token(&*store, name)),
        approve,
        app_actions: fixed.app_actions.clone(),
        live_search,
        reports: fixed.reports.clone(),
        request: Mutex::new(said),
        unattended,
    });

    let (desk, handed) = tokio::sync::mpsc::unbounded_channel();
    let serving = tokio::spawn(serve_desk(tools.clone(), handed));
    let (activity, mut steps) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (draft, mut drafts) = tokio::sync::mpsc::unbounded_channel::<String>();
    let (cards, _no_cards) = tokio::sync::mpsc::unbounded_channel();
    let step_sink = fixed.sink.clone();
    let following_steps = tokio::spawn(async move {
        while let Some(step) = steps.recv().await {
            step_sink(Progress::Step(step));
        }
    });
    let draft_sink = fixed.sink.clone();
    let following_drafts = tokio::spawn(async move {
        while let Some(words) = drafts.recv().await {
            draft_sink(Progress::Draft(words));
        }
    });
    let out = Out {
        activity: &activity,
        draft: &draft,
        cards: &cards,
    };
    let stopped = async {
        while !stop.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    let result = tokio::select! {
        answer = tokio::time::timeout(REQUEST_LIMIT, agent::think(brain, &prompt, &out, &desk, &*tools)) => match answer {
            Ok(Ok(answer)) => Ok((answer.say, answer.listen)),
            Ok(Err(why)) => Err(format!("{why:#}")),
            Err(_) => Err("That took too long, so Keenu stopped it.".to_owned()),
        },
        () = stopped => {
            // What it started goes with it; the next request begins afresh.
            *warm = None;
            Ok(("Stopped.".to_owned(), false))
        }
    };
    drop(desk);
    drop((activity, draft));
    let _ = serving.await;
    let _ = following_steps.await;
    let _ = following_drafts.await;
    drop(own);
    result
}

/// The home folder, which the file tools stay within.
fn dirs_home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing(_: &str) -> Option<&'static str> {
        None
    }

    fn claude(wanted: &str) -> Option<&'static str> {
        matches!(wanted, "auto" | "claude").then_some("claude")
    }

    #[test]
    fn auto_takes_what_is_there_in_order() {
        assert_eq!(resolve("auto", true, None, &claude), Ok(Route::ChatGpt));
        assert_eq!(
            resolve("auto", false, None, &claude),
            Ok(Route::Cli("claude"))
        );
        let api = Some(("ollama".to_owned(), "llama".to_owned()));
        assert!(matches!(
            resolve("auto", false, api, &nothing),
            Ok(Route::Api { .. })
        ));
        assert!(
            resolve("auto", false, None, &nothing)
                .unwrap_err()
                .contains("No AI is connected")
        );
    }

    #[test]
    fn a_route_asked_for_is_never_swapped_for_another() {
        assert!(
            resolve("chatgpt", false, None, &claude)
                .unwrap_err()
                .contains("Connect ChatGPT")
        );
        assert!(
            resolve("api", true, None, &claude)
                .unwrap_err()
                .contains("Set API")
        );
        assert!(
            resolve("grok", true, None, &claude)
                .unwrap_err()
                .contains("Grok is not installed")
        );
        assert_eq!(
            resolve("claude", true, None, &claude),
            Ok(Route::Cli("claude"))
        );
    }

    #[test]
    fn short_names_mean_their_addresses() {
        assert_eq!(known_api("anthropic").1, WireName::Anthropic);
        let (base, wire, key) = known_api("ollama");
        assert!(base.starts_with("http://127.0.0.1") && wire == WireName::Chat && !key);
        assert!(known_api("xai").0.contains("api.x.ai"));
        let (base, wire, key) = known_api("https://llm.example/v1");
        assert!(base == "https://llm.example/v1" && wire == WireName::Chat && key);
        assert!(!known_api("http://localhost:1234/v1").2);
    }

    #[test]
    fn routes_read_as_people_call_them() {
        assert_eq!(Route::ChatGpt.words(), "ChatGPT");
        assert_eq!(Route::Cli("claude").words(), "Claude Code");
        assert_eq!(
            Route::Api {
                wire: WireName::Chat,
                base: String::new(),
                model: "m".into()
            }
            .words(),
            "API: m"
        );
    }
}
