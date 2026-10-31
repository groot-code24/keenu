// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Platform-independent core of Keenu: no D-Bus, GTK, Win32, AppKit or network, so every rule that decides
//! what the island shows can be unit tested in isolation.

pub mod app_usage;
pub mod calendar;
pub mod clipboard;
pub mod file_shelf;
pub mod json_file;
pub mod lrc;
pub mod lyrics;
pub mod migration;
pub mod notifications;
pub mod paths;
pub mod pomodoro;
pub mod redact;
pub mod release;
pub mod reminders;
pub mod settings;
pub mod shelf;
pub mod speech;
pub mod todo;
pub mod track;
pub mod usage_log;
pub mod voice;
