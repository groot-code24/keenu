// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

use keenu_core::reminders::Reminders;
use zbus::object_server::SignalEmitter;
use zbus::{fdo, interface};

use crate::clock::now_ms;
use crate::store::Store;

/// The longest delay a reminder takes: a week.
const MAX_DELAY_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// The reminders D-Bus service, kept on disk so they survive a restart.
pub struct RemindersService {
    reminders: Reminders,
    store: Store<Reminders>,
}

impl RemindersService {
    /// Reads the saved reminders, or starts with none.
    pub fn load() -> Self {
        let mut store = Store::new("reminders");
        let reminders = store.load();
        Self { reminders, store }
    }

    /// The current reminders, for the daemon's own checks.
    pub fn reminders(&self) -> &Reminders {
        &self.reminders
    }
}

#[interface(name = "io.github.zickrian.isle.Reminders1")]
impl RemindersService {
    /// Adds a reminder due `delay_ms` from now and returns its id.
    async fn add(
        &mut self,
        delay_ms: u64,
        text: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<u64> {
        if delay_ms > MAX_DELAY_MS {
            return Err(fdo::Error::InvalidArgs(
                "a reminder is at most a week away".into(),
            ));
        }
        let id = self
            .reminders
            .add(text, now_ms() + delay_ms as i64)
            .ok_or_else(|| fdo::Error::InvalidArgs("a reminder needs text, and room".into()))?;
        self.store.save(&self.reminders);
        self.items_changed(&emitter).await?;
        Ok(id)
    }

    /// Forgets a reminder once it has been shown.
    async fn dismiss(
        &mut self,
        id: u64,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        if self.reminders.dismiss(id) {
            self.store.save(&self.reminders);
            self.items_changed(&emitter).await?;
        }
        Ok(())
    }

    /// JSON array of `{id, text, due_ms}`, soonest first.
    #[zbus(property)]
    fn items(&self) -> String {
        serde_json::to_string(self.reminders.items()).unwrap_or_default()
    }
}
