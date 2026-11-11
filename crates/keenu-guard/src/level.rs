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
    /// The user lets the assistant look at the screen without asking each time. Clicking on it still asks.
    pub see_screen: bool,
}

impl Default for Policy {
    /// Small actions run; anything more asks.
    fn default() -> Self {
        Self {
            ask_for_everything: false,
            see_screen: false,
        }
    }
}

impl Policy {
    /// What to do about `action`.
    pub fn decide(self, action: &Value) -> Decision {
        let level = level_of(action);
        if self.see_screen && level == Level::Change && action["type"] == "look_at_screen" {
            return Decision::Run;
        }
        match level {
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
    "mail_read",
    "mail_search",
    "memory_search",
    "note_read",
    "notes_list",
    "notes_search",
    "plan",
    "plan_step",
    "read_page",
    "read_text",
    "read_window",
    "recent_files",
    "research",
    "search_x",
    "system_info",
    "top_processes",
    "web_search",
];

/// A page address up to this long is read without asking; a longer one could carry what the AI read elsewhere.
const MAX_QUIET_URL: usize = 300;

/// Actions that change little, and that the user can undo with a click.
const REVERSIBLE: &[&str] = &[
    "add_event",
    "add_task",
    "arrange",
    "bluetooth",
    "brightness",
    "chrome_profile",
    "close_app",
    "copy_text",
    "dark_mode",
    "dnd",
    "done_task",
    "focus",
    "focus_window",
    "lock",
    "mail_draft",
    "media",
    "memory_add",
    "night_light",
    "note_add",
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
            .any(|key| {
                action[*key]
                    .as_str()
                    .is_some_and(names_something_off_limits)
            })
    {
        return Level::Never;
    }
    // A page address long enough to carry what the AI read elsewhere is shown to the user first.
    if kind == "read_page"
        && action["url"]
            .as_str()
            .is_some_and(|url| url.chars().count() > MAX_QUIET_URL)
    {
        return Level::Change;
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
            if target.is_empty() {
                "the field"
            } else {
                target
            }
        ),
        "press_keys" => format!("Press {}{within}", text("keys")),
        "open_app" => format!("Open {}", text("app")),
        "close_app" => format!("Close {}", text("app")),
        "open_url" => format!("Open {}", text("url")),
        "run" => format!("Run: {}", text("command")),
        "connector" => format!("{}: {}", text("service"), text("tool")),
        "web_search" | "research" => format!("Search the web for “{}”", text("query")),
        "write" => format!("Type “{}” where the cursor is", text("text")),
        "read_clipboard" => "Read what is on the clipboard".into(),
        "power" => match text("command")
            .to_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "sleep" => "Put the computer to sleep".into(),
            "restart" | "reboot" => {
                "Restart the computer now: unsaved work in open apps is lost".into()
            }
            "sign_out" | "log_out" | "logout" => {
                "Sign out now: unsaved work in open apps is lost".into()
            }
            _ => "Shut down the computer now: unsaved work in open apps is lost".into(),
        },
        "search_x" => format!("Search X for “{}”", text("query")),
        "look_at_screen" => {
            "Look at your screen: a picture of it goes to the AI you connected".into()
        }
        "pointer" => {
            let at = format!(
                "{}, {}",
                action["x"].as_i64().unwrap_or_default(),
                action["y"].as_i64().unwrap_or_default()
            );
            let what = if target.is_empty() {
                String::new()
            } else {
                format!(" (“{target}”)")
            };
            match text("command") {
                "double_click" => format!("Double-click at {at} on the screen{what}"),
                "right_click" => format!("Right-click at {at} on the screen{what}"),
                "move" => format!("Move the pointer to {at}{what}"),
                "scroll" => format!("Scroll at {at} on the screen{what}"),
                "drag" => format!(
                    "Drag from {at} to {}, {} on the screen{what}",
                    action["to_x"].as_i64().unwrap_or_default(),
                    action["to_y"].as_i64().unwrap_or_default()
                ),
                _ => format!("Click at {at} on the screen{what}"),
            }
        }
        "send_message" => format!(
            "Send “{}” to {} on {}",
            text("text"),
            text("to"),
            match text("app").to_lowercase().as_str() {
                "telegram" => "Telegram",
                _ => "WhatsApp",
            }
        ),
        "mail_send" => format!("Send an email to {}: “{}”", text("to"), text("subject")),
        "mail_draft" => format!(
            "Save a draft email to {}: “{}”",
            text("to"),
            text("subject")
        ),
        "note_add" => format!("Add to the note “{}”", text("title")),
        "read_page" => format!("Read the page {}", text("url")),
        "plan" => format!("Plan: {}", text("title")),
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
    fn ending_the_session_typing_for_the_user_and_reading_the_clipboard_ask() {
        let policy = Policy::default();
        for action in [
            json!({"type": "power", "command": "shut_down"}),
            json!({"type": "write", "text": "Dear Ana"}),
            json!({"type": "read_clipboard"}),
        ] {
            assert_eq!(policy.decide(&action), Decision::Ask, "{action}");
        }
        assert_eq!(
            policy.decide(&json!({"type": "system_info"})),
            Decision::Run
        );
        assert_eq!(
            policy.decide(&json!({"type": "volume", "level": 30})),
            Decision::Run
        );
        assert!(describe(&json!({"type": "power", "command": "restart"})).contains("unsaved work"));
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
            see_screen: false,
        };
        assert_eq!(policy.decide(&json!({"type": "volume"})), Decision::Ask);
        assert_eq!(policy.decide(&json!({"type": "find_files"})), Decision::Run);
    }

