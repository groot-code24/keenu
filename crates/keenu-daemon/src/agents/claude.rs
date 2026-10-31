// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Claude Code: hooks in its settings.json running `keenud --agent-hook claude`: `PermissionRequest` (its
//! permission prompt, answered from the island), `Notification` (it waits for the user) and `Stop` (done).

use std::path::{Path, PathBuf};

use keenu_integrations::agents::json_hooks::{self, Spec};

/// What marks a hook as Keenu's, whichever keenud it runs.
const MARK: &str = "--agent-hook claude";
/// The hook events Keenu listens to; a permission waits for the user's answer up to ten minutes.
pub const SPEC: Spec = Spec {
    events: &[
        ("PermissionRequest", Some(600)),
        ("Notification", None),
        ("Stop", None),
    ],
    mark: MARK,
    retired: &[],
};

/// Whether Keenu's hooks are in the settings file.
pub fn hooked(path: &Path) -> bool {
    json_hooks::hooked(path, &SPEC)
}

/// Adds Keenu's hooks beside the user's own, keeping a backup of the file.
pub fn install(path: &Path, command: &str) -> anyhow::Result<()> {
    json_hooks::install(path, command, &SPEC)
}

/// Takes Keenu's hooks out again; false if none were there.
pub fn remove(path: &Path) -> anyhow::Result<bool> {
    json_hooks::remove(path, &SPEC)
}

/// `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json`.
pub fn settings_path() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(
            || dirs::home_dir().unwrap_or_default().join(".claude"),
            PathBuf::from,
        )
        .join("settings.json")
}

/// The command each hook runs for `ai` ("claude", "codex"): `keenud` quoted for the shell it runs in.
pub fn command(keenud: &str, ai: &str) -> String {
    format!("{} --agent-hook {ai}", shell_quote(keenud))
}

/// Single quotes unless the path is plain, so a folder with a space still runs.
fn shell_quote(text: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._+-".contains(c);
    if !text.is_empty() && text.chars().all(plain) {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::{Value, json};

    use super::*;
    use crate::agents::test_scratch as scratch;
    use keenu_integrations::agents::file;
    use keenu_integrations::agents::json_hooks::{add_hooks, remove_hooks};

    // A program that is there, as an installed keenud would be (a hook to a missing one counts as out).
    const COMMAND: &str = "/bin/sh --agent-hook claude";

    #[test]
    fn hooks_go_in_beside_the_users_own() {
        let dir = scratch("claude-install");
        let path = dir.join("settings.json");
        let before = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "notify-send done"
          }
        ]
      }
    ],
    "PreToolUse": []
  },
  "env": {
    "B": "1",
    "A": "2"
  }
}
"#;
        fs::write(&path, before).unwrap();
        assert!(!hooked(&path));
        install(&path, COMMAND).unwrap();
        assert!(hooked(&path));
        let text = fs::read_to_string(&path).unwrap();
        let settings: Value = serde_json::from_str(&text).unwrap();
        // Keys keep their order; the user's own hook stays first.
        let keys: Vec<&String> = settings.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "hooks", "env"]);
        let env: Vec<&String> = settings["env"].as_object().unwrap().keys().collect();
        assert_eq!(env, ["B", "A"]);
        assert_eq!(
            settings["hooks"]["Stop"][0]["hooks"][0]["command"],
            "notify-send done"
        );
        assert_eq!(
            settings["hooks"]["Stop"][1],
            json!({"hooks": [{"type": "command", "command": COMMAND}]})
        );
        assert_eq!(
            settings["hooks"]["Notification"][0]["hooks"][0]["command"],
            COMMAND
        );
        assert_eq!(
            settings["hooks"]["PermissionRequest"][0],
            json!({"hooks": [{"type": "command", "command": COMMAND, "timeout": 600}]})
        );
        assert!(text.contains("\n  \"model\": \"opus\",\n") && text.ends_with("}\n"));
        assert_eq!(
            fs::read_to_string(file::sibling(&path, ".keenu-backup")).unwrap(),
            before
        );

        // Twice is once.
        install(&path, COMMAND).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), text);

        // Off again: the file is as it was.
        assert!(remove(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert!(!remove(&path).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_file_is_made_and_emptied_again() {
        let dir = scratch("claude-new");
        let path = dir.join("sub/settings.json");
        // An keenud in a folder with a space, quoted in the hook; once it is gone, the hook counts as out.
        let keenud = dir.join("my bin/keenud");
        fs::create_dir_all(keenud.parent().unwrap()).unwrap();
        fs::write(&keenud, "").unwrap();
        install(&path, &command(&keenud.to_string_lossy(), "claude")).unwrap();
        assert!(hooked(&path));
        fs::remove_file(&keenud).unwrap();
        assert!(!hooked(&path));
        assert!(!file::sibling(&path, ".keenu-backup").exists());
        assert!(remove(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn bad_json_is_never_written() {
        let dir = scratch("claude-bad");
        let path = dir.join("settings.json");
        fs::write(&path, "{\"model\": ").unwrap();
        let err = install(&path, COMMAND).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"), "{err}");
        assert!(remove(&path).is_err());
        fs::write(&path, r#"{"hooks": []}"#).unwrap();
        assert!(install(&path, COMMAND).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"hooks": []}"#);
        assert!(!file::sibling(&path, ".keenu-backup").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_moved_isled_is_followed() {
        let mut settings = json!({});
        assert!(add_hooks(&mut settings, "/old/keenud --agent-hook claude", &SPEC).unwrap());
        assert!(add_hooks(&mut settings, COMMAND, &SPEC).unwrap());
        assert!(!add_hooks(&mut settings, COMMAND, &SPEC).unwrap());
        assert_eq!(settings["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], COMMAND);
    }

    #[test]
    fn only_isles_hooks_are_removed() {
        let mut settings = json!({
            "hooks": {
                "Notification": [
                    {"hooks": [
                        {"type": "command", "command": "mine"},
                        {"type": "command", "command": COMMAND}
                    ]}
                ],
                "Stop": [{"hooks": [{"type": "command", "command": COMMAND}]}],
                "SessionStart": []
            }
        });
        assert!(remove_hooks(&mut settings, &SPEC));
        assert_eq!(
            settings,
            json!({
                "hooks": {
                    "Notification": [{"hooks": [{"type": "command", "command": "mine"}]}],
                    "SessionStart": []
                }
            })
        );
        assert!(!remove_hooks(&mut settings, &SPEC));
        // A list already empty is the user's to keep.
        let mut settings = json!({"hooks": {"Stop": []}});
        assert!(!remove_hooks(&mut settings, &SPEC));
        assert_eq!(settings, json!({"hooks": {"Stop": []}}));
    }

    #[test]
    fn odd_paths_are_quoted() {
        assert_eq!(
            command("/usr/bin/keenud", "claude"),
            "/usr/bin/keenud --agent-hook claude"
        );
        assert_eq!(
            command("/home/a b/it's/keenud", "claude"),
            r"'/home/a b/it'\''s/keenud' --agent-hook claude"
        );
    }
}
