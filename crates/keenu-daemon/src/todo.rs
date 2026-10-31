// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

use std::sync::Arc;

use keenu_core::todo::TodoList;
use log::warn;
use tokio::sync::Notify;
use zbus::object_server::{InterfaceRef, SignalEmitter};
use zbus::{fdo, interface};

use crate::clock::{next_midnight, now_ms, start_of_today, wait_until};
use crate::store::Store;

/// The to-do list on the bus, saved to disk after every change.
pub struct TodoService {
    list: TodoList,
    store: Store<TodoList>,
    wake: Arc<Notify>,
}

impl TodoService {
    /// Loads the saved list, dropping tasks finished before today.
    pub fn load() -> Self {
        let mut store = Store::new("todo");
        let mut list: TodoList = store.load();
        if list.drop_done_before(start_of_today()) {
            store.save(&list);
        }
        Self {
            list,
            store,
            wake: Arc::default(),
        }
    }

    /// The current list, for the assistant to read.
    pub fn list(&self) -> &TodoList {
        &self.list
    }

    /// Saves the list, rearms the midnight wait and tells the extension.
    async fn changed(&self, emitter: &SignalEmitter<'_>) -> zbus::Result<()> {
        self.store.save(&self.list);
        self.wake.notify_one();
        self.items_changed(emitter).await
    }
}

/// The error for a task id that is not on the list.
fn not_found(id: u64) -> fdo::Error {
    fdo::Error::InvalidArgs(format!("no task with id {id}"))
}

#[interface(name = "io.github.zickrian.isle.Todo1")]
impl TodoService {
    /// Adds a task and returns its id.
    async fn add(
        &mut self,
        text: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<u64> {
        let id = self
            .list
            .add(text)
            .ok_or_else(|| fdo::Error::InvalidArgs("a task needs a title".into()))?;
        self.changed(&emitter).await?;
        Ok(id)
    }

    /// Ticks a task off, or back on.
    async fn set_done(
        &mut self,
        id: u64,
        done: bool,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        if !self.list.set_done(id, done, now_ms()) {
            return Err(not_found(id));
        }
        Ok(self.changed(&emitter).await?)
    }

    /// Renames a task; a blank title is refused.
    async fn rename(
        &mut self,
        id: u64,
        title: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        if !self.list.rename(id, title) {
            return Err(fdo::Error::InvalidArgs(format!(
                "cannot rename task {id} to `{title}`"
            )));
        }
        Ok(self.changed(&emitter).await?)
    }

    /// Deletes a task.
    async fn remove(
        &mut self,
        id: u64,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        if !self.list.remove(id) {
            return Err(not_found(id));
        }
        Ok(self.changed(&emitter).await?)
    }

    /// Deletes every finished task.
    async fn clear_done(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> fdo::Result<()> {
        self.list.clear_done();
        Ok(self.changed(&emitter).await?)
    }

    /// JSON array of `TodoItem`.
    #[zbus(property)]
    fn items(&self) -> String {
        serde_json::to_string(self.list.items()).unwrap_or_default()
    }
}

/// Clears yesterday's finished tasks at midnight.
pub async fn run(iface: InterfaceRef<TodoService>) {
    let wake = iface.get().await.wake.clone();
    loop {
        if !wait_until(next_midnight(), &wake).await {
            continue;
        }
        let mut service = iface.get_mut().await;
        if !service.list.drop_done_before(start_of_today()) {
            continue;
        }
        if let Err(err) = service.changed(iface.signal_emitter()).await {
            warn!("could not clear yesterday's tasks: {err}");
        }
    }
}
