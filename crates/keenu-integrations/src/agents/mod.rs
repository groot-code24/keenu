// SPDX-License-Identifier: GPL-3.0-or-later

//! Coding agents asking the user for permission, whichever agent it is.
//!
//! Follows each vendor's documented hooks (checked 2026-10-31):
//! <https://code.claude.com/docs/en/hooks> and <https://learn.chatgpt.com/docs/hooks>. Both call a command on
//! `PermissionRequest` with JSON on its standard input and read a decision from its standard output; no
//! output means no decision, and the agent asks in its own terminal as usual. That is Keenu's safe default:
//! if it is not running, does not answer in time or cannot read a request, nothing is allowed.

pub mod file;
pub mod json_hooks;
pub mod protocol;

use std::path::PathBuf;

use anyhow::Context;

use self::json_hooks::Spec;

/// The agents Keenu works with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
        }
    }

    /// The file holding this agent's hooks: `CLAUDE_CONFIG_DIR` and `CODEX_HOME` are honoured as the
    /// agents themselves honour them.
    pub fn hooks_file(self) -> Option<PathBuf> {
        let (variable, folder, file) = match self {
            Self::Claude => ("CLAUDE_CONFIG_DIR", ".claude", "settings.json"),
            Self::Codex => ("CODEX_HOME", ".codex", "hooks.json"),
        };
        let dir = std::env::var_os(variable)
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs_home().map(|home| home.join(folder)))?;
        Some(dir.join(file))
    }

    fn spec(self) -> &'static Spec {
        match self {
            Self::Claude => &CLAUDE,
            Self::Codex => &CODEX,
        }
    }

    /// What the user has to do after installing, in the agent itself.
    pub fn after_install(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            Self::Codex => Some(
                "Codex runs a new hook only once you have trusted it: open Codex and review it with /hooks.",
            ),
        }
    }
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

const CLAUDE: Spec = Spec {
    // 600 s is the documented longest a permission hook may wait.
    events: &[
        ("PermissionRequest", Some(600)),
        ("Notification", None),
        ("Stop", None),
    ],
    mark: "--agent-hook claude",
    retired: &[],
};

const CODEX: Spec = Spec {
    events: &[("PermissionRequest", Some(600)), ("Stop", None)],
    mark: "--agent-hook codex",
    retired: &[],
};

/// The command an agent runs to reach this program.
pub fn hook_command(executable: &std::path::Path, agent: Agent) -> String {
    format!("\"{}\" --agent-hook {}", executable.display(), agent.name())
}

/// Adds Keenu's hooks to `agent`'s settings, leaving everything else there as it was. If anything goes
/// wrong the file is back as it was before.
pub fn install(agent: Agent, command: &str) -> anyhow::Result<()> {
    let path = agent
        .hooks_file()
        .context("could not tell where your home folder is")?;
    let before = file::read(&path).with_context(|| format!("could not read {}", path.display()))?;
    let result = json_hooks::install(&path, command, agent.spec());
    if result.is_err() {
        restore(&path, before.as_deref());
    }
    result
}

/// Puts a file back as it was, or removes it if there was none.
fn restore(path: &std::path::Path, before: Option<&str>) {
    let done = match before {
        Some(text) => file::write_atomic(path, text),
        None if path.exists() => std::fs::remove_file(path),
        None => Ok(()),
    };
    if let Err(err) = done {
        log::warn!("could not put {} back: {err}", path.display());
    }
}

/// Takes Keenu's hooks out; whether there were any.
pub fn uninstall(agent: Agent) -> anyhow::Result<bool> {
    let path = agent
        .hooks_file()
        .context("could not tell where your home folder is")?;
    json_hooks::remove(&path, agent.spec())
}

/// Whether all of Keenu's hooks are in place, and the program they run is still there.
pub fn installed(agent: Agent) -> bool {
    agent
        .hooks_file()
        .is_some_and(|path| json_hooks::hooked(&path, agent.spec()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("keenu-install-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn hooks_go_in_beside_what_the_user_has_and_come_out_again() {
        let path = scratch("both");
        let mine = json!({
            "model": "opus",
            "hooks": {"Stop": [{"matcher": "", "hooks": [{"type": "command", "command": "mine.sh"}]}]},
        });
        std::fs::write(&path, serde_json::to_string_pretty(&mine).unwrap()).unwrap();
        json_hooks::install(&path, "\"C:\\k\\keenu.exe\" --agent-hook claude", &CLAUDE).unwrap();
        let installed: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(installed["model"], "opus");
        assert_eq!(
            installed["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"],
            600
        );
        let stop = installed["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "the user's own hook stays");
        assert!(json_hooks::remove(&path, &CLAUDE).unwrap());
        let removed: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(removed, mine);
    }

    #[test]
    fn a_file_that_is_not_json_is_left_alone() {
        let path = scratch("broken");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(json_hooks::install(&path, "x --agent-hook claude", &CLAUDE).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn restoring_puts_back_exactly_what_was_there() {
        let path = scratch("restore");
        std::fs::write(&path, "before").unwrap();
        std::fs::write(&path, "half-written").unwrap();
        restore(&path, Some("before"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "before");
        restore(&path, None);
        assert!(!path.exists());
    }

    #[test]
    fn the_command_is_quoted_for_paths_with_spaces() {
        let command = hook_command(
            std::path::Path::new("C:\\Program Files\\k.exe"),
            Agent::Codex,
        );
        assert_eq!(command, "\"C:\\Program Files\\k.exe\" --agent-hook codex");
    }

    #[test]
    fn a_hook_whose_program_is_gone_does_not_count_as_installed() {
        let path = scratch("gone");
        json_hooks::install(&path, "\"/no/such/keenu\" --agent-hook codex", &CODEX).unwrap();
        assert!(!json_hooks::hooked(&path, &CODEX));
    }
}
