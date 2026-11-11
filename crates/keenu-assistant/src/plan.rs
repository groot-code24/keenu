// SPDX-License-Identifier: GPL-3.0-or-later

//! Plans: a task in steps, with the AI's own score and estimate beside what this computer's record says. The record
//! is the action log: how often each kind of step the plan uses has worked here before, and how earlier plans ended.
//! A plan is kept for Keenu (plans.json) and for the user, as a document they can read, and each step is marked off
//! as it is done.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use keenu_guard::{ActionLog, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// More steps than this is several plans.
const MAX_STEPS: usize = 30;
/// One line of a plan, at most.
const MAX_TEXT: usize = 400;
/// Issues, risks or improvements, at most.
const MAX_LIST: usize = 10;
/// How much of the action log is read for the record.
const HISTORY: usize = 5000;
/// Plans kept; older ones go, their documents stay.
const KEPT: usize = 200;
/// What a plan's document name may hold.
const MAX_NAME: usize = 60;

/// Where a step is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Waiting,
    Done,
    Failed,
    Skipped,
}

impl StepStatus {
    fn parse(text: &str) -> Option<Self> {
        match text.trim().to_lowercase().as_str() {
            "done" | "ok" | "finished" | "complete" | "completed" => Some(Self::Done),
            "failed" | "fail" | "error" => Some(Self::Failed),
            "skipped" | "skip" => Some(Self::Skipped),
            "waiting" | "todo" | "pending" => Some(Self::Waiting),
            _ => None,
        }
    }

    fn words(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// One step of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub what: String,
    /// The kind of action it uses (`open_app`, `send_message`…), which the record is kept by; empty if none.
    #[serde(default)]
    pub uses: String,
    pub status: StepStatus,
    #[serde(default)]
    pub note: String,
}

/// What this computer's record says about a plan's steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The chance every step with a record works, in percent.
    pub chance: u8,
    /// How often those kinds of actions failed here, in percent.
    pub failure_rate: u8,
    /// How many earlier actions of those kinds it rests on.
    pub tries: usize,
    /// The kinds that failed most, with how often they worked, in percent.
    pub weakest: Vec<(String, u8)>,
}

/// A plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// When it was made, in ms since 1970; also its name.
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub goal: String,
    pub steps: Vec<Step>,
    /// The AI's score for the plan itself, out of 100.
    pub score: u8,
    /// The AI's estimate of the chance it works, in percent.
    pub chance: u8,
    #[serde(default)]
    pub issues: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default)]
    pub improvements: Vec<String>,
    #[serde(default)]
    pub record: Option<Record>,
    /// Its document for the user.
    #[serde(default)]
    pub file: String,
    #[serde(default)]
    pub finished: bool,
}

impl Plan {
    /// The first step not yet done, by its number from 1.
    pub fn next_step(&self) -> Option<(usize, &Step)> {
        self.steps
            .iter()
            .enumerate()
            .find(|(_, step)| step.status == StepStatus::Waiting)
            .map(|(index, step)| (index + 1, step))
    }

    fn count(&self, status: StepStatus) -> usize {
        self.steps.iter().filter(|s| s.status == status).count()
    }

    /// The plan as a document.
    pub fn markdown(&self) -> String {
        let mut text = format!("# {}\n\n", self.title);
        if !self.goal.is_empty() {
            text.push_str(&format!("Goal: {}\n\n", self.goal));
        }
        text.push_str(&format!(
            "- Plan score: {}/100 (the AI's view of the plan)\n- Chance it works: {}% (the AI's estimate)\n",
            self.score, self.chance
        ));
        match &self.record {
            Some(record) => {
                text.push_str(&format!(
                    "- From this computer's record: {}% for every step together, from {} earlier actions of these kinds\n- Failure rate of these kinds of actions here: {}%\n",
                    record.chance, record.tries, record.failure_rate
                ));
                for (kind, worked) in &record.weakest {
                    text.push_str(&format!("- Weakest here: {kind}, which worked {worked}% of the time\n"));
                }
            }
            None => text.push_str(
                "- No record yet on this computer for these kinds of steps: the chance is the AI's estimate alone\n",
            ),
        }
        text.push_str("\n## Steps\n\n");
        for (index, step) in self.steps.iter().enumerate() {
            let mark = if step.status == StepStatus::Done {
                "x"
            } else {
                " "
            };
            text.push_str(&format!("- [{mark}] {}. {}", index + 1, step.what));
            if step.status != StepStatus::Waiting && step.status != StepStatus::Done {
                text.push_str(&format!(" ({})", step.status.words()));
            }
            if !step.note.is_empty() {
                text.push_str(&format!(": {}", step.note));
            }
            text.push('\n');
        }
        for (heading, items) in [
            ("Issues", &self.issues),
            ("How it could fail", &self.risks),
            ("Improvements", &self.improvements),
        ] {
            if !items.is_empty() {
                text.push_str(&format!("\n## {heading}\n\n"));
                for item in items {
                    text.push_str(&format!("- {item}\n"));
                }
            }
        }
        if self.finished {
            text.push_str(&format!(
                "\nFinished: {} done, {} failed, {} skipped.\n",
                self.count(StepStatus::Done),
                self.count(StepStatus::Failed),
                self.count(StepStatus::Skipped)
            ));
        }
        text
    }
}

