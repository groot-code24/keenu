// SPDX-License-Identifier: GPL-3.0-or-later

//! `keenu --agent-hook claude|codex`: what a coding agent runs. It passes the agent's request to the running
//! Keenu and prints the answer. With no Keenu, no token or no answer in time it prints nothing, and the
//! agent asks in its own terminal: nothing is ever allowed by default.

use std::io::Read;
use std::time::Duration;

use keenu_integrations::agents::Agent;
use keenu_integrations::agents::protocol::{
    self, Answer, ApprovalRequest, MAX_PAYLOAD, decision_output, notification_of, parse_request,
};

/// The agents give a hook ten minutes; a little less leaves time to answer in.
const MOST_WAIT: Duration = Duration::from_secs(580);

/// Reads the agent's payload and does what it needs. Returns what to print, if anything.
pub fn run(
    agent_name: &str,
    input: &mut dyn Read,
    token: Option<String>,
    timeout: Duration,
    send: &dyn Fn(&protocol::Message, Duration) -> Answer,
    notify: &dyn Fn(&str, &str),
) -> Option<String> {
    let agent = Agent::from_name(agent_name)?;
    let mut bytes = Vec::new();
    input
        .take(MAX_PAYLOAD as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    if payload["hook_event_name"] != "PermissionRequest" {
        if let Some((title, text)) = notification_of(agent, &payload) {
            notify(&title, &text);
        }
        return None;
    }
    let id = keenu_security::random_hex(16).ok()?;
    let request: ApprovalRequest = parse_request(agent, &payload, id).ok()?;
    let message = protocol::Message {
        token: token?,
        request,
    };
    let answer = send(&message, timeout.min(MOST_WAIT));
    decision_output(&answer)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    const PERMISSION: &str = r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash","cwd":"/x/app","tool_input":{"command":"ls"}}"#;

    fn hook(
        input: &str,
        token: Option<&str>,
        answer: Answer,
        sent: &RefCell<Vec<ApprovalRequest>>,
    ) -> Option<String> {
        run(
            "claude",
            &mut input.as_bytes(),
            token.map(str::to_owned),
            Duration::from_secs(1),
            &|message, _| {
                sent.borrow_mut().push(message.request.clone());
                answer.clone()
            },
            &|_, _| {},
        )
    }

    #[test]
    fn an_answer_is_printed_as_the_agent_expects() {
        let sent = RefCell::new(Vec::new());
        let out = hook(PERMISSION, Some("t"), Answer::Allow, &sent).unwrap();
        assert!(out.contains("\"behavior\":\"allow\""));
        assert_eq!(sent.borrow()[0].detail, "ls");
    }

    #[test]
    fn with_keenu_not_answering_nothing_is_printed() {
        let sent = RefCell::new(Vec::new());
        assert_eq!(hook(PERMISSION, Some("t"), Answer::NoDecision, &sent), None);
    }

    #[test]
    fn without_a_token_keenu_is_not_even_asked() {
        let sent = RefCell::new(Vec::new());
        assert_eq!(hook(PERMISSION, None, Answer::Allow, &sent), None);
        assert!(sent.borrow().is_empty());
    }

    #[test]
    fn garbage_is_ignored() {
        let sent = RefCell::new(Vec::new());
        for bad in [
            "",
            "not json",
            "[]",
            r#"{"hook_event_name":"PermissionRequest"}"#,
        ] {
            assert_eq!(hook(bad, Some("t"), Answer::Allow, &sent), None, "{bad}");
        }
        assert!(sent.borrow().is_empty());
    }

    #[test]
    fn an_unknown_agent_is_ignored() {
        let out = run(
            "skynet",
            &mut PERMISSION.as_bytes(),
            Some("t".into()),
            Duration::from_secs(1),
            &|_, _| Answer::Allow,
            &|_, _| {},
        );
        assert_eq!(out, None);
    }

    #[test]
    fn a_finished_task_notifies_and_prints_nothing() {
        let notes = RefCell::new(Vec::new());
        let out = run(
            "codex",
            &mut r#"{"hook_event_name":"Stop","last_assistant_message":"Done"}"#.as_bytes(),
            Some("t".into()),
            Duration::from_secs(1),
            &|_, _| Answer::Allow,
            &|title, text| notes.borrow_mut().push((title.to_owned(), text.to_owned())),
        );
        assert_eq!(out, None);
        assert_eq!(
            notes.borrow()[0],
            ("Codex finished".to_owned(), "Done".to_owned())
        );
    }
}
