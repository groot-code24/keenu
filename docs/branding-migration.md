# The move from Isle and Neeru to Keenu

This project began as [Isle](https://github.com/zickrian/isle), was renamed Neeru, and is now Keenu. Everything a
person sees says Keenu. Some internal identifiers keep the old name, on purpose.

## What changed

| | From | To |
|---|---|---|
| Product name | Neeru | Keenu |
| Crates | `isle-core`, `isle-daemon`, `isle-shelf`, `isle-whisper`, `neeru-windows` | `keenu-core`, `keenu-daemon`, `keenu-shelf`, `keenu-whisper`, `keenu-windows` |
| Programs | `isled`, `isle-shelf`, `isle-whisper`, `isle-setup-voice`, `isle-charge-limit`, `isle-uninstall`; `neeru.exe` | `keenud` (and `keenu`, a link to it), `keenu-shelf`, `keenu-whisper`, `keenu-setup-voice`, `keenu-charge-limit`, `keenu-uninstall`; `keenu.exe` |
| Service | `isled.service` | `keenud.service` |
| Linux data | `~/.local/share/isle`, `~/.cache/isle` | `~/.local/share/keenu`, `~/.cache/keenu` |
| Windows data | `%LOCALAPPDATA%\Neeru` | `%LOCALAPPDATA%\Keenu` |
| Environment | `ISLE_*`, `NEERU_*` | `KEENU_*` |
| Windows identity | `Neeru.Desktop` | `Keenu.Desktop` |
| Packages | `isle`, `isle-bin`, `Neeru-Setup-x64.msi` | `keenu`, `keenu-bin`, `Keenu-<version>-windows-x64.msi` |

The Windows MSI keeps its UpgradeCode, which is what would let a new version replace an old one.

## What stays, and why

Renaming these would break installations or lose settings:

| Identifier | Why it stays |
|---|---|
| D-Bus names `io.github.zickrian.isle.*` | The extension and the service find each other by them, and each user's service file names them |
| Extension id `isle@zickrian.github.io` | GNOME stores which extensions are on by this id, and its install folder is named after it |
| GSettings schema `org.gnome.shell.extensions.isle` | Each user's settings are stored under this path in dconf |
| Calendar id `isle` and `super-v` value `isle` | Saved values in those settings |
| CSS classes and icon names `isle-*` | Internal to the extension, and part of its theme's selectors |
| `Copyright (C) 2026 zickrian` | The GPL requires copyright notices to stay |

They are never shown to people, and they do not mean the original author publishes or supports Keenu: see
[NOTICE](../NOTICE).

Two deliberately kept names were nearly broken by the rename, and are worth knowing about when changing either side:
the extension's command allow-list (`gnome-extension/src/actionRules.js`) must agree with `keenud`'s
`--unit=keenu-run-*` and argv0 in `crates/keenu-daemon/src/assistant/tasks.rs`.

## How data moves

On first start, Keenu looks for an earlier version's folder and brings its data over (`keenu_core::migration`).

- **Nothing is lost.** The old folder is copied, not moved, and stays exactly as it was.
- **Nothing is overwritten.** A file the new folder already has is kept, and the fact logged.
- **It is complete or it runs again.** Each file appears whole or not at all. A marker, `.migrated-from-<name>`, is
  written only after everything is across; a failed run leaves no marker and tries again next start.
- **It runs once.** With the marker there, what you delete in the new folder stays deleted.
- **It says what it did**, in `migration.log` in the data folder.

The exception is the Linux speech models under `models/`. They can be several gigabytes and can be downloaded again,
so they are moved rather than copied.

On Windows, an old `neeru-data.json` is also converted into Keenu's files: tasks, undismissed reminders, clipboard
history, shelf entries (those still on disk) and usage totals, along with the switches that were on. It is converted
once, and never over a file Keenu already has. A damaged file is reported and left alone.

## Going back

```sh
keenu --migration-status     # what was brought over
keenu --migration-rollback   # sets Keenu's data folder aside and migrates again next start
```

Rolling back deletes nothing: the folder is renamed `<name>.rolled-back-<time>`.

To return to an earlier version entirely, install it: its folder is untouched. The exception is the Linux speech
models, which it would download again.

## Linux upgrades

`install.sh` stops and removes `isled.service` and the old programs. Hooks in Claude Code's and Codex's settings are
recognised by `--agent-hook`, whichever program name they hold, and re-pointed at `keenud` the next time they are
turned on or checked. The extension's id and settings do not change, so it stays on with your choices.
