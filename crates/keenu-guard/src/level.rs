// SPDX-License-Identifier: GPL-3.0-or-later

//! How much each action can change, and what that means for it. An action this file does not know needs approval: the
//! list of what runs by itself is short and written out, so a new or misspelled action can never slip through it.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How much an action can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Only looks.
    Look,
    /// Small and easily undone.
    Reversible,
    /// Sends, deletes, installs or changes something: the user sees it first.
    Change,
    /// Passwords, payments, admin prompts: only the user does these.
    Never,
}

/// What to do about an action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Run,
    Ask,
    Refuse(String),
}

/// What the user has decided the assistant may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    /// Ask before small, reversible actions too.
    pub ask_for_everything: bool,
}

impl Default for Policy {
    /// Small actions run; anything more asks.
    fn default() -> Self {
        Self {
            ask_for_everything: false,
        }
    }
}

impl Policy {
    /// What to do about `action`.
    pub fn decide(self, action: &Value) -> Decision {
        match level_of(action) {
            Level::Look => Decision::Run,
            Level::Reversible if self.ask_for_everything => Decision::Ask,
            Level::Reversible => Decision::Run,
            Level::Change => Decision::Ask,
            Level::Never => Decision::Refuse(
                "That is something only you can do, so Keenu will not do it for you.".into(),
            ),
        }
    }
}

/// Actions that only look.
const LOOK: &[&str] = &[
    "check",
    "chrome_profiles",
    "find_apps",
    "find_files",
    "list_folder",
    "list_windows",
    "memory_search",
    "read_text",
    "read_window",
    "recent_files",
    "research",
    "search_x",
    "top_processes",
    "web_search",
];

/// Actions that change little, and that the user can undo with a click.
const REVERSIBLE: &[&str] = &[
    "add_event",
    "add_task",
    "arrange",
    "bluetooth",
    "brightness",
    "chrome_profile",
    "close_app",
    "dark_mode",
    "dnd",
    "done_task",
    "focus",
    "focus_window",
    "lock",
    "media",
    "memory_add",
    "night_light",
    "notifications",
    "open_app",
    "open_file",
    "open_folder",
    "open_url",
    "play_music",
    "power_mode",
    "remind",
    "reveal_file",
    "screenshot",
    "settings",
    "volume",
    "wifi",
    "window",
];

/// Words that say a control or field is about money or secrets, in the languages Keenu is used in most. Matching one
/// is never a judgement of safety, only a reason to leave it to the user.
const OFF_LIMITS: &[&str] = &[
    "password",
    "passphrase",
    "passcode",
    "kata sandi",
    "contraseña",
    "mot de passe",
    "pin code",
    "credit card",
    "card number",
    "cvv",
    "cvc",
    "security code",
    "one-time code",
    "otp",
    "2fa",
    "seed phrase",
    "recovery phrase",
    "private key",
    "api key",
    "pay now",
    "place order",
    "buy now",
    "confirm payment",
    "confirm purchase",
    "complete purchase",
    "transfer funds",
    "bayar sekarang",
];

/// Whether `text` holds one of `OFF_LIMITS` as words of their own: "otp" in "OTP code", not in "laptop".
fn names_something_off_limits(text: &str) -> bool {
    let text = text.to_lowercase();
    OFF_LIMITS.iter().any(|word| {
        text.match_indices(word).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + word.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    })
}

/// How much `action` can change.
pub fn level_of(action: &Value) -> Level {
    let kind = action["type"].as_str().unwrap_or_default();
    // What a control is called, and what would be typed into it: a page cannot rename its way past this, as the
    // platform reports a password field as one whatever its label says (see `secret`).
    if action["secret"].as_bool() == Some(true)
        || ["target", "name", "label", "title", "field"]
            .iter()
            .any(|key| action[*key].as_str().is_some_and(names_something_off_limits))
    {
        return Level::Never;
    }
    if LOOK.contains(&kind) {
        Level::Look
    } else if REVERSIBLE.contains(&kind) {
        Level::Reversible
    } else {
        Level::Change
    }
}

