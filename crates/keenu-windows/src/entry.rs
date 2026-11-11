// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the program starts: a command, an agent's hook, or the island.

use std::io::Read;
use std::sync::Arc;

use keenu_app::Platform;
use keenu_app::data::Paths;
use keenu_platform::{NativeNotification, NotificationProvider};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows_core::{HSTRING, PCWSTR};

use crate::clipboard::Clipboard;
use crate::media::Media;
use crate::notify::Notifier;
use crate::shell::Shell;
use crate::system::{Displays, Foreground, Updates};
use crate::voice::{Voice, play_wav};

/// Opens a web page. Only ever given addresses Keenu made itself: a sign-in page.
fn open_url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("Only secure addresses are opened.".into());
    }
    let (url, verb) = (HSTRING::from(url), HSTRING::from("open"));
    // SAFETY: both strings end in a NUL.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(url.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err("Windows could not open the browser.".into())
    }
}

fn platform() -> Platform {
    Platform {
        media: Arc::new(Media),
        notifier: Arc::new(Notifier::new(None)),
        clipboard: Arc::new(Clipboard),
        shelf: Arc::new(Shell),
        voice: Arc::new(Voice::default()),
        monitors: Arc::new(Displays),
        usage: Arc::new(Foreground),
        lifecycle: Arc::new(Shell),
        updates: Arc::new(Updates),
        credentials: Arc::from(keenu_security::credentials::system_store()),
        control: Arc::new(crate::control::AppControl),
        open_url,
        play_wav,
    }
}

/// What the program does. Returns its exit code.
pub fn main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = Paths::system();
    if args.first().is_some_and(|arg| arg == "--agent-hook") {
        return run_hook(args.get(1).map_or("", String::as_str), &paths);
    }
    if !args.is_empty() {
        // SAFETY: fails harmlessly when there is no console to attach to or output is already redirected.
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    if args.first().is_some_and(|arg| arg == "--render-sample") {
        let usage = "Usage: keenu --render-sample <page> <out.png> [scale]";
        let (Some(page), Some(out)) = (args.get(1), args.get(2)) else {
            eprintln!("{usage}");
            return 2;
        };
        let scale = args
            .get(3)
            .and_then(|scale| scale.parse().ok())
            .unwrap_or(1.0);
        return match crate::host::render_sample(page, std::path::Path::new(out), scale) {
            Ok(()) => 0,
            Err(why) => {
                eprintln!("{why}");
                1
            }
        };
    }
    keenu_app::logger::init(&paths.root);
    let platform = platform();
    if let Some(result) = keenu_app::cli::run(&args, &platform, &paths) {
        return match result {
            Ok(()) => 0,
            Err(why) => {
                eprintln!("{why}");
                2
            }
        };
    }
    match crate::host::run(platform, paths) {
        Ok(()) => 0,
        Err(why) => {
            eprintln!("Keenu could not start: {why}");
            1
        }
    }
}

/// `keenu --agent-hook <agent>`, which must always leave the agent able to go on.
fn run_hook(agent: &str, paths: &Paths) -> i32 {
    let token = std::fs::read_to_string(paths.agent_token()).ok();
    let notifier = Notifier::new(None);
    let mut input = std::io::stdin().lock();
    let output = keenu_app::hook::run(
        agent,
        &mut (&mut input as &mut dyn Read),
        token,
        keenu_app::cli::hook_timeout(paths),
        &crate::ipc::request,
        &|title, body| {
            let _ = notifier.show(&NativeNotification {
                tag: format!("agent-{agent}"),
                title: title.to_owned(),
                body: body.to_owned(),
                actions: Vec::new(),
            });
        },
    );
    if let Some(output) = output {
        println!("{output}");
    }
    0
}
