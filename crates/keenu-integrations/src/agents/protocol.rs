// SPDX-License-Identifier: GPL-3.0-or-later

//! What goes between an agent's hook and Keenu. A payload comes from outside and is only ever shown to
//! the user, never run: it is checked, bounded and stripped of anything that could make what is on
//! screen differ from what would run.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Agent;

/// The largest hook payload read.
pub const MAX_PAYLOAD: usize = 1024 * 1024;
/// Longest command or path shown; a longer one is marked as cut.
const MAX_DETAIL: usize = 2000;
const MAX_FIELD: usize = 512;

/// A permission request, checked and ready to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique to this request; a second answer or a replay of it does nothing.
    pub id: String,
    pub agent: Agent,
    pub tool: String,
    /// The command, path or address it wants.
    pub detail: String,
    /// The detail was too long to show whole: the user should not allow what they cannot see.
    pub truncated: bool,
    pub cwd: String,
    /// The last folder of `cwd`, the usual name of a project.
    pub project: String,
    /// The agent's own reason, where it gives one.
    pub description: String,
    /// What it would remember for next time if "always" were chosen, in words.
    pub suggested_permissions: Vec<String>,
}

/// What a hook sends to the running Keenu.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub token: String,
    pub request: ApprovalRequest,
}

/// What the user decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    Allow,
    Deny,
    /// Cancelled, timed out or unavailable: the agent asks in its terminal instead.
    NoDecision,
}

/// Reads a `PermissionRequest` payload. `Err` means it should be ignored, which leaves the agent asking in
/// its terminal: nothing gets allowed by a payload that cannot be read.
pub fn parse_request(agent: Agent, payload: &Value, id: String) -> Result<ApprovalRequest, String> {
    if payload["hook_event_name"] != "PermissionRequest" {
        return Err("not a permission request".into());
    }
    let tool = payload["tool_name"].as_str().ok_or("no tool name")?;
    let valid_tool = !tool.is_empty()
        && tool.len() <= MAX_FIELD
        && tool
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'));
    if !valid_tool {
        return Err("the tool name is not valid".into());
    }
    let input = &payload["tool_input"];
    if !input.is_object() && !input.is_null() {
        return Err("the tool input is not an object".into());
    }
    let cwd = clean(payload["cwd"].as_str().unwrap_or_default(), MAX_FIELD).0;
    let raw_detail = ["command", "file_path", "path", "url"]
        .iter()
        .find_map(|key| input[*key].as_str())
        .map_or_else(|| input.to_string(), str::to_owned);
    let (detail, truncated) = clean(&raw_detail, MAX_DETAIL);
    Ok(ApprovalRequest {
        id,
        agent,
        tool: tool.to_owned(),
        detail,
        truncated,
        project: cwd
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .to_owned(),
        cwd,
        description: clean(input["description"].as_str().unwrap_or_default(), MAX_FIELD).0,
        suggested_permissions: suggestions(&payload["permission_suggestions"]),
    })
}

/// Claude Code's suggested rules, in words. They are only shown: Keenu's answers never add a rule.
fn suggestions(value: &Value) -> Vec<String> {
    let rules = value.as_array().into_iter().flatten();
    rules
        .filter(|suggestion| suggestion["type"] == "addRules")
        .flat_map(|suggestion| suggestion["rules"].as_array().into_iter().flatten())
        .take(8)
        .filter_map(|rule| {
            let tool = rule["toolName"].as_str()?;
            let content = rule["ruleContent"].as_str().unwrap_or_default();
            Some(
                clean(&format!("{tool} {content}"), MAX_FIELD)
                    .0
                    .trim()
                    .to_owned(),
            )
        })
        .collect()
}

