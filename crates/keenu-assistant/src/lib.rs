// SPDX-License-Identifier: GPL-3.0-or-later

//! Keenu's assistant, shared by every system: the agent loop, the AIs behind it (their own programs with the user's
//! own sign-in, or an API), and its file tools. Each host adds its own desktop actions behind `agent::Toolbox`.

pub mod agent;
pub mod agy;
pub mod answer;
pub mod brain;
pub mod chat;
pub mod child;
pub mod claude;
pub mod codex;
pub mod files;
pub mod grok;
pub mod private;
pub mod sign_in;
pub mod uri;

/// `text` with every run of quotes cut to one, so no number of them adds up to a closing \"\"\".
pub fn soften(text: &str) -> String {
    let mut softened = String::with_capacity(text.len());
    for c in text.chars() {
        if c != '"' || !softened.ends_with('"') {
            softened.push(c);
        }
    }
    softened
}