/// `text` on one line, cut to a size.
fn line(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.chars().count() <= MAX_TEXT {
        return joined;
    }
    let mut cut: String = joined.chars().take(MAX_TEXT).collect();
    cut.push('…');
    cut
}

/// A list of lines, as AIs write one: a list, or one line.
fn lines(value: &Value) -> Vec<String> {
    let items: Vec<String> = match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(line))
            .collect(),
        Value::String(one) => vec![line(one)],
        _ => Vec::new(),
    };
    items
        .into_iter()
        .filter(|item| !item.is_empty())
        .take(MAX_LIST)
        .collect()
}

/// A percentage as AIs write one: 75, 0.75 or "75%".
fn percent(value: &Value) -> Option<u8> {
    let number = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().trim_end_matches('%').trim().parse().ok()?,
        _ => return None,
    };
    let number = if number > 0.0 && number <= 1.0 && !matches!(value, Value::String(_)) {
        number * 100.0
    } else {
        number
    };
    Some(number.round().clamp(0.0, 100.0) as u8)
}

/// The steps of a plan as the AI wrote them: lines, or objects with what to do and what it uses.
fn steps(value: &Value) -> Vec<Step> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let (what, uses) = match item {
                Value::String(text) => (line(text), String::new()),
                Value::Object(_) => {
                    let what = ["do", "what", "step", "text", "title"]
                        .iter()
                        .find_map(|key| item[*key].as_str())
                        .map(line)
                        .unwrap_or_default();
                    let uses = ["uses", "action", "tool", "type"]
                        .iter()
                        .find_map(|key| item[*key].as_str())
                        .map(|kind| kind.trim().to_lowercase().replace([' ', '-'], "_"))
                        .unwrap_or_default();
                    (what, uses)
                }
                _ => return None,
            };
            (!what.is_empty()).then_some(Step {
                what,
                uses,
                status: StepStatus::Waiting,
                note: String::new(),
            })
        })
        .take(MAX_STEPS)
        .collect()
}

/// What the action log says about the kinds of action `steps` use.
pub fn record(steps: &[Step], log: &ActionLog) -> Option<Record> {
    let mut tally: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for step in steps.iter().filter(|s| !s.uses.is_empty()) {
        tally.insert(step.uses.as_str(), (0, 0));
    }
    if tally.is_empty() {
        return None;
    }
    for entry in log.newest(HISTORY) {
        if let Some((done, failed)) = tally.get_mut(entry.kind.as_str()) {
            match entry.outcome {
                Outcome::Done => *done += 1,
                Outcome::Failed => *failed += 1,
                _ => {}
            }
        }
    }
    let tries: usize = tally.values().map(|(done, failed)| done + failed).sum();
    if tries == 0 {
        return None;
    }
    // Each kind's rate with one success and one failure added, so two tries are not taken as certainty.
    let rate = |done: usize, failed: usize| (done as f64 + 1.0) / ((done + failed) as f64 + 2.0);
    let mut chance = 1.0;
    for step in steps.iter().filter(|s| !s.uses.is_empty()) {
        if let Some((done, failed)) = tally.get(step.uses.as_str())
            && done + failed > 0
        {
            chance *= rate(*done, *failed);
        }
    }
    let failed: usize = tally.values().map(|(_, failed)| failed).sum();
    let mut weakest: Vec<(String, u8)> = tally
        .iter()
        .filter(|(_, (_, failed))| *failed > 0)
        .map(|(kind, (done, failed))| {
            let worked = *done as f64 / (*done + *failed) as f64;
            ((*kind).to_owned(), (worked * 100.0).round() as u8)
        })
        .collect();
    weakest.sort_by_key(|(_, worked)| *worked);
    weakest.truncate(3);
    Some(Record {
        chance: (chance * 100.0).round().clamp(0.0, 100.0) as u8,
        failure_rate: (failed as f64 * 100.0 / tries as f64).round() as u8,
        tries,
        weakest,
    })
}

