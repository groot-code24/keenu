# Data retention

What Keenu keeps, for how long, and how it goes. All of it is on your computer: there is no server.

The folder is `%LOCALAPPDATA%\Keenu` on Windows, `~/Library/Application Support/Keenu` on macOS and
`~/.local/share/keenu` on Linux. `KEENU_DATA_DIR` chooses another.

| Data | File | Kept for | Removed by |
|---|---|---|---|
| Clipboard history | `clipboard.json`, `clips/<hash>.png` | `clipboard.retention_days`, default 30; the newest `clipboard.max_items` unpinned, default 100. Pinned items until removed | Hourly pruning, Delete, Clear, Delete my data |
| App usage | `usage.json` | `usage.retention_days`, default 30 | Hourly pruning, Delete, Delete all, Delete my data |
| File shelf | `shelf.json` | Until removed; the oldest unpinned goes beyond 100. A missing file is marked, not dropped, as a drive may come back | Remove, Clear, Delete my data |
| Calendar copies | `calendars/<account>.json` | While the account is connected | Removing the account, Delete my data |
| Tasks | `todo.json` | Until deleted | `--todo-*`, Delete my data |
| Reminders | `reminders.json` | Until shown | Showing them, `--reminder-dismiss`, Delete my data |
| Settings | `settings.json` | Until reset | Reset settings (keeps calendar accounts); never by Delete my data |
| Agent token | `agent-token` | Replaced each time Keenu starts | Each start |
| Log | `keenu.log`, `keenu.log.old` | About 2 MB in all | Replaced as it grows |
| Migration record | `migration.log`, `.migrated-from-*` | Indefinitely, as a record | By hand |
| Earlier versions' data | `%LOCALAPPDATA%\Neeru`, `~/.local/share/isle` | Never touched by Keenu | By hand |
| Lyrics and cover art | `cache/` or `~/.cache/keenu` | Newest 500 songs on Linux | Pruning; safe to delete at any time |
| Downloaded updates | `cache/updates/` | Until installed, or deleted if a check fails | |

**Secrets** are not in that folder at all. They are in the system's credential store under the name `Keenu`:

| Key | What | Removed by |
|---|---|---|
| `calendar/<account>` | CalDAV password or OAuth tokens | Removing the account, Delete my data |
| `voice/api-key` | Voice endpoint's key | Clearing the endpoint, Delete my data |

## Settings

Retention can be changed in `settings.json` while Keenu is closed. Values outside the allowed range are brought back
into it: 1 to 365 days for the clipboard, 1 to 730 for usage, and 1 to 1000 clipboard items.

## Exporting

**Settings → Export data**, or `keenu --export-data <file>`, writes one JSON file with settings, clipboard, shelf,
usage, tasks and reminders. It holds no secrets. **Usage → Export CSV**, or `keenu --usage-export <file>`, writes a
spreadsheet. A field that begins like a formula is defused, so opening it cannot run anything.

## Uninstalling

Uninstalling removes the program and Keenu's hooks from the agents' settings, but leaves your data, in case you come
back. Use **Settings → Delete my data** first, or delete the folder afterwards.
