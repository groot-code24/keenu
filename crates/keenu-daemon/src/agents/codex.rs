// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Codex: `keenud --agent-hook codex` hooks in hooks.json for approvals and done. A Codex before 0.124 gets
//! a `notify` in config.toml instead (done alone), and only when no other program has it.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use toml_edit::{Array, DocumentMut, Item, value};

use keenu_integrations::agents::file;
use keenu_integrations::agents::json_hooks::{self, Spec};

/// The hook events Keenu listens to; an approval waits for the user's answer up to ten minutes.
pub const SPEC: Spec = Spec {
    events: &[("PermissionRequest", Some(600)), ("Stop", None)],
    mark: "--agent-hook codex",
    retired: &[],
};
/// The first Codex with hooks.
const HOOKS_SINCE: (u32, u32) = (0, 124);

/// `$CODEX_HOME/hooks.json`, else `~/.codex/hooks.json`.
pub fn hooks_path() -> PathBuf {
    config_path().with_file_name("hooks.json")
}

/// Whether the installed Codex has hooks; unknown counts as yes (a current one).
fn has_hooks() -> bool {
    let Some(program) = crate::assistant::Provider::Codex.program() else {
        return true;
    };
    let output = std::process::Command::new(program)
        .arg("--version")
        .output();
    let text = output
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    version(&text).is_none_or(|(major, minor)| (major, minor) >= HOOKS_SINCE)
}

/// "codex-cli 0.163.0" → (0, 163).
fn version(text: &str) -> Option<(u32, u32)> {
    let numbers = text
        .split_whitespace()
        .find(|word| word.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = numbers.split('.').map(|part| part.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??))
}

/// Whether Codex runs Keenu's hooks (or, an older one, notifies Keenu). An older `notify` left after Codex
/// gained hooks counts as not hooked, so the hooks go in at the next sync.
pub fn hooked(path: &Path) -> bool {
    json_hooks::hooked(&hooks_path(), &SPEC) || (notifies(path) && !has_hooks())
}

/// Keenu's hooks in hooks.json, and an older `notify` of Keenu's taken out; an old Codex gets `notify`.
pub fn install(path: &Path, keenud: &str, command: &str) -> anyhow::Result<()> {
    if !has_hooks() {
        return install_notify(path, keenud);
    }
    json_hooks::install(&hooks_path(), command, &SPEC)?;
    remove_notify(path)?;
    Ok(())
}

/// Takes Keenu's hooks and `notify` out; whether there were any.
pub fn remove(path: &Path) -> anyhow::Result<bool> {
    let hooks = json_hooks::remove(&hooks_path(), &SPEC)?;
    let notify = remove_notify(path)?;
    Ok(hooks || notify)
}

/// `$CODEX_HOME/config.toml`, else `~/.codex/config.toml`.
pub fn config_path() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .filter(|dir| !dir.is_empty())
        .map_or_else(
            || dirs::home_dir().unwrap_or_default().join(".codex"),
            PathBuf::from,
        )
        .join("config.toml")
}

/// The strings of a TOML array, such as `notify`'s command line.
fn words(item: &Item) -> Vec<&str> {
    let array = item.as_array().into_iter().flatten();
    array.filter_map(|word| word.as_str()).collect()
}

/// Whether a `notify` runs some keenud, this one or another install's.
fn is_ours(item: &Item) -> bool {
    words(item)
        .windows(2)
        .any(|pair| pair == ["--agent-hook", "codex"])
}

/// Keenu's `notify` command line.
fn notify(keenud: &str) -> Array {
    Array::from_iter([keenud, "--agent-hook", "codex"])
}

/// Whether Codex notifies Keenu.
fn notifies(path: &Path) -> bool {
    file::read(path)
        .ok()
        .flatten()
        .and_then(|text| text.parse::<DocumentMut>().ok())
        .is_some_and(|config| config.get("notify").is_some_and(is_ours))
}

