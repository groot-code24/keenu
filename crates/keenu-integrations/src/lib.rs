// SPDX-License-Identifier: GPL-3.0-or-later

//! Keenu's connections to what is outside it: calendar servers, OAuth, lyrics, speech APIs, coding agents
//! and the release manifest. Everything here works the same on every operating system.

pub mod agents;
pub mod calendar;
pub mod http;
pub mod ical;
pub mod lyrics;
pub mod oauth;
pub mod updater;
pub mod voice;