/// An action in the user's words, for its approval card and the log.
pub fn describe(action: &Value) -> String {
    let text = |key: &str| action[key].as_str().unwrap_or_default();
    let target = [text("target"), text("name"), text("label")]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or_default();
    let window = text("window");
    let within = if window.is_empty() {
        String::new()
    } else {
        format!(" in {window}")
    };
    match action["type"].as_str().unwrap_or_default() {
        "click" => format!("Press “{target}”{within}"),
        "type_text" => format!(
            "Type “{}” into {}{within}",
            text("text"),
            if target.is_empty() { "the field" } else { target }
        ),
        "press_keys" => format!("Press {}{within}", text("keys")),
        "open_app" => format!("Open {}", text("app")),
        "close_app" => format!("Close {}", text("app")),
        "open_url" => format!("Open {}", text("url")),
        "run" => format!("Run: {}", text("command")),
        "connector" => format!("{}: {}", text("service"), text("tool")),
        "web_search" | "research" => format!("Search the web for “{}”", text("query")),
        "search_x" => format!("Search X for “{}”", text("query")),
        "" => "Do something it did not name".into(),
        kind => kind.replace('_', " "),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn looking_runs_and_changing_asks() {
        let policy = Policy::default();
        assert_eq!(
            policy.decide(&json!({"type": "read_window"})),
            Decision::Run
        );
        assert_eq!(
            policy.decide(&json!({"type": "open_app", "app": "Excel"})),
            Decision::Run
        );
        assert_eq!(
            policy.decide(&json!({"type": "click", "target": "Send"})),
            Decision::Ask
        );
        assert_eq!(
            policy.decide(&json!({"type": "run", "command": "ls"})),
            Decision::Ask
        );
    }

    #[test]
    fn an_action_nobody_listed_asks_rather_than_runs() {
        for action in [
            json!({"type": "format_disk"}),
            json!({"type": "OPEN_APP"}),
            json!({"type": 7}),
            json!({}),
        ] {
            assert_eq!(level_of(&action), Level::Change, "{action}");
        }
    }

    #[test]
    fn asking_for_everything_leaves_only_looking_to_run() {
        let policy = Policy {
            ask_for_everything: true,
        };
        assert_eq!(policy.decide(&json!({"type": "volume"})), Decision::Ask);
        assert_eq!(
            policy.decide(&json!({"type": "find_files"})),
            Decision::Run
        );
    }

    #[test]
    fn secrets_and_payments_are_left_to_the_user_whatever_the_action() {
        for action in [
            json!({"type": "type_text", "target": "Password", "text": "x"}),
            json!({"type": "type_text", "target": "field", "secret": true, "text": "x"}),
            json!({"type": "click", "target": "Pay now"}),
            json!({"type": "click", "target": "PLACE ORDER"}),
            json!({"type": "click", "name": "Confirm purchase"}),
            json!({"type": "type_text", "label": "CVV", "text": "123"}),
            // Looking at it is off limits too, if it is named.
            json!({"type": "read_window", "title": "Your recovery phrase"}),
        ] {
            assert_eq!(level_of(&action), Level::Never, "{action}");
            assert!(matches!(
                Policy::default().decide(&action),
                Decision::Refuse(_)
            ));
        }
    }

    #[test]
    fn a_word_inside_another_is_not_that_word() {
        assert_eq!(
            level_of(&json!({"type": "click", "target": "Laptop settings"})),
            Level::Change
        );
        assert!(names_something_off_limits("OTP code"));
        assert!(!names_something_off_limits("Hotpot"));
    }

    #[test]
    fn says_what_will_happen_in_plain_words() {
        assert_eq!(
            describe(&json!({"type": "click", "target": "Send", "window": "Outlook"})),
            "Press “Send” in Outlook"
        );
        assert_eq!(
            describe(&json!({"type": "type_text", "text": "hi", "target": "Subject"})),
            "Type “hi” into Subject"
        );
        assert_eq!(describe(&json!({})), "Do something it did not name");
    }
}
