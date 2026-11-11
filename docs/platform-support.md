# What works where

This page says what has actually been tested, on what. A feature is not listed as working because code for it exists.
[The acceptance matrix](feature-acceptance-matrix.md) has the individual checks.

| Word | Meaning |
|---|---|
| **Tested** | Run for real on that system, by a test or a person |
| **Mock-tested** | The logic is tested against a stand-in; never against the real service |
| **Compiles** | Type-checked or built only; never run |
| **Not yet** | Not implemented |
| **Not supported** | Cannot be done, for the reason given |

## Systems

| System | Status | What has been run |
|---|---|---|
| Windows 11, x64 | **Alpha: tested** | Windows 11 build 26300 at 150%, one display, built with mingw-w64 through Docker. The release's MSVC build: its tests, smoke test, installers and one-command install on GitHub's Windows Server |
| Windows 10 2004+, x64 | Declared, **not tested** | Same APIs; needs a run |
| Windows 11, ARM64 | **Compiles** in CI only: no download | Nothing |
| Linux, GNOME 46–51, x64 | **Alpha**: logic tested, packages installed | The release's packages and one-command install, in containers with a headless GNOME Shell on Ubuntu 24.04, Debian 13 and Fedora 43. Not yet used on a real desktop since the rename |
| Linux, GNOME 46–51, ARM64 | Built and tested in CI: **never installed** | Its unit tests on GitHub's arm64 runner |
| Linux, KDE Plasma and others | **No island yet** | `keenud` starts anywhere with a session bus, but the only UI is a GNOME Shell extension |
| macOS 11+, Apple Silicon and Intel | **Starts: nobody has used it** | On GitHub's macOS 15 machines, both architectures: unit tests, build, disk image, one-command install, stays running, removal. No person has seen the island on a Mac. Older than macOS 15 has not run |

Official Windows releases are built with MSVC by CI. The real desktop runs above used a mingw-w64 build of the same code.

## Features

| Feature | Windows | Linux with GNOME | macOS |
|---|---|---|---|
| Sessions, metadata, play/pause | Tested with a real session | Existing code, tested with mocks in the daemon's tests | Compiles: Music, Spotify |
| Next, previous, seek | Compiles against real APIs | As above | Compiles |
| Cover art | Compiles against real APIs; the test session had none | As above | Not yet |
| Several sessions | Mock-tested | As above | Mock-tested |
| Local LRC lyrics | Tested | Not on this edition: LRCLIB | Mock-tested |
| Online lyrics (LRCLIB) | Mock-tested; opt-in | Existing, opt-in | Mock-tested |
| Toast or notification shown | Tested accepted by Windows | The extension's own | Compiles |
| Notification action buttons | Compiles; nothing uses them yet | The extension's own | Not supported: needs a signed bundle |
| CalDAV | **Tested against a real server** (Radicale), 100 full rounds | Through GNOME's accounts instead | Compiles |
| Google Calendar | Mock-tested. **Blocked** on a client ID | Through GNOME's accounts instead | Mock-tested |
| Microsoft 365 | Mock-tested. **Blocked** on a client ID | Through GNOME's accounts instead | Mock-tested |
| Clipboard: text, privacy flags, copy back, clear | Tested on the real clipboard, on a build shortly before the final one | Existing | Not yet |
| Clipboard: rich text, images, files | Unit-tested conversions | Existing, without rich text | Not yet |
| Shelf: references, pins, missing files | Tested | Existing, smaller | Mock-tested |
| Shelf: drag and drop itself, thumbnails | Compiles; Explorer drag needs a person | Existing | Not yet |
| Microphone list, recording | Compiles; **needs a person with a microphone** | Existing | Not yet |
| Speaking, listening with the system | Compiles; needs a speech pack and a person | whisper.cpp | `say` compiles |
| OpenAI-compatible speech | Mock-tested | Not on this edition | Not reachable: no recording |
| Claude Code and Codex: hook install, schema, pipe, buttons | **Tested end to end** with the documented payload | Existing, with its own tests | Not yet: the hook prints nothing |
| Claude Code and Codex: the real CLIs | **Not run**: neither CLI was on this machine's PATH | Not run | |
| App usage | Tested as far as the record; real foreground reading tested | Existing | Compiles |
| Updates: manifest, checksum, signature logic | Tested | Notice only | Compiles |
| Updates: installing one | **Blocked** on a code-signing certificate | Not supported: use the package manager or `install.sh` | Not yet |
| Installer: install, upgrade, remove | **Tested**: MSI, setup, ZIP, one-command | Tarball, .deb, .rpm, one-command: see the matrix | DMG, one-command: **tested** by CI |
| Tray or menu bar | Tested as far as surviving Explorer's restart message | Not applicable | Compiles |
| One instance | Tested | Existing | Not yet |
| Sleep, resume, lock | Tested by simulated messages | Existing | Not yet |
| Displays: several, mixed scale, hotplug | Logic unit-tested, messages simulated. **Needs a person with two displays** | Existing | Compiles |
| Fullscreen | Compiles | Existing | Not yet |
| Screen reader | Not yet: regions have names, but they are not exposed to UI Automation | Through GNOME | Not yet |
| Keyboard | Tested by real key messages | Existing | Not yet |

### The assistant (branch `assistant-phases`, not released)

