// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

use std::sync::Arc;

use keenu_core::pomodoro::{FocusStats, Phase, Pomodoro, PomodoroConfig, PomodoroView, TimerState};
use log::warn;
use serde::Serialize;
use tokio::sync::Notify;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::{fdo, interface};

use crate::clock::{next_midnight, now_ms, today, wait_until};
use crate::store::Store;

/// What the `State` property holds: the timer plus today's focus figures.
#[derive(Serialize)]
struct State {
    #[serde(flatten)]
    timer: PomodoroView,
    today_sessions: u32,
    today_focus_ms: i64,
}

/// The Pomodoro1 D-Bus object: the timer, its settings and the focus figures, each saved to disk.
pub struct PomodoroService {
    timer: Pomodoro,
    store: Store<PomodoroConfig>,
    /// The running or paused phase, so it survives keenud restarting.
    timer_store: Store<TimerState>,
    stats: FocusStats,
    stats_store: Store<FocusStats>,
    wake: Arc<Notify>,
}

impl PomodoroService {
    /// Restores the settings, the phase in progress and the figures from disk.
    pub fn load() -> Self {
        let mut store = Store::new("pomodoro");
        let mut timer_store = Store::new("pomodoro-timer");
        let mut stats_store = Store::new("focus-stats");
        // A phase that ran out while keenud was down is finished by `run`'s
        // first tick, like any other.
        Self {
            timer: Pomodoro::restore(store.load(), timer_store.load()),
            store,
            timer_store,
            stats: stats_store.load(),
            stats_store,
            wake: Arc::default(),
        }
    }

    /// Saves and publishes the new state, and reschedules the phase-end
    /// wake-up. Only called when the state changes, never on a mere tick.
    async fn changed(&self, emitter: &SignalEmitter<'_>) -> zbus::Result<()> {
        self.timer_store.save(&self.timer.state());
        self.wake.notify_one();
        self.state_changed(emitter).await
    }
}

#[interface(name = "io.github.zickrian.isle.Pomodoro1")]
impl PomodoroService {
    /// Starts a focus session, or resumes a paused phase.
    async fn start(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.timer.start(now_ms());
        Ok(self.changed(&emitter).await?)
    }

    /// Pauses the running phase.
    async fn pause(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.timer.pause(now_ms());
        Ok(self.changed(&emitter).await?)
    }

    /// Abandons the current cycle.
    async fn stop(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.timer.stop();
        Ok(self.changed(&emitter).await?)
    }

    /// Starts a phase right away: "focus", "short_break", "long_break", or
    /// "idle" to stop.
    async fn start_phase(
        &mut self,
        phase: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let phase: Phase = serde_json::from_value(serde_json::Value::String(phase.to_owned()))
            .map_err(|_| fdo::Error::InvalidArgs(format!("unknown phase `{phase}`")))?;
        self.timer.start_phase(phase, now_ms());
        Ok(self.changed(&emitter).await?)
    }

    /// Ends the current phase early.
    async fn skip(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.timer.skip(now_ms());
        Ok(self.changed(&emitter).await?)
    }

    /// Replaces the configuration with a JSON-encoded `PomodoroConfig`.
    async fn set_config(
        &mut self,
        config: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        let config: PomodoroConfig =
            serde_json::from_str(config).map_err(|err| fdo::Error::InvalidArgs(err.to_string()))?;
        self.store.save(&config);
        self.timer.set_config(config);
        self.config_changed(&emitter).await?;
        Ok(self.changed(&emitter).await?)
    }

    /// JSON `PomodoroView`, plus `today_sessions` and `today_focus_ms`.
    #[zbus(property)]
    fn state(&self) -> String {
        let (today_sessions, today_focus_ms) = self.stats.on(&today());
        let state = State {
            timer: self.timer.view(now_ms()),
            today_sessions,
            today_focus_ms,
        };
        serde_json::to_string(&state).unwrap_or_default()
    }

    /// JSON `PomodoroConfig`.
    #[zbus(property)]
    fn config(&self) -> String {
        serde_json::to_string(self.timer.config()).unwrap_or_default()
    }

    /// A phase ended on its own (not skipped or stopped).
    #[zbus(signal)]
    async fn phase_finished(
        emitter: &SignalEmitter<'_>,
        finished: &str,
        next: &str,
    ) -> zbus::Result<()>;
}

/// Advances the timer whenever a phase runs out, records finished focus
/// sessions, and refreshes the state at midnight when "today" resets.
pub async fn run(iface: InterfaceRef<PomodoroService>) {
    let wake = iface.get().await.wake.clone();
    loop {
        let phase_end = iface.get().await.timer.deadline();
        let deadline = [phase_end, next_midnight()].into_iter().flatten().min();
        if !wait_until(deadline, &wake).await {
            continue;
        }

        let mut service = iface.get_mut().await;
        let now = now_ms();
        // The length the session was started with, even if the config has
        // changed since.
        let focus_ms = service.timer.view(now).duration_ms;
        let transition = service.timer.tick(now);
        if transition.is_some_and(|t| t.finished == Phase::Focus) {
            service.stats.record(&today(), focus_ms);
            service.stats_store.save(&service.stats);
        }
        if transition.is_some() {
            service.timer_store.save(&service.timer.state());
        }

        let emitter = iface.signal_emitter();
        let result = async {
            service.state_changed(emitter).await?;
            if let Some(transition) = transition {
                PomodoroService::phase_finished(
                    emitter,
                    &phase_name(transition.finished),
                    &phase_name(transition.next),
                )
                .await?;
            }
            zbus::Result::Ok(())
        };
        if let Err(err) = result.await {
            warn!("could not announce the pomodoro phase change: {err}");
        }
    }
}

/// The phase's name as used on D-Bus, e.g. "short_break".
fn phase_name(phase: Phase) -> String {
    serde_json::to_value(phase)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}
