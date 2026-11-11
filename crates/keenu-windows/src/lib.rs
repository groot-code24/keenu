// SPDX-License-Identifier: GPL-3.0-or-later

//! Keenu's Windows host: the native side of `keenu-app`, only built on Windows.

#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod control;
#[cfg(windows)]
pub mod entry;
#[cfg(windows)]
pub mod host;
#[cfg(windows)]
pub mod ipc;
#[cfg(windows)]
pub mod machine;
#[cfg(windows)]
pub mod media;
#[cfg(windows)]
pub mod notify;
#[cfg(windows)]
pub mod paint;
#[cfg(windows)]
pub mod sample;
#[cfg(windows)]
pub mod shell;
#[cfg(windows)]
pub mod system;
#[cfg(windows)]
mod uia;
#[cfg(windows)]
pub mod voice;