| Feature | Windows | Linux with GNOME | macOS |
|---|---|---|---|
| Listing windows, reading their controls, pressing, typing, keys, closing | Smoke test on GitHub's Windows drives a real window, password box included | keenud's own window commands; reading controls (AT-SPI) **not yet** | Compiles. Needs the Accessibility permission, which CI cannot give |
| Refusing password fields | Tested on a real password box | Not applicable yet | Compiles |
| Agent loop, approvals on the card, action log | Mock-tested with a fake AI | Existing on keenud, unchanged tests | Mock-tested |
| ChatGPT sign-in and answers | Mock-tested against a fake OpenAI. **Never tried with a real account** | Not yet | As Windows |
| An API (OpenAI-, Anthropic-compatible, local) | Mock-tested | Not yet | Mock-tested |
| Claude Code, Codex, Grok as the AI | **Not run** with the real programs | Existing | **Not run** |
| Spoken answers, and stopping them | Compiles; needs a person | Existing | Compiles; needs a person |
| Wake words: "Keenu", "Hey Keenu", "Hi/Hello/OK Keenu", "Good morning/afternoon/evening Keenu", with or without the request in the same breath | Compiles; needs a person with a microphone and a Windows speech language. How well Windows' engine hears "Keenu" is not known | Not yet | Not yet |
| Windows on the screen: minimize, maximize, halves, quarters, side by side | Smoke test puts a real window on the left half; tiling several is unit-tested only | keenud, existing | Compiles; needs Accessibility |
| Volume | Compiles; CI's machine has no sound device | keenud, existing | Compiles |
| Brightness | Compiles; works only where the display lets Windows set it (most laptop screens) | keenud, existing | **Not supported**: no public way |
| Wi-Fi and Bluetooth on and off | Compiles; needs a person. Windows may refuse desktop programs: then Keenu says so | keenud, existing | Wi-Fi compiles; Bluetooth **not supported** |
| Dark mode, Settings pages | Compiles | keenud, existing | Compiles |
| Do Not Disturb, night light, power mode | **Not supported**: no switch for other programs; their Settings page opens | keenud, existing | As Windows |
| Screenshot | Smoke test saves a real one | keenud, existing | Compiles |
| Lock, sleep, restart, shut down, sign out | Compiles; not run, on purpose. All but lock ask first | keenud: lock | Compiles |
| Typing where the cursor is | Smoke test, when the runner lets a window come to the front | keenud, existing | Compiles |
| Clipboard: copy, read when asked | Compiles; reading asks first | keenud | Compiles |
| System report, busiest programs, read-only checks | Smoke test runs each, and refuses a check that would show a Wi-Fi password | keenud, existing | Compiles |
| Shortcut: Ctrl+Alt+Space, Option+Space | Compiles; needs a person | Super+I, existing | Compiles; needs a person |
| Memory, projects, tone | Unit-tested, encryption included | Not yet | Unit-tested |
| Morning briefing, tasks | Unit-tested | Not yet | Unit-tested |
| MCP connectors | Mock-tested against a fake server over HTTP | Not yet | Mock-tested |
| Web search through xAI | Mock-tested. **Never tried with a real key** | Not yet | As Windows |
| Plans: steps, the AI's score and chance, issues, risks, improvements; the chance and failure rate from this computer's action log; a document per plan; steps marked off | Unit- and mock-tested | Unit-tested; the record has no outcomes yet, as keenud logs approvals only, so it says there is none | As Windows |
| Notes in Documents/Keenu Notes: add, list, search, read | Unit-tested | Unit-tested | Unit-tested |
| Reading web pages and following their links (https, never the local network) | HTML reading unit-tested; **not run against real sites in CI** | As Windows | As Windows |
| Seeing the screen: a picture to the AI connected (ChatGPT or an API), or a file for an AI in its own program | Picture unit-tested on CI's real desktop; sending it mock-tested. **Not tried with a real AI** | **Not yet**: keenud has no approval card for it | Compiles; needs Screen Recording, which CI cannot give |
| Clicking, scrolling and dragging at a place in that picture | Compiles; mapping unit-tested. **Needs a person** | **Not yet** | Compiles; needs Accessibility |
| WhatsApp and Telegram: open the chat with the message written in, then send | Compiles; mock-tested without the apps. **Not tried with the real apps** | **Not yet** | As Windows |
| Gmail and Outlook mail: search, read, draft, send (sending asks) | Message building unit-tested; **never tried with a real account**, which needs your own OAuth client (docs/calendar-setup.md) | **Not yet**: GNOME Online Accounts would be the way | As Windows |
| Keep the conversation going after a spoken answer | Unit-tested; needs a person | Existing listen-again only | As Windows |
| Window tiling | See above | keenud, existing | Halves, quarters, side by side, minimize: compiles; needs Accessibility |

## Limits that are the system's

- **macOS** has no public API for every app's now-playing information. Keenu can only ask apps with a scripting
  interface: Music and Spotify.
- **macOS** notification actions and identity need a signed, notarized bundle.
- **Windows** toasts from a program without a package show its registered name; a Start Menu entry helps some systems.
- **Windows** gives no way to read a clipboard entry an app has delay-rendered without asking that app to render it.
- **Linux** clipboard monitoring needs the session's own privileges: the extension does this inside GNOME Shell, which
  no separate program could do on Wayland. So it is GNOME-only.
- **AppImage** does not fit a GNOME Shell extension, which must be installed where the shell looks.

## Known gaps

- On Windows, `keenu` is a windowed program, so a terminal does not wait for it and its output can appear after the
  prompt. To capture it, pipe it: `keenu --version | Out-String`.
- Windows ships no automatic recovery if Direct2D cannot start, beyond saying so.
- Rich text and images on the clipboard have had unit tests but no run with a real app.
- With clipboard history on, the Windows island reads each copy on its window's thread. An app that copies with delayed
  rendering and then stops responding can stall the island until Windows gives up on that app.
- `keenu-macos` does not reach the agents' pipe, so agents ask in their terminals there.
- `Reminder` and task pages exist only on Linux; elsewhere they are reachable through the command line, and reminders
  still notify.
- Several features of the Linux edition are not on the others: the focus timer, calls, the AI assistant and its plan
  usage pages.
