// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Pomodoro timer: focus sessions, short breaks, and a long break every few sessions.
//! Times are epoch milliseconds passed in by the caller, so the logic stays deterministic.

use serde::{Deserialize, Serialize};

const MINUTE_MS: i64 = 60_000;

/// Where the timer is in the cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Focus,
    ShortBreak,
    LongBreak,
}

/// Phase lengths and what starts on its own; saved apart from the timer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PomodoroConfig {
    pub focus_minutes: u32,
    pub short_break_minutes: u32,
    pub long_break_minutes: u32,
    pub sessions_before_long_break: u32,
    /// Start a break as soon as a focus session ends.
    pub auto_start_breaks: bool,
    /// Start the next focus session as soon as a break ends.
    pub auto_start_focus: bool,
}

impl Default for PomodoroConfig {
    /// The classic 25/5/15, a long break after four sessions.
    fn default() -> Self {
        Self {
            focus_minutes: 25,
            short_break_minutes: 5,
            long_break_minutes: 15,
            sessions_before_long_break: 4,
            auto_start_breaks: true,
            auto_start_focus: false,
        }
    }
}

impl PomodoroConfig {
    /// A phase's length; at least a minute, so a zero in the config can't end it at once.
    fn duration_ms(&self, phase: Phase) -> i64 {
        let minutes = match phase {
            Phase::Idle => 0,
            Phase::Focus => self.focus_minutes,
            Phase::ShortBreak => self.short_break_minutes,
            Phase::LongBreak => self.long_break_minutes,
        };
        i64::from(minutes.max(1)) * MINUTE_MS
    }
}

/// A phase that just ended and the one that follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub finished: Phase,
    pub next: Phase,
}

/// What the UI needs to draw the timer at a given moment.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PomodoroView {
    pub phase: Phase,
    pub running: bool,
    /// When the phase ends, if running.
    pub ends_at_ms: Option<i64>,
    /// When the phase first ran; none while it waits to be started.
    pub started_at_ms: Option<i64>,
    pub remaining_ms: i64,
    pub duration_ms: i64,
    pub completed_sessions: u32,
    pub sessions_before_long_break: u32,
}

/// The timer's progress, saved so a running or paused phase survives a
/// restart of the daemon. The configuration is stored on its own.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimerState {
    pub phase: Phase,
    pub running: bool,
    /// When the phase ends, if running.
    pub ends_at_ms: Option<i64>,
    /// When the phase first ran.
    pub started_at_ms: Option<i64>,
    /// Time left, if paused.
    pub remaining_ms: i64,
    pub duration_ms: i64,
    pub completed_sessions: u32,
}

/// The timer itself. Pure: the caller passes the time and acts on the transitions.
#[derive(Debug, Clone, Default)]
pub struct Pomodoro {
    config: PomodoroConfig,
    phase: Phase,
    /// Set while running.
    ends_at: Option<i64>,
    /// When the phase first ran: a phase waiting to be started has none.
    started_at: Option<i64>,
    /// Time left while not running.
    remaining_ms: i64,
    /// Length of the current phase, fixed when it began: a config change
    /// mid-phase must not change what the phase counts as (see `set_config`).
    duration_ms: i64,
    /// Focus sessions completed since the last long break.
    completed_sessions: u32,
}