/// The plans, and where their documents go.
#[derive(Debug, Clone)]
pub struct Plans {
    file: PathBuf,
    folder: PathBuf,
}

impl Plans {
    /// Plans kept in `file`, their documents in `folder`.
    pub fn new(file: impl Into<PathBuf>, folder: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            folder: folder.into(),
        }
    }

    fn load(&self) -> Vec<Plan> {
        std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, plans: &[Plan]) -> Result<(), String> {
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let text = serde_json::to_string_pretty(plans).map_err(|err| err.to_string())?;
        let temp = self.file.with_extension("tmp");
        std::fs::write(&temp, text)
            .and_then(|()| std::fs::rename(&temp, &self.file))
            .map_err(|err| format!("the plan could not be kept: {err}"))
    }

    /// The plan being worked on: the newest one not finished.
    pub fn current(&self) -> Option<Plan> {
        self.load().into_iter().rev().find(|plan| !plan.finished)
    }

    /// The plan being worked on, for the AI with each request; empty when there is none.
    pub fn for_prompt(&self) -> String {
        let Some(plan) = self.current() else {
            return String::new();
        };
        let next = plan.next_step().map_or_else(
            || "every step has an outcome".to_owned(),
            |(number, step)| format!("next is step {number}: {}", step.what),
        );
        format!(
            "A plan is under way: “{}”, {} of {} steps done; {next}. Mark steps with plan_step as you go.\n",
            crate::soften(&plan.title),
            plan.count(StepStatus::Done),
            plan.steps.len()
        )
    }

    /// Writes `plan`'s document, at its own path once it has one.
    fn write_document(&self, plan: &mut Plan) -> Result<(), String> {
        if plan.file.is_empty() {
            std::fs::create_dir_all(&self.folder)
                .map_err(|err| format!("could not make {}: {err}", self.folder.display()))?;
            let name: String = plan
                .title
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == ' ' || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .take(MAX_NAME)
                .collect();
            let name = if name.trim().is_empty() {
                "Plan"
            } else {
                name.trim()
            };
            let stamp = chrono::DateTime::from_timestamp_millis(plan.id)
                .map(|at| {
                    at.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d %H.%M")
                        .to_string()
                })
                .unwrap_or_default();
            plan.file = self
                .folder
                .join(format!("{name} {stamp}.md"))
                .to_string_lossy()
                .into_owned();
        }
        std::fs::write(&plan.file, plan.markdown())
            .map_err(|err| format!("could not write the plan's document: {err}"))
    }

    /// Makes a plan from the AI's `plan` action; what to tell the AI.
    pub fn make(&self, action: &Value, log: &ActionLog, now_ms: i64) -> String {
        let steps = steps(&action["steps"]);
        if steps.is_empty() {
            return "plan: a plan needs its steps, as a list in \"steps\". Make it again.".into();
        }
        let title = action["title"]
            .as_str()
            .map(line)
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "Plan".into());
        let mut plan = Plan {
            id: now_ms,
            title,
            goal: action["goal"].as_str().map(line).unwrap_or_default(),
            record: record(&steps, log),
            steps,
            score: percent(&action["score"]).unwrap_or(0),
            chance: ["chance", "success", "probability"]
                .iter()
                .find_map(|key| percent(&action[*key]))
                .unwrap_or(0),
            issues: lines(&action["issues"]),
            risks: [&action["risks"], &action["failure_modes"]]
                .iter()
                .flat_map(|value| lines(value))
                .take(MAX_LIST)
                .collect(),
            improvements: lines(&action["improvements"]),
            file: String::new(),
            finished: false,
        };
        let mut plans = self.load();
        let earlier: Vec<&Plan> = plans.iter().filter(|p| p.finished).collect();
        let earlier_line = if earlier.is_empty() {
            String::new()
        } else {
            let whole = earlier
                .iter()
                .filter(|p| p.steps.iter().all(|s| s.status == StepStatus::Done))
                .count();
            format!(
                "Earlier plans here: {whole} of {} finished with every step done.\n",
                earlier.len()
            )
        };
        // A new plan takes over from one left unfinished.
        for old in plans.iter_mut().filter(|p| !p.finished) {
            old.finished = true;
        }
        let written = self.write_document(&mut plan);
        let record_line = match &plan.record {
            Some(record) => {
                let weakest: Vec<String> = record
                    .weakest
                    .iter()
                    .map(|(kind, worked)| format!("{kind} worked {worked}% of the time"))
                    .collect();
                format!(
                    "From this computer's record ({} earlier actions of these kinds): {}% chance that every step works; failure rate {}%.{}\n",
                    record.tries,
                    record.chance,
                    record.failure_rate,
                    if weakest.is_empty() {
                        String::new()
                    } else {
                        format!(" Weakest: {}.", weakest.join("; "))
                    }
                )
            }
            None => "No record yet on this computer for these kinds of steps, so the chance is your estimate alone: say so.\n".into(),
        };
        let summary = format!(
            "plan: “{}”, {} steps. Your score {}/100, your chance {}%.\n{record_line}{earlier_line}{}\
             Tell the user in a sentence or two: the score, both chances, the biggest risk, and what would improve it; \
             then ask whether to start (listen true), unless they already said to go ahead. Then do the steps in \
             order, and after each one use plan_step with its number and how it went.",
            plan.title,
            plan.steps.len(),
            plan.score,
            plan.chance,
            match &written {
                Ok(()) => format!("Saved for the user as {}.\n", plan.file),
                Err(why) => format!("Not saved as a document: {why}.\n"),
            },
        );
        plans.push(plan);
        let extra = plans.len().saturating_sub(KEPT);
        plans.drain(..extra);
        match self.save(&plans) {
            Ok(()) => summary,
            Err(why) => format!("{summary}\n(Keenu could not keep it: {why}.)"),
        }
    }

    /// Marks a step of the current plan from the AI's `plan_step` action; what to tell the AI.
    pub fn step(&self, action: &Value) -> String {
        let mut plans = self.load();
        let Some(plan) = plans.iter_mut().rev().find(|p| !p.finished) else {
            return "plan_step: no plan is under way. Make one with plan first.".into();
        };
        let Some(status) = action["status"].as_str().and_then(StepStatus::parse) else {
            return "plan_step: say how the step went: done, failed or skipped.".into();
        };
        let number = action["step"]
            .as_u64()
            .or_else(|| {
                action["step"]
                    .as_f64()
                    .filter(|n| *n >= 1.0)
                    .map(|n| n.round() as u64)
            })
            .or_else(|| action["step"].as_str().and_then(|s| s.trim().parse().ok()))
            .map(|n| n as usize)
            .or_else(|| plan.next_step().map(|(n, _)| n));
        let Some(number) = number.filter(|n| (1..=plan.steps.len()).contains(n)) else {
            return format!("plan_step: the plan has steps 1 to {}.", plan.steps.len());
        };
        let step = &mut plan.steps[number - 1];
        step.status = status;
        step.note = action["note"].as_str().map(line).unwrap_or_default();
        let total = plan.steps.len();
        plan.finished = plan.steps.iter().all(|s| s.status != StepStatus::Waiting);
        let told = if plan.finished {
            format!(
                "plan_step: step {number} {}. The plan is finished: {} done, {} failed, {} skipped. Tell the user plainly how it went.",
                status.words(),
                plan.count(StepStatus::Done),
                plan.count(StepStatus::Failed),
                plan.count(StepStatus::Skipped)
            )
        } else {
            let next = plan.next_step().map_or_else(String::new, |(n, s)| {
                format!(" Next is step {n} of {total}: {}.", s.what)
            });
            format!(
                "plan_step: step {number} of {total} {}.{next}",
                status.words()
            )
        };
        let mut plan = plan.clone();
        let written = self.write_document(&mut plan);
        if let Some(kept) = plans.iter_mut().find(|p| p.id == plan.id) {
            *kept = plan;
        }
        match (self.save(&plans), written) {
            (Ok(()), Ok(())) => told,
            (Err(why), _) | (_, Err(why)) => format!("{told} ({why}.)"),
        }
    }

    /// Where the plans are kept.
    pub fn path(&self) -> &Path {
        &self.file
    }
}