    #[test]
    fn planning_and_notes_run_and_sending_or_using_the_screen_asks() {
        let policy = Policy::default();
        for quiet in [
            json!({"type": "plan", "title": "Move house"}),
            json!({"type": "plan_step", "step": 1, "status": "done"}),
            json!({"type": "notes_search", "query": "milk"}),
            json!({"type": "note_add", "title": "Groceries", "text": "milk"}),
            json!({"type": "read_page", "url": "https://example.org/a"}),
            json!({"type": "mail_search", "query": "invoice"}),
            json!({"type": "mail_draft", "to": "a@b.c", "subject": "Hi"}),
        ] {
            assert_eq!(policy.decide(&quiet), Decision::Run, "{quiet}");
        }
        for asked in [
            json!({"type": "look_at_screen"}),
            json!({"type": "pointer", "command": "click", "x": 10, "y": 20}),
            json!({"type": "send_message", "app": "whatsapp", "to": "+1555", "text": "hi"}),
            json!({"type": "mail_send", "to": "a@b.c", "subject": "Hi", "text": "x"}),
            json!({"type": "read_page", "url": format!("https://x.example/?q={}", "a".repeat(400))}),
        ] {
            assert_eq!(policy.decide(&asked), Decision::Ask, "{asked}");
        }
        assert_eq!(
            describe(
                &json!({"type": "send_message", "app": "telegram", "to": "@ana", "text": "On my way"})
            ),
            "Send “On my way” to @ana on Telegram"
        );
        assert!(describe(&json!({"type": "look_at_screen"})).contains("picture"));
        assert_eq!(
            describe(&json!({"type": "pointer", "command": "double_click", "x": 5, "y": 6})),
            "Double-click at 5, 6 on the screen"
        );
    }

    #[test]
    fn seeing_the_screen_can_be_allowed_but_clicking_on_it_still_asks() {
        let policy = Policy {
            ask_for_everything: false,
            see_screen: true,
        };
        assert_eq!(
            policy.decide(&json!({"type": "look_at_screen"})),
            Decision::Run
        );
        assert_eq!(
            policy.decide(&json!({"type": "pointer", "x": 1, "y": 1})),
            Decision::Ask
        );
        // Not past what only the user may do.
        assert!(matches!(
            policy.decide(&json!({"type": "look_at_screen", "title": "Your recovery phrase"})),
            Decision::Refuse(_)
        ));
        assert!(matches!(
            policy.decide(&json!({"type": "pointer", "x": 1, "y": 1, "target": "Pay now"})),
            Decision::Refuse(_)
        ));
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