impl Pomodoro {
    /// An idle timer.
    pub fn new(config: PomodoroConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    /// Picks up a saved timer. A running phase keeps its end time, so one that ran out
    /// meanwhile finishes on the next [`tick`](Self::tick).
    pub fn restore(config: PomodoroConfig, state: TimerState) -> Self {
        if state.phase == Phase::Idle {
            return Self::new(config);
        }
        let duration_ms = if state.duration_ms > 0 {
            state.duration_ms
        } else {
            config.duration_ms(state.phase)
        };
        Self {
            phase: state.phase,
            ends_at: state.ends_at_ms.filter(|_| state.running),
            started_at: state.started_at_ms,
            remaining_ms: state.remaining_ms.clamp(0, duration_ms),
            duration_ms,
            completed_sessions: state.completed_sessions,
            config,
        }
    }

    /// What [`restore`](Self::restore) needs to bring this timer back.
    pub fn state(&self) -> TimerState {
        TimerState {
            phase: self.phase,
            running: self.ends_at.is_some(),
            ends_at_ms: self.ends_at,
            started_at_ms: self.started_at,
            remaining_ms: self.remaining_ms,
            duration_ms: self.duration_ms,
            completed_sessions: self.completed_sessions,
        }
    }

    /// The durations in use.
    pub fn config(&self) -> &PomodoroConfig {
        &self.config
    }

    /// Applies new durations. The current phase keeps its length; later
    /// phases use the new one.
    pub fn set_config(&mut self, config: PomodoroConfig) {
        self.config = config;
    }

    /// The current phase; `Idle` before the first start.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// A snapshot for the UI; idle shows a full focus session.
    pub fn view(&self, now: i64) -> PomodoroView {
        PomodoroView {
            phase: self.phase,
            running: self.ends_at.is_some(),
            ends_at_ms: self.ends_at,
            started_at_ms: self.started_at,
            remaining_ms: self.remaining(now),
            duration_ms: if self.phase == Phase::Idle {
                self.config.duration_ms(Phase::Focus)
            } else {
                self.duration_ms
            },
            completed_sessions: self.completed_sessions,
            sessions_before_long_break: self.config.sessions_before_long_break,
        }
    }

    /// Time left at `now`; idle counts a whole focus session.
    pub fn remaining(&self, now: i64) -> i64 {
        match self.ends_at {
            Some(ends_at) => (ends_at - now).max(0),
            None if self.phase == Phase::Idle => self.config.duration_ms(Phase::Focus),
            None => self.remaining_ms,
        }
    }

    /// When the running phase ends, for scheduling a wake-up.
    pub fn deadline(&self) -> Option<i64> {
        self.ends_at
    }

    /// Starts a focus session from idle, or resumes a paused phase.
    pub fn start(&mut self, now: i64) {
        if self.ends_at.is_some() {
            return;
        }
        if self.phase == Phase::Idle {
            self.enter(Phase::Focus, now, true);
        } else {
            self.started_at.get_or_insert(now);
            self.ends_at = Some(now + self.remaining_ms);
        }
    }

    /// Starts `phase` right away, e.g. a short break from the timer's chips.
    /// Starting `Idle` stops the timer.
    pub fn start_phase(&mut self, phase: Phase, now: i64) {
        if phase == Phase::Idle {
            self.stop();
        } else {
            self.enter(phase, now, true);
        }
    }

    /// Freezes the time left; a no-op if already paused.
    pub fn pause(&mut self, now: i64) {
        if let Some(ends_at) = self.ends_at.take() {
            self.remaining_ms = (ends_at - now).max(0);
        }
    }

    /// Abandons the cycle and returns to idle.
    pub fn stop(&mut self) {
        *self = Self::new(self.config.clone());
    }

    /// Ends the current phase early. A skipped focus session does not count.
    pub fn skip(&mut self, now: i64) -> Option<Transition> {
        (self.phase != Phase::Idle).then(|| self.advance(now, false))
    }

    /// Advances to the next phase if the running one has ended.
    pub fn tick(&mut self, now: i64) -> Option<Transition> {
        match self.ends_at {
            Some(ends_at) if now >= ends_at => Some(self.advance(now, true)),
            _ => None,
        }
    }

    /// Moves to the next phase: a long break every few completed sessions, else a short one.
    fn advance(&mut self, now: i64, completed: bool) -> Transition {
        let finished = self.phase;
        let (next, autostart) = match finished {
            Phase::Focus => {
                if completed {
                    self.completed_sessions += 1;
                }
                let every = self.config.sessions_before_long_break.max(1);
                let long = completed && self.completed_sessions.is_multiple_of(every);
                let next = if long {
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                };
                (next, self.config.auto_start_breaks)
            }
            Phase::LongBreak => {
                self.completed_sessions = 0;
                (Phase::Focus, self.config.auto_start_focus)
            }
            Phase::ShortBreak | Phase::Idle => (Phase::Focus, self.config.auto_start_focus),
        };
        self.enter(next, now, autostart);
        Transition { finished, next }
    }

    /// Begins `phase` at its full length, running or paused.
    fn enter(&mut self, phase: Phase, now: i64, running: bool) {
        let duration = self.config.duration_ms(phase);
        self.phase = phase;
        self.duration_ms = duration;
        self.remaining_ms = duration;
        self.ends_at = running.then_some(now + duration);
        self.started_at = running.then_some(now);
    }
}

/// Focus completed on one day: how many sessions and for how long.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusStats {
    /// Local date the figures belong to, e.g. "2026-10-20".
    pub date: String,
    pub sessions: u32,
    pub focus_ms: i64,
}