/// Points `notify` at keenud; never over another program's.
fn install_notify(path: &Path, keenud: &str) -> anyhow::Result<()> {
    let text = file::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let mut config = parse(path, text.as_deref())?;
    match config.get("notify") {
        // Keenu's, but maybe another keenud's: pointed at this one.
        Some(item) if is_ours(item) => {
            if words(item) == [keenud, "--agent-hook", "codex"] {
                return Ok(());
            }
        }
        Some(_) => bail!("Codex already sends its notifications to another program"),
        None => {}
    }
    config["notify"] = value(notify(keenud));
    save(path, &config)
}

/// Takes Keenu's `notify` out; whether it was there.
fn remove_notify(path: &Path) -> anyhow::Result<bool> {
    let text = file::read(path).with_context(|| format!("could not read {}", path.display()))?;
    let Some(text) = text else {
        return Ok(false);
    };
    let mut config = parse(path, Some(&text))?;
    if !config.get("notify").is_some_and(is_ours) {
        return Ok(false);
    }
    config.remove("notify");
    save(path, &config)?;
    Ok(true)
}

/// A missing config reads as empty; a broken one is left for the user to fix.
fn parse(path: &Path, text: Option<&str>) -> anyhow::Result<DocumentMut> {
    text.unwrap_or_default()
        .parse()
        .with_context(|| format!("{} is not valid TOML; fix it first", path.display()))
}

/// Writes the config whole, keeping the user's original as a backup the first time.
fn save(path: &Path, config: &DocumentMut) -> anyhow::Result<()> {
    file::backup_once(path).with_context(|| format!("could not back up {}", path.display()))?;
    file::write_atomic(path, &config.to_string())
        .with_context(|| format!("could not write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::agents::test_scratch as scratch;

    const KEENUD: &str = "/usr/bin/keenud";

    #[test]
    fn reads_codexs_version() {
        assert_eq!(version("codex-cli 0.163.0\n"), Some((0, 163)));
        assert_eq!(version("codex-cli 0.98.2"), Some((0, 98)));
        assert_eq!(version("nonsense"), None);
        assert!((0, 98) < HOOKS_SINCE && (0, 163) >= HOOKS_SINCE);
    }

    #[test]
    fn notify_goes_in_above_the_tables_and_comes_out_again() {
        let dir = scratch("codex-install");
        let path = dir.join("config.toml");
        let before = "# My Codex\nmodel = \"gpt-5\"  # the best\n\n[projects.\"/home/a\"]\ntrust_level = \"trusted\"\n";
        fs::write(&path, before).unwrap();
        assert!(!notifies(&path));
        install_notify(&path, KEENUD).unwrap();
        assert!(notifies(&path));
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            "# My Codex\nmodel = \"gpt-5\"  # the best\nnotify = [\"/usr/bin/keenud\", \"--agent-hook\", \"codex\"]\n\n[projects.\"/home/a\"]\ntrust_level = \"trusted\"\n"
        );
        assert_eq!(
            fs::read_to_string(file::sibling(&path, ".keenu-backup")).unwrap(),
            before
        );
        install_notify(&path, KEENUD).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert!(remove_notify(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert!(!remove_notify(&path).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn another_programs_notify_is_left_alone() {
        let dir = scratch("codex-other");
        let path = dir.join("config.toml");
        let before = "notify = [\"notify-send\", \"Codex\"]\n";
        fs::write(&path, before).unwrap();
        let err = install_notify(&path, KEENUD).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Codex already sends its notifications to another program"
        );
        assert!(!remove_notify(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        fs::write(&path, "notify = [").unwrap();
        assert!(install_notify(&path, KEENUD).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "notify = [");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_config_is_made_with_one_line() {
        let dir = scratch("codex-new");
        let path = dir.join("codex/config.toml");
        install_notify(&path, "/home/a b/.local/bin/keenud").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "notify = [\"/home/a b/.local/bin/keenud\", \"--agent-hook\", \"codex\"]\n"
        );
        // Another keenud's entry is Keenu's too: pointed at this one.
        install_notify(&path, KEENUD).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "notify = [\"/usr/bin/keenud\", \"--agent-hook\", \"codex\"]\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