#[cfg(test)]
mod tests {
    use keenu_guard::{Entry, Level};
    use serde_json::json;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keenu-plan-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn logged(log: &ActionLog, kind: &str, outcome: Outcome, times: usize) {
        for _ in 0..times {
            log.add(&Entry {
                at_ms: 1,
                request: String::new(),
                level: Level::Change,
                kind: kind.into(),
                action: kind.into(),
                outcome,
                detail: String::new(),
            })
            .unwrap();
        }
    }

    #[test]
    fn reads_scores_and_steps_however_the_ai_writes_them() {
        assert_eq!(percent(&json!(75)), Some(75));
        assert_eq!(percent(&json!(0.8)), Some(80));
        assert_eq!(percent(&json!("64%")), Some(64));
        assert_eq!(percent(&json!(140)), Some(100));
        assert_eq!(percent(&json!("high")), None);
        let read = steps(&json!([
            "Open WhatsApp",
            {"do": "Send Ana the time", "uses": "send-message"},
            {"what": "  "},
            7
        ]));
        assert_eq!(read.len(), 2);
        assert_eq!(read[1].uses, "send_message");
    }

    #[test]
    fn the_record_comes_from_what_happened_here() {
        let dir = scratch("record");
        let log = ActionLog::new(dir.join("actions.log"));
        let plan_steps = steps(&json!([
            {"do": "Open the app", "uses": "open_app"},
            {"do": "Send it", "uses": "send_message"},
            {"do": "Think about it"}
        ]));
        assert_eq!(record(&plan_steps, &log), None);
        logged(&log, "open_app", Outcome::Done, 8);
        logged(&log, "send_message", Outcome::Done, 1);
        logged(&log, "send_message", Outcome::Failed, 1);
        logged(&log, "send_message", Outcome::Denied, 5);
        logged(&log, "click", Outcome::Failed, 9);
        let found = record(&plan_steps, &log).unwrap();
        assert_eq!(found.tries, 10);
        assert_eq!(found.failure_rate, 10);
        // (8+1)/(8+2) × (1+1)/(2+2) = 0.45
        assert_eq!(found.chance, 45);
        assert_eq!(found.weakest, vec![("send_message".to_owned(), 50)]);
    }