impl FocusStats {
    /// Adds a completed focus session on `today`, starting afresh on a new day.
    pub fn record(&mut self, today: &str, focus_ms: i64) {
        if self.date != today {
            *self = Self {
                date: today.to_owned(),
                ..Self::default()
            };
        }
        self.sessions += 1;
        self.focus_ms += focus_ms.max(0);
    }

    /// The figures for `today`: zero if nothing was recorded yet today.
    pub fn on(&self, today: &str) -> (u32, i64) {
        if self.date == today {
            (self.sessions, self.focus_ms)
        } else {
            (0, 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quick() -> Pomodoro {
        Pomodoro::new(PomodoroConfig {
            focus_minutes: 2,
            short_break_minutes: 1,
            long_break_minutes: 3,
            sessions_before_long_break: 2,
            auto_start_breaks: true,
            auto_start_focus: true,
        })
    }

    #[test]
    fn runs_a_full_cycle() {
        let mut timer = quick();
        let mut now = 0;
        timer.start(now);
        let mut phases = Vec::new();
        for _ in 0..4 {
            now = timer.deadline().unwrap();
            phases.push(timer.tick(now).unwrap().next);
        }
        use Phase::*;
        assert_eq!(phases, [ShortBreak, Focus, LongBreak, Focus]);
        assert_eq!(timer.view(now).completed_sessions, 0);
    }

    #[test]
    fn start_phase_jumps_straight_in() {
        let mut timer = quick();
        timer.start_phase(Phase::ShortBreak, 0);
        let view = timer.view(0);
        assert_eq!(view.phase, Phase::ShortBreak);
        assert!(view.running);
        assert_eq!(timer.deadline(), Some(MINUTE_MS));
    }

    #[test]
    fn pause_keeps_remaining_time() {
        let mut timer = quick();
        timer.start(0);
        timer.pause(30_000);
        assert_eq!(timer.remaining(1_000_000), 90_000);
        timer.start(1_000_000);
        assert_eq!(timer.deadline(), Some(1_090_000));
    }

    #[test]
    fn does_not_tick_early() {
        let mut timer = quick();
        timer.start(0);
        assert_eq!(timer.tick(119_999), None);
    }

    #[test]
    fn skipped_focus_does_not_count() {
        let mut timer = quick();
        timer.start(0);
        let transition = timer.skip(10).unwrap();
        assert_eq!(transition.next, Phase::ShortBreak);
        assert_eq!(timer.view(10).completed_sessions, 0);
    }

    #[test]
    fn waits_when_autostart_is_off() {
        let mut timer = Pomodoro::new(PomodoroConfig {
            auto_start_breaks: false,
            ..PomodoroConfig::default()
        });
        timer.start(0);
        timer.tick(timer.deadline().unwrap());
        let view = timer.view(0);
        assert_eq!(view.phase, Phase::ShortBreak);
        assert!(!view.running);
        assert_eq!(view.remaining_ms, 5 * MINUTE_MS);
    }

    #[test]
    fn a_session_waiting_to_start_has_no_start_time() {
        let mut timer = Pomodoro::new(PomodoroConfig {
            auto_start_focus: false,
            ..quick().config().clone()
        });
        timer.start_phase(Phase::ShortBreak, 0);
        timer.tick(MINUTE_MS);
        assert_eq!(timer.view(MINUTE_MS).phase, Phase::Focus);
        assert_eq!(timer.view(MINUTE_MS).started_at_ms, None);
        // Started, it keeps its start through a pause, a resume and a restart of keenud.
        timer.start(70_000);
        timer.pause(80_000);
        timer.start(90_000);
        let restored = Pomodoro::restore(timer.config().clone(), timer.state());
        assert_eq!(restored.view(100_000).started_at_ms, Some(70_000));
        // The next phase is another.
        timer.skip(100_000);
        assert_eq!(timer.view(100_000).started_at_ms, Some(100_000));
    }

    #[test]
    fn config_changes_keep_the_current_phase_length() {
        let mut timer = quick();
        timer.start(0);
        timer.set_config(PomodoroConfig {
            focus_minutes: 50,
            ..timer.config().clone()
        });
        assert_eq!(timer.view(0).duration_ms, 2 * MINUTE_MS);
        assert_eq!(timer.deadline(), Some(2 * MINUTE_MS));
        // The next focus session uses the new length.
        timer.start_phase(Phase::Focus, 0);
        assert_eq!(timer.view(0).duration_ms, 50 * MINUTE_MS);
    }

    #[test]
    fn restores_a_paused_phase_paused() {
        let mut timer = quick();
        timer.start(0);
        timer.pause(30_000);
        let restored = Pomodoro::restore(timer.config().clone(), timer.state());
        let view = restored.view(5_000_000);
        assert_eq!(view.phase, Phase::Focus);
        assert!(!view.running);
        assert_eq!(view.remaining_ms, 90_000);
        assert_eq!(view.duration_ms, 2 * MINUTE_MS);
    }

    #[test]
    fn restores_a_running_phase_with_its_end_time() {
        let mut timer = quick();
        timer.start(0);
        let restored = Pomodoro::restore(timer.config().clone(), timer.state());
        assert_eq!(restored.deadline(), Some(2 * MINUTE_MS));
        assert_eq!(restored.view(60_000).remaining_ms, MINUTE_MS);
    }

    #[test]
    fn overdue_phase_finishes_on_the_first_tick() {
        let mut timer = quick();
        timer.start(0);
        let mut restored = Pomodoro::restore(timer.config().clone(), timer.state());
        // Back long after the session should have ended.
        let now = 10 * MINUTE_MS;
        let transition = restored.tick(now).unwrap();
        assert_eq!(transition.finished, Phase::Focus);
        assert_eq!(transition.next, Phase::ShortBreak);
        let view = restored.view(now);
        assert_eq!(view.completed_sessions, 1);
        // auto_start_breaks: the break starts now.
        assert_eq!(view.ends_at_ms, Some(now + MINUTE_MS));
    }

    #[test]
    fn idle_state_restores_to_idle() {
        let restored = Pomodoro::restore(PomodoroConfig::default(), TimerState::default());
        assert_eq!(restored.phase(), Phase::Idle);
        assert_eq!(restored.deadline(), None);
    }

    #[test]
    fn focus_stats_reset_each_day() {
        let mut stats = FocusStats::default();
        stats.record("2026-10-20", 25 * MINUTE_MS);
        stats.record("2026-10-20", 25 * MINUTE_MS);
        assert_eq!(stats.on("2026-10-20"), (2, 50 * MINUTE_MS));
        assert_eq!(stats.on("2026-10-21"), (0, 0));
        stats.record("2026-10-21", 10 * MINUTE_MS);
        assert_eq!(stats.on("2026-10-21"), (1, 10 * MINUTE_MS));
    }
}
