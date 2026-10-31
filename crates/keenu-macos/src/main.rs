// SPDX-License-Identifier: GPL-3.0-or-later

//! Keenu's macOS host. It has been type-checked for both Mac architectures but has never run on a Mac: treat
//! everything here as a starting point, and see `docs/platform-support.md`.

#[cfg(target_os = "macos")]
mod adapters;
#[cfg(target_os = "macos")]
mod host;

#[cfg(target_os = "macos")]
fn main() {
    use std::sync::Arc;

    use keenu_app::Platform;
    use keenu_app::data::Paths;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = Paths::system();
    if args.first().is_some_and(|arg| arg == "--agent-hook") {
        // No pipe to the island on macOS yet: the agent asks in its own terminal, as it does with no Keenu.
        return;
    }
    keenu_app::logger::init(&paths.root);
    let platform = Platform {
        media: Arc::new(adapters::Media),
        notifier: Arc::new(adapters::Notifier),
        clipboard: Arc::new(adapters::Clipboard),
        shelf: Arc::new(adapters::Shell),
        voice: Arc::new(adapters::Voice),
        monitors: Arc::new(adapters::Displays),
        usage: Arc::new(adapters::Foreground),
        lifecycle: Arc::new(adapters::Lifecycle),
        updates: Arc::new(adapters::Updates),
        credentials: Arc::from(keenu_security::credentials::system_store()),
        open_url: adapters::open_url,
        play_wav: adapters::play_wav,
    };
    if let Some(result) = keenu_app::cli::run(&args, &platform, &paths) {
        if let Err(why) = result {
            eprintln!("{why}");
            std::process::exit(2);
        }
        return;
    }
    host::run(platform, paths);
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This is Keenu's macOS host, and only runs on a Mac.");
    std::process::exit(1);
}