    #[test]
    fn a_plan_is_kept_marked_off_and_finished() {
        let dir = scratch("flow");
        let log = ActionLog::new(dir.join("actions.log"));
        logged(&log, "open_app", Outcome::Done, 3);
        let plans = Plans::new(dir.join("plans.json"), dir.join("Plans"));
        assert!(
            plans
                .make(&json!({"title": "x", "steps": []}), &log, 1)
                .contains("needs its steps")
        );
        let said = plans.make(
            &json!({
                "title": "Weekly report / draft",
                "goal": "Send the report by Friday",
                "steps": [{"do": "Open Excel", "uses": "open_app"}, "Write the summary"],
                "score": 82, "success": 0.7,
                "issues": ["The data may be late"],
                "risks": "Excel may ask to sign in",
                "improvements": ["Start Thursday"]
            }),
            &log,
            1_700_000_000_000,
        );
        assert!(said.contains("82/100") && said.contains("70%"), "{said}");
        assert!(said.contains("record (3 earlier actions"), "{said}");
        let plan = plans.current().unwrap();
        let document_path = Path::new(&plan.file);
        assert!(document_path.exists());
        assert_eq!(document_path.parent(), Some(dir.join("Plans").as_path()));
        assert!(
            document_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("Weekly report _ draft ")
        );
        assert!(plans.for_prompt().contains("0 of 2 steps done"));
        assert!(
            plans
                .step(&json!({"step": 1, "status": "done"}))
                .contains("Next is step 2")
        );
        assert!(
            plans
                .step(&json!({"status": "maybe"}))
                .contains("done, failed or skipped")
        );
        assert!(
            plans
                .step(&json!({"step": 9, "status": "done"}))
                .contains("steps 1 to 2")
        );
        let end =
            plans.step(&json!({"step": 2, "status": "failed", "note": "Excel would not open"}));
        assert!(end.contains("finished: 1 done, 1 failed"), "{end}");
        assert!(plans.current().is_none() && plans.for_prompt().is_empty());
        let document = std::fs::read_to_string(&plan.file).unwrap();
        assert!(document.contains("- [x] 1. Open Excel"));
        assert!(document.contains("(failed): Excel would not open"));
        assert!(document.contains("## How it could fail"));
        // The next plan hears how this one went.
        let next = plans.make(
            &json!({"title": "y", "steps": ["a"]}),
            &log,
            1_700_000_100_000,
        );
        assert!(
            next.contains("0 of 1 finished with every step done"),
            "{next}"
        );
        assert!(next.contains("No record yet"), "{next}");
    }
}
