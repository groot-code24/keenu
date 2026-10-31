// SPDX-License-Identifier: GPL-3.0-or-later

//! The assistant's guard: which actions run by themselves, which wait for the user, and which never run, decided here
//! in plain code and never by the AI. Everything it lets through or stops goes in the action log.

pub mod level;
pub mod log;

pub use level::{Decision, Level, Policy, describe, level_of};
pub use log::{ActionLog, Entry, Outcome};
