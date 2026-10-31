// SPDX-License-Identifier: GPL-3.0-or-later

// No console window of its own: the island is a desktop program. Commands attach to the one they were
// started from, and a hook's output goes down the pipe its agent gave it.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(keenu_windows::entry::main());
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "This is Keenu's Windows host. On Linux use keenud with the GNOME extension; on macOS, keenu-macos."
    );
    std::process::exit(1);
}