/// Makes text safe to show: control characters, and those that reorder or hide text so a command could
/// look like something else, become visible. Returns whether it had to be cut.
pub fn clean(text: &str, max_chars: usize) -> (String, bool) {
    let mut out = String::with_capacity(text.len().min(max_chars));
    for (count, c) in text.chars().enumerate() {
        if count == max_chars {
            return (out, true);
        }
        let hidden = matches!(c,
            '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}' | '\u{00AD}');
        match c {
            '\n' | '\t' => out.push(c),
            '\r' => {}
            c if c.is_control() || hidden => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
    (out, false)
}

/// What the hook prints for an answer, in the form both agents document. `None` prints nothing, so the
/// agent asks in its terminal. Codex rejects fields it does not know on this event, so only documented ones
/// are ever sent, and `message` only with a denial.
pub fn decision_output(answer: &Answer) -> Option<String> {
    let decision = match answer {
        Answer::Allow => json!({"behavior": "allow"}),
        Answer::Deny => json!({"behavior": "deny", "message": "Denied by the user in Keenu."}),
        Answer::NoDecision => return None,
    };
    Some(
        json!({
            "hookSpecificOutput": {
                "hookEventName": "PermissionRequest",
                "decision": decision,
            }
        })
        .to_string(),
    )
}

/// What to notify about from a non-permission event, or `None` for events not worth interrupting for.
pub fn notification_of(agent: Agent, payload: &Value) -> Option<(String, String)> {
    let (title, text) = match payload["hook_event_name"].as_str()? {
        "Stop" => (
            format!("{} finished", agent.title()),
            payload["last_assistant_message"]
                .as_str()
                .unwrap_or("The task is done."),
        ),
        "Notification" => (
            payload["title"].as_str().map_or_else(
                || agent.title().to_owned(),
                |title| clean(title, MAX_FIELD).0,
            ),
            payload["message"].as_str().unwrap_or_default(),
        ),
        _ => return None,
    };
    Some((title, clean(text, MAX_FIELD).0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The payload from Claude Code's documentation of `PermissionRequest`.
    fn documented_claude_payload() -> Value {
        json!({
            "session_id": "abc123",
            "transcript_path": "/Users/x/.claude/projects/p/1.jsonl",
            "cwd": "/Users/x/keenu",
            "permission_mode": "default",
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {"command": "rm -rf node_modules", "description": "Remove node_modules directory"},
            "permission_suggestions": [{
                "type": "addRules",
                "rules": [{"toolName": "Bash", "ruleContent": "rm -rf node_modules"}],
                "behavior": "allow",
                "destination": "localSettings"
            }]
        })
    }

    #[test]
    fn reads_claude_codes_documented_payload() {
        let request =
            parse_request(Agent::Claude, &documented_claude_payload(), "1".into()).unwrap();
        assert_eq!(request.tool, "Bash");
        assert_eq!(request.detail, "rm -rf node_modules");
        assert_eq!(request.project, "keenu");
        assert_eq!(request.description, "Remove node_modules directory");
        assert_eq!(request.suggested_permissions, ["Bash rm -rf node_modules"]);
        assert!(!request.truncated);
    }

    #[test]
    fn reads_codexs_documented_fields() {
        let payload = json!({
            "session_id": "s", "transcript_path": null, "cwd": "C:\\code\\app\\", "hook_event_name": "PermissionRequest",
            "model": "m", "turn_id": "t", "tool_name": "apply_patch",
            "tool_input": {"command": "*** Begin Patch", "description": null}, "permission_mode": "default",
        });
        let request = parse_request(Agent::Codex, &payload, "2".into()).unwrap();
        assert_eq!(request.tool, "apply_patch");
        assert_eq!(request.project, "app");
        assert_eq!(request.description, "");
    }

    #[test]
    fn mcp_tool_names_are_valid_and_other_input_is_shown_as_json() {
        let payload = json!({
            "hook_event_name": "PermissionRequest", "tool_name": "mcp__fs__read", "tool_input": {"a": 1},
        });
        let request = parse_request(Agent::Claude, &payload, "3".into()).unwrap();
        assert_eq!(request.detail, r#"{"a":1}"#);
    }

    #[test]
    fn what_cannot_be_read_is_refused() {
        for bad in [
            json!({"hook_event_name": "Stop", "tool_name": "Bash"}),
            json!({"hook_event_name": "PermissionRequest"}),
            json!({"hook_event_name": "PermissionRequest", "tool_name": ""}),
            json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash; rm -rf /"}),
            json!({"hook_event_name": "PermissionRequest", "tool_name": "Bash", "tool_input": "x"}),
            json!([]),
            json!(null),
        ] {
            assert!(
                parse_request(Agent::Claude, &bad, "x".into()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_command_cannot_disguise_itself_on_screen() {
        // Right-to-left override and an escape sequence that could rewrite the line.
        let payload = json!({
            "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "echo safe\u{202E}\u{1b}[2K rm -rf /\r\nnext"},
        });
        let request = parse_request(Agent::Claude, &payload, "4".into()).unwrap();
        assert!(!request.detail.contains('\u{202E}'));
        assert!(!request.detail.contains('\u{1b}'));
        assert!(!request.detail.contains('\r'));
        assert!(
            request.detail.contains("rm -rf /"),
            "what it would do stays visible"
        );
        assert!(request.detail.contains('\n'));
    }

    #[test]
    fn a_long_command_is_marked_as_cut() {
        let payload = json!({
            "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "x".repeat(MAX_DETAIL + 1)},
        });
        let request = parse_request(Agent::Claude, &payload, "5".into()).unwrap();
        assert!(request.truncated);
        assert_eq!(request.detail.chars().count(), MAX_DETAIL);
    }

    #[test]
    fn decisions_are_exactly_what_the_documentation_shows() {
        let allow: Value = serde_json::from_str(&decision_output(&Answer::Allow).unwrap()).unwrap();
        assert_eq!(
            allow,
            json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}})
        );
        let deny: Value = serde_json::from_str(&decision_output(&Answer::Deny).unwrap()).unwrap();
        assert_eq!(deny["hookSpecificOutput"]["decision"]["behavior"], "deny");
        assert!(deny["hookSpecificOutput"]["decision"]["message"].is_string());
        // Codex fails closed on these; Keenu never sends them.
        let text = decision_output(&Answer::Allow).unwrap();
        for field in ["updatedInput", "updatedPermissions", "interrupt", "message"] {
            assert!(!text.contains(field), "{field}");
        }
    }

    #[test]
    fn no_decision_prints_nothing_so_the_terminal_asks() {
        assert_eq!(decision_output(&Answer::NoDecision), None);
    }

    #[test]
    fn notifications_are_taken_from_stop_and_notification_only() {
        let (title, text) = notification_of(
            Agent::Codex,
            &json!({"hook_event_name": "Stop", "last_assistant_message": "All done"}),
        )
        .unwrap();
        assert_eq!(
            (title.as_str(), text.as_str()),
            ("Codex finished", "All done")
        );
        let (title, text) = notification_of(
            Agent::Claude,
            &json!({"hook_event_name": "Notification", "title": "Permission needed", "message": "Claude needs you"}),
        )
        .unwrap();
        assert_eq!(
            (title.as_str(), text.as_str()),
            ("Permission needed", "Claude needs you")
        );
        assert_eq!(
            notification_of(Agent::Claude, &json!({"hook_event_name": "PreToolUse"})),
            None
        );
    }
}
