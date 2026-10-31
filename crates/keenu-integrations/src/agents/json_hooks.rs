// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Keenu's hooks in a JSON hooks file of the shape Claude Code's settings.json and Codex's hooks.json share.
//! Everything else in the file is left as it was, in its order.

use std::path::Path;

use anyhow::{Context, bail};
use serde_json::{Value, json};

use super::file;

/// Which hooks are Keenu's: the events with the time (s) each may take, the mark in their command, and
/// events Keenu no longer uses, whose hooks go when it is turned off.
pub struct Spec {
    pub events: &'static [(&'static str, Option<u64>)],
    pub mark: &'static str,
    pub retired: &'static [&'static str],
}

/// Whether all of Keenu's hooks of `spec` are in the file.
pub fn hooked(path: &Path, spec: &Spec) -> bool {
    file::read(path)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .is_some_and(|settings| has_hooks(&settings, spec))
}

/// Adds Keenu's hooks (or points them at `command`); a file that is not JSON is never written.
pub fn install(path: &Path, command: &str, spec: &Spec) -> anyhow::Result<()> {
    let text = file::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let mut settings = parse(path, text.as_deref())?;
    if add_hooks(&mut settings, command, spec).with_context(|| format!("in {}", path.display()))? {
        save(path, &settings, text.as_deref())?;
    }
    Ok(())
}

/// Takes Keenu's hooks out; whether there were any.
pub fn remove(path: &Path, spec: &Spec) -> anyhow::Result<bool> {
    let text = file::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let Some(text) = text else {
        return Ok(false);
    };
    let mut settings = parse(path, Some(&text))?;
    let removed = remove_hooks(&mut settings, spec);
    if removed {
        save(path, &settings, Some(&text))?;
    }
    Ok(removed)
}

/// The file's JSON; a missing or empty file is an empty object.
fn parse(path: &Path, text: Option<&str>) -> anyhow::Result<Value> {
    match text {
        Some(text) if !text.trim().is_empty() => serde_json::from_str(text)
            .with_context(|| format!("{} is not valid JSON; fix it first", path.display())),
        _ => Ok(json!({})),
    }
}

/// Written as the AIs write it: two spaces, and the old file's final newline.
fn save(path: &Path, settings: &Value, old: Option<&str>) -> anyhow::Result<()> {
    let mut text = serde_json::to_string_pretty(settings)?;
    if old.is_none_or(|old| old.ends_with('\n')) {
        text.push('\n');
    }
    file::backup_once(path).with_context(|| format!("could not back up {}", path.display()))?;
    file::write_atomic(path, &text).with_context(|| format!("could not write {}", path.display()))
}

/// Keenu's hooks carry its mark in the command.
fn is_ours(hook: &Value, spec: &Spec) -> bool {
    hook["command"]
        .as_str()
        .is_some_and(|c| c.contains(spec.mark))
}

/// Every hook listed under `event`, whatever its matcher.
fn hooks_of<'a>(settings: &'a Value, event: &str) -> impl Iterator<Item = &'a Value> {
    settings["hooks"][event]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["hooks"].as_array())
        .flatten()
}

/// Every event has Keenu's hook, with the right timeout and an keenud still there.
fn has_hooks(settings: &Value, spec: &Spec) -> bool {
    spec.events.iter().all(|(event, timeout)| {
        hooks_of(settings, event).any(|hook| {
            is_ours(hook, spec)
                && hook["timeout"].as_u64() == *timeout
                && runs_something(hook["command"].as_str().unwrap_or_default())
        })
    })
}

/// Whether the keenud a hook runs is still there (not moved by a reinstall): else it is put in again.
fn runs_something(command: &str) -> bool {
    // Quoted one way on Linux and the other on Windows.
    let program = match command.chars().next() {
        Some(quote @ ('\'' | '"')) => command[1..].split(quote).next().unwrap_or_default(),
        _ => command.split_whitespace().next().unwrap_or_default(),
    };
    Path::new(program).is_file()
}

/// Whether anything changed.
pub fn add_hooks(settings: &mut Value, command: &str, spec: &Spec) -> anyhow::Result<bool> {
    let Some(settings) = settings.as_object_mut() else {
        bail!("the settings are not a JSON object");
    };
    let Some(hooks) = settings
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    else {
        bail!("`hooks` is not a JSON object");
    };
    let mut changed = false;
    for (event, timeout) in spec.events {
        let event = *event;
        let Some(list) = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
        else {
            bail!("`hooks.{event}` is not a list");
        };
        let ours = list
            .iter_mut()
            .filter_map(|entry| entry.get_mut("hooks").and_then(Value::as_array_mut))
            .flatten()
            .find(|hook| is_ours(hook, spec));
        let mut wanted = json!({"type": "command", "command": command});
        if let Some(timeout) = timeout {
            wanted["timeout"] = json!(timeout);
        }
        match ours {
            Some(hook) if hook["command"] == command && hook["timeout"].as_u64() == *timeout => {}
            // Kept, but run by this keenud (an older one may be gone), with the time it may take.
            Some(hook) => {
                *hook = wanted;
                changed = true;
            }
            None => {
                list.push(json!({"hooks": [wanted]}));
                changed = true;
            }
        }
    }
    Ok(changed)
}

/// Whether anything changed. Only lists Keenu emptied go, with `hooks` if that empties it.
pub fn remove_hooks(settings: &mut Value, spec: &Spec) -> bool {
    let Some(settings) = settings.as_object_mut() else {
        return false;
    };
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut removed = false;
    let mut emptied = false;
    // Every event Keenu ever used, so one it no longer uses goes too.
    for event in spec
        .events
        .iter()
        .map(|(event, _)| *event)
        .chain(spec.retired.iter().copied())
    {
        let Some(list) = hooks.get_mut(event).and_then(Value::as_array_mut) else {
            continue;
        };
        let mut found = false;
        list.retain_mut(|entry| {
            let Some(inner) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            let count = inner.len();
            inner.retain(|hook| !is_ours(hook, spec));
            found |= inner.len() < count;
            !(inner.is_empty() && count > 0)
        });
        removed |= found;
        if found && list.is_empty() {
            hooks.shift_remove(event);
            emptied = true;
        }
    }
    if emptied && hooks.is_empty() {
        settings.shift_remove("hooks");
    }
    removed
}
