// SPDX-License-Identifier: GPL-3.0-or-later

//! What Keenu does on hosts that draw the island themselves, with the operating system behind the
//! interfaces of `keenu-platform`. A host owns the window and the message loop and calls in here; this
//! crate never touches a native API, so all of it is tested against a mock platform on any system.

pub mod app;
pub mod assistant;
pub mod cli;
pub mod data;
pub mod hook;
pub mod logger;

pub use app::{App, AppHandle, Effect, Key, Platform, Waker};
