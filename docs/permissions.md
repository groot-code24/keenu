# Permissions

What Keenu asks of your system, when, and what happens if you say no. It runs as your user and never asks for
administrator rights, on any system.

## Windows

| What | When it is used | If refused or off |
|---|---|---|
| Notifications (Settings → System → Notifications) | When something notifies you | Settings says notifications are off; nothing else changes |
| Microphone (Settings → Privacy & security → Microphone, "Let desktop apps access your microphone") | Only while you record | The Voice page says so, and recording does not start |
| Clipboard | On a change, only while clipboard history is on | History stays empty |
| Foreground app | Every two seconds, only while app usage is on | No usage is recorded |
| Credential Manager | When you save a password, token or key | The account is not added; it is never saved elsewhere |
| Run at login (`HKCU\...\Run`) | Only if you turn on Start at login | |

Installing writes only under your user: `%LOCALAPPDATA%\Programs\Keenu`, `HKCU`, and your Start Menu.

## macOS

> The macOS host has not been run on a Mac yet, so what macOS actually prompts for has not been confirmed.

| What | When it is used |
|---|---|
| Automation of Music and Spotify (a system prompt the first time) | Reading what is playing, and its controls |
| Notifications | When something notifies you, under Script Editor's name until UserNotifications is implemented |

The microphone, clipboard monitoring and starting at login are not implemented on macOS yet.

## Linux

The GNOME extension and `keenud` run as your user, in your session.

| What | When it is used |
|---|---|
| Session D-Bus | Always: how the parts talk, and how music players are found |
| Microphone, through PipeWire | Only while you record a voice request |
| Files | Your own folders, when the assistant is asked to look, and the shelf |
| `sudo` | Never by itself. The assistant shows a command, and only you can run it |

## Coding agents

With hooks installed, each agent runs `keenu --agent-hook` on its own events.

| | Claude Code | Codex |
|---|---|---|
| File changed | `settings.json` in `CLAUDE_CONFIG_DIR` or `~/.claude` | `hooks.json` in `CODEX_HOME` or `~/.codex` |
| Your own settings there | Left as they were | Left as they were |
| Backup of the original | `<file>.keenu-backup`, once | `<file>.keenu-backup`, once |
| Extra step | None | **Codex runs a new hook only after you trust it**: open Codex and review it with `/hooks` |
| To remove | Settings, or `keenu --agent-uninstall` | The same |

A settings file that is not valid JSON is refused and left alone. Keenu's answers only allow or deny the one request:
they never add a permanent rule to the agent's settings.

## Network

See [PRIVACY.md](../PRIVACY.md#what-may-leave-your-computer). Windows Firewall may ask once, the first time you sign in
to Google or Microsoft, about the temporary listener on `127.0.0.1` that receives the browser's redirect. It listens
on this computer only, and is closed when sign-in ends.
