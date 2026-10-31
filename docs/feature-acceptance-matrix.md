# Feature acceptance matrix

Every check, what it needs, and what actually happened. A release needs each one that applies to be **PASS**, or
listed in its notes with a reason. Successful compilation counts for nothing here.

**Status:** PASS, FAIL, BLOCKED (needs something outside the code), NOT RUN, NOT SUPPORTED.

**Mock** in the steps means a stand-in was used and the real service was not. Those rows prove the logic, not the
integration.

## Environments

| Id | OS and version | Architecture | Desktop | Notes |
|---|---|---|---|---|
| W1 | Windows 11 Home 10.0.26300 | x64 | Windows shell, one display, 1920x1080 at 150% | Program built with mingw-w64 in Docker; Python 3.10; run 2026-10-31 |
| L1 | Debian 13 container (`rust:1-trixie`) | x64 | None | Logic tests only |
| L2 | Debian 13 container | x64 | Headless GNOME Shell 48.7, 1280x720 virtual display | No systemd, no real session |
| L3 | Fedora 43 container | x64 | Headless GNOME Shell 49.10, 1280x720 virtual display | No systemd, no real session |
| M1 | None: no Mac was available | | | Type-checked and linted on W1 for the macOS targets; never linked or run |
| CI | GitHub-hosted runners | Various | Various | **Workflows have not run yet**: the repository has no remote |

## How current each result is

Testing found bugs late, so not every result is from the very last code. In the final code:

- **Run on it:** all of W1's unit tests, smoke test, installer test and real CalDAV server; formatting; clippy on Windows,
  Linux and macOS; the Linux tarball on L2; the one-command installer on L2.
- **From the build before the last fixes:** the Linux unit tests' run (LX-1), and the `.deb` and `.rpm` on L2 and L3
  (LX-3). The changes after them were the XML parser's version and depth limit, and not reusing HTTP connections. Clippy
  did compile the Linux tests on the final code. Docker Desktop stopped before these could run again.
- **From an earlier build:** the rows marked with † below, on the real clipboard. The final run left them out on purpose,
  because they lose anything on the clipboard that is not text, and someone may have been using the computer. The code
  they cover changed only in how it loops over pixels, and its unit tests (CL-7, CL-8) passed on the final code.

## Identity and data

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| ID-1 | W1 | Release build | Read `keenu.exe`'s version information | ProductName Keenu | `Keenu 2.0.0-alpha.1` | Run output | PASS |
| ID-2 | W1, L1 | Source tree | Search for old names outside the kept identifiers | None | None left in code or docs, apart from CHANGELOG's history | [Branding](branding-migration.md) | PASS |
| MI-1 | W1, L1 | Temporary folders | `keenu_core::migration` unit tests: copy, never overwrite, once only, partial file, move bulk, roll back | All hold | 7 passed on both | `cargo test -p keenu-core migration` | PASS |
| MI-2 | W1 | A synthetic `neeru-data.json` | `keenu_app::data` unit tests | Becomes Keenu's files; original stays; once only; never over newer data; damaged file reported | Passed | `cargo test -p keenu-app data` | PASS |
| MI-3 | | A real Isle install on GNOME | Install Keenu over it; check data, settings, hooks | All carried over | | | NOT RUN |

## Media and lyrics

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| ME-1 | W1 | No other session | Play silence through `Windows.Media.Playback.MediaPlayer`; `--media-status` | Session found, with its title and artist, playing | `Keenu Test Tone`, `The Smoke Test`, playing. On the final code, as are ME-2 and ME-3 | Probe output | PASS |
| ME-2 | W1 | As ME-1 | `--media-playpause` twice | Paused, then playing | `playing:false`, then `true` | Probe output | PASS |
| ME-3 | W1 | As ME-1 | Stop the player; `--media-status` | Session gone, no error | `[]` | Probe output | PASS |
| ME-4 | W1 | No session | Command a session that does not exist | Error, no crash | Error | `media::tests` | PASS |
| ME-5 | | Spotify and a browser | Next, previous, seek, cover art, switching sessions, track change | All work; lyrics follow | | | NOT RUN |
| ME-6 | L1 | Mock players | The daemon's MPRIS tests | Hold | 207 passed in the daemon | `cargo test -p keenu-daemon` | PASS |
| ME-7 | | macOS with Music and Spotify | Everything above | | | | NOT RUN |
| LY-1 | W1, L1 | | `keenu_core::lyrics` tests: position, seeking, encodings, invalid files, unsafe names | All hold | Passed | `cargo test -p keenu-core lyrics` | PASS |
| LY-2 | W1 | A real `.lrc` | `--lyrics file 2000` | The line at 2 s | `First line` | Smoke test | PASS |
| LY-3 | W1 | A file named for the session's track | App test | Found, follows position; lyrics from an old track ignored | Passed | `keenu_app::app::tests` | PASS |
| LY-4 | | Network | Real LRCLIB lookup on Windows | Lyrics appear | | | NOT RUN |

## Notifications

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| NO-1 | W1 | Notifications on | `--notify` | Windows accepts it, exit 0 | Accepted. Display not confirmed by the test | Smoke test | PASS |
| NO-2 | W1 | | Hostile text | Escaped; Windows' own XML parser accepts it | Passed | `notify::tests` | PASS |
| NO-3 | W1, L1 | Mock platform | Queue, priority, dedupe, retry, give up | All hold | Passed | `notifications::tests`, `app::tests` | PASS |
| NO-4 | | A person | A toast appears under Keenu's name; turning off notifications shows in Settings | | | | NOT RUN |
| NO-5 | | | Toast buttons come back to the program | Nothing uses them yet | | | NOT RUN |

## Calendar

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| CA-1 | W1 | Radicale 3.5, real, seeded | `--caldav-test`: find calendars, full sync | 1 calendar, 1 event, sync token | As expected | Log below | PASS |
| CA-2 | W1 | As CA-1 | Create, then incremental sync | Created with an ETag; 1 changed, 0 removed | As expected | Log below | PASS |
| CA-3 | W1 | As CA-1 | Update with the right ETag | New ETag | As expected | Log below | PASS |
| CA-4 | W1 | As CA-1 | Update with the old ETag | Refused as a conflict | `true` | Log below | PASS |
| CA-5 | W1 | As CA-1 | Delete, then incremental sync | Reported removed | `true`, after fixing a real bug this found | Log below | PASS |
| CA-5a | W1 | As CA-1, on this computer, no Docker in the path | CA-1 to CA-5, 100 times in a row | No failures | 0 of 100, after fixing a real bug: it was 4 of 30 before. See the threat model | Run output | PASS |
| CA-6 | W1, L1 | Mock server | CalDAV, Google, Graph request and answer tests | Hold | Passed; 113 in the crate | `keenu_integrations::calendar` | PASS |
| CA-7 | W1, L1 | | Repeats, time zones, daylight saving, changed instances, patching | Hold | Passed | `keenu_integrations::ical` | PASS |
| CA-8 | W1, L1 | Mock provider | Offline queue, expired token, conflict, busy, removed calendar | Hold | Passed | `calendar::engine` | PASS |
| CA-9 | | Google client ID | Real sign-in, sync, create, change, delete, revoke | | | | BLOCKED |
| CA-10 | | Microsoft client ID | Real sign-in, sync, create, change, delete | | | | BLOCKED |
| CA-11 | | Nextcloud, Fastmail or iCloud | As CA-1 to CA-5 | Other servers differ | | | NOT RUN |
| CA-12 | | A person | Add a CalDAV account in the UI | Listed, synced, removable | | | NOT RUN |
| OA-1 | W1, L1 | Mock server | PKCE, `state`, refresh, revoked, expired | Hold; the RFC's example matches | Passed | `oauth::tests`, `integrity::tests` | PASS |

The CA-1 to CA-5 run:

```text
1 calendar(s)
Work & life: 5 event(s), token true
created /tester/work/ce7cb319001b5b4e7a3393c34c8b1576.ics etag "5cc39024..."
incremental: 1 changed, 0 removed
updated, etag "54685a6f..."
stale update refused as conflict: true
deleted; incremental reports removed: true
```

## Clipboard

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| CL-1 † | W1 | Real clipboard, history off | Copy, then list | Nothing kept | Nothing | Smoke test | PASS |
| CL-2 | W1 | As CL-1, mock | The platform's read is never called while off | 0 reads | 0 | `app::tests` | PASS |
| CL-3 † | W1 | History on | Copy | Kept | Kept | Smoke test | PASS |
| CL-4 † | W1 | History on | Copy with `ExcludeClipboardContentFromMonitorProcessing` | Not kept | Not kept | Smoke test | PASS |
| CL-5 † | W1 | History on | Copy with `CanIncludeInClipboardHistory` = 0 | Not kept | Not kept | Smoke test | PASS |
| CL-6 † | W1 | An entry | Copy it back, clear | On the clipboard; empty | As expected | Smoke test | PASS |
| CL-7 | W1, L1 | | Search, pin, delete, limits, retention, pause | Hold | Passed | `app::tests`, `clipboard::tests` | PASS |
| CL-8 | W1 | | Bitmap to PNG and back; malformed ones | Right colours; refused | Passed | `clipboard::tests` | PASS |
| CL-9 | | A person | Rich text from Word, a screenshot, files from Explorer | Kept and pasted back | | | NOT RUN |

## File shelf

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| FS-1 | W1, L1 | Real files | Add, duplicate, missing and back, pin, clear, capacity, unsafe paths | Hold; files untouched | Passed | `file_shelf::tests` | PASS |
| FS-2 | W1 | | Thumbnail of a real file; absent file | A real PNG or nothing | Passed | `shell::tests` | PASS |
| FS-3 | W1 | | Reveal or open what is not there | Error | Error | `shell::tests` | PASS |
| FS-4 | | A person | Drag from Explorer, open, reveal | | | | NOT RUN |

## Voice

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| VO-1 | W1, L1 | Mock platform | Off: cannot start. On: nothing opens until asked. Turning off stops it. Cancel | All hold | Passed | `app::tests` | PASS |
| VO-2 | W1, L1 | Mock server | Transcribe, answer, refused key, insecure address | Hold; key not repeated | Passed | `voice::tests` | PASS |
| VO-3 | W1 | Real audio APIs | List microphones | Default first; nothing recording | Passed | `voice::tests` | PASS |
| VO-4 | | A person, a microphone | Record, stop, cancel, unplug; refuse permission | | | | NOT RUN |
| VO-5 | | A speech pack | Local listening and speaking | | | | NOT RUN |
| VO-6 | | An endpoint and key | Real transcription and speech | | | | NOT RUN |

## Coding agents

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| AG-1 | W1 | Temporary agent folders | `--agent-install` | Both files, with ten minutes on the permission hook; says Codex needs `/hooks` | As expected | Smoke test | PASS |
| AG-2 | W1 | Settings that are not JSON | `--agent-install` | Refused, left alone | As expected | Smoke test | PASS |
| AG-3 | W1 | Existing settings | Install and remove | Back to exactly what it was | As expected | `agents::tests` | PASS |
| AG-4 | W1 | No island | Hook with Claude Code's documented payload | Prints nothing | Nothing | Smoke test | PASS |
| AG-5 | W1 | Island running | Real hook, real pipe, real keys: Deny | Documented deny | As expected | Smoke test | PASS |
| AG-6 | W1 | As AG-5 | Ask in terminal | Prints nothing | Nothing | Smoke test | PASS |
| AG-7 | W1 | As AG-5 | Allow | Documented allow, only documented fields | As expected | Smoke test | PASS |
| AG-8 | W1 | As AG-5 | Enter with nothing chosen | Answers nothing | Still waiting | Smoke test | PASS |
| AG-9 | W1 | As AG-5 | Wrong token; garbage | Prints nothing | Nothing | Smoke test | PASS |
| AG-10 | W1 | Real pipe | Timeout; client gone | No decision; noticed | As expected | `ipc::tests` | PASS |
| AG-11 | W1, L1 | | Replay, double answer, Allow too soon, focus reset, Escape, shutdown | Never an unintended allow | Passed | `app::tests` | PASS |
| AG-12 | W1, L1 | | Disguised commands, long commands, schema | Visible; marked; documented form | Passed | `protocol::tests` | PASS |
| AG-13 | | The real Claude Code CLI | A real permission request | Island asks; CLI obeys the answer | Not on this machine's PATH | | NOT RUN |
| AG-14 | | The real Codex CLI, hook trusted | A real permission request | As AG-13 | Not on this machine's PATH | | NOT RUN |

## Usage

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| US-1 | W1 | Off | Workers run | Foreground never asked | 0 asks | `app::tests` | PASS |
| US-2 | W1, L1 | | A stored session's fields | Only app and times | Exactly those | `usage_log::tests` | PASS |
| US-3 | W1, L1 | | Summary, categories, retention, delete, CSV with a hostile name | Hold; formula defused | Passed | `usage_log::tests` | PASS |
| US-4 | W1 | Real desktop | Read the foreground app | A bare file name | Passed | `system::tests` | PASS |
| US-5 | | A person, an hour | Real sessions; idle; lock | Add up, and pause | | | NOT RUN |

## Updates

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| UP-1 | W1, L1 | | Good, changed, forged, unsigned and unsafe manifests | Only the good one is accepted | Passed | `updater::tests`, `release::tests` | PASS |
| UP-2 | W1, L1 | Mock server | Download good, tampered, oversized, insecure | Only the good one keeps its name | Passed | `updater::tests` | PASS |
| UP-3 | W1 | An unsigned file | `WinVerifyTrust`, thumbprint | Refused | Refused | `system::tests` | PASS |
| UP-4 | W1 | Build without settings | `availability`; `--capabilities` | Says it cannot update | As expected | `system::tests` | PASS |
| UP-5 | | A code-signing certificate | Real update from an older MSI; restart; wrong signer refused | | | | BLOCKED |
| UP-6 | | Developer ID | macOS | Not implemented | | | NOT SUPPORTED |

## Desktop lifecycle (Windows)

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| LC-1 | W1 | | Start | Window, visible, small | As expected | Smoke test | PASS |
| LC-2 | W1 | Running | Start again | Second leaves; first opens | As expected | Smoke test | PASS |
| LC-3 | W1 | Running | Send display, settings, resume, lock and unlock messages, and TaskbarCreated | Survives | Survives. **Simulated** | Smoke test | PASS |
| LC-4 | W1 | Running | WM_CLOSE | Exits within 5 s, saved | As expected, after fixing a real hang this found | Smoke test | PASS |
| LC-5 | W1, L1 | | Negative coordinates, taskbar on top, a display gone | Right position | Passed. **Logic only** | `system::tests` | PASS |
| LC-6 | W1 | Real registry | Start at login, on and off | Set and cleared | As expected | `shell::tests` | PASS |
| LC-7 | W1 | | Tab, Shift+Tab, Enter, Escape | Reach and press buttons | Passed | `app::tests`, smoke test | PASS |
| LC-8 | | Two displays | Mixed scale, negative coordinates, hotplug | | Only one display here | | NOT RUN |
| LC-9 | | A person | Real sleep and resume; really killing Explorer; fullscreen game; tray menu; hover | | | | NOT RUN |
| LC-10 | | Windows 10 | All of the above | | | | NOT RUN |
| LC-11 | | Narrator | Screen reader | Not exposed yet | | | NOT RUN |

## Installers

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| IN-1 | W1 | Not installed | Install the older MSI silently, without admin | Installed, shortcut, one Apps entry | As expected | `installer_test.py` | PASS |
| IN-2 | W1 | IN-1, with data | Upgrade MSI | New version, one entry, data kept | As expected | `installer_test.py` | PASS |
| IN-3 | W1 | IN-2 | Older MSI | Refused, new version stays | As expected | `installer_test.py` | PASS |
| IN-4 | W1 | IN-2 | Uninstall | All gone but the data; PATH as it was | As expected | `installer_test.py` | PASS |
| IN-5 | W1 | Not installed | Setup program `/S`, then uninstall | Installed; removed; data kept | As expected | `installer_test.py` | PASS |
| IN-6 | W1 | Server on this computer | `install.ps1` | Installed, checksum verified, says unsigned | As expected | `installer_test.py` | PASS |
| IN-7 | W1 | As IN-6 | Run again; `-Uninstall` | Keeps data; removed | As expected, after fixing a real false "removed" | `installer_test.py` | PASS |
| IN-8 | W1 | As IN-6 | A changed file on the server | Refused, not installed | As expected | `installer_test.py` | PASS |
| IN-9 | W1 | | No address; insecure address; missing list | Clear error, nothing changed | As expected | `installer_test.py` | PASS |
| IN-10 | | | Portable ZIP through `install.ps1` | | Tested only as a listed file; MSI is preferred | | NOT RUN |
| IN-10a | L2 | Server in the container | `scripts/install/install.sh`: as root, no address, insecure address, missing list, prerelease not asked for, changed package; install, again, `--uninstall` | Refused with a reason; installed, checksum verified; data kept; removed | 20 of 20, after fixing a real bug: `--uninstall` could not find the command | Run output | PASS |
| IN-11 | | A real domain | `irm ... \| iex`, as people will | | | | BLOCKED |
| IN-12 | | Several antivirus products | Installers and scripts | Not flagged | One product blocked a PowerShell test script; `install.ps1` ran | [Notes](release-engineering.md#testing-a-package-by-hand) | NOT RUN |

## Website

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| WE-1 | W1 | Node 24 | `choose.test.js`: detection, recommendation, nothing for this system, only `https://` links | Hold | 6 of 6 | `tests/website` | PASS |
| WE-2 | | A browser and a manifest | Load the page | | | | NOT RUN |

## Release automation

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| RE-1 | W1, L1 | | `keenu-release` tests: lists only what exists, real checksums, signing as stated | Hold | Passed | `keenu-release` tests | PASS |
| RE-1a | W1 | | The workflows and PowerShell scripts parse; shellcheck on the shell scripts | Clean | Clean. **Parsing only: no workflow has run** | Run output | PASS |
| RE-2 | CI | A repository on GitHub | Each workflow | | No remote yet | | BLOCKED |
| RE-3 | CI | A test tag | A full release, stopped by a failure, and completed | | | | BLOCKED |

## macOS

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| MC-1 | M1 | | `cargo clippy --target aarch64-apple-darwin` and `x86_64-apple-darwin`, `-D warnings` | Type-checks, no warnings | Both clean, after fixing the 2 errors its first check found. **Not linked, not run** | Run output | PASS |
| MC-2 | | A Mac | Starts, island, menu bar, every feature, permissions, Spaces, fullscreen, sleep, Retina | | No Mac | | NOT RUN |
| MC-3 | | A Mac | `build-app.sh`, DMG | | No Mac | | NOT RUN |
| MC-4 | | Developer ID | Sign and notarize | | | | BLOCKED |

## Linux

| ID | Env | Preconditions | Steps | Expected | Actual | Evidence | Status |
|---|---|---|---|---|---|---|---|
| LX-1 | L1 | | `./test.sh`: formatting, all shared crates and the daemon | Pass | core 125, daemon 207 (5 ignored), integrations 109, app 53, UI 24, security 10, release 6. **Before the last fixes**, which add 5 integrations tests | Build log | PASS |
| LX-1a | L1 | | Clippy on the shared crates, the daemon and the shelf, with their tests, `-D warnings`, on the final code | Clean | Clean | Build log | PASS |
| LX-2 | W1 | Node 24 | The extension's logic tests | Pass | 39 of 39 | `tests/extension` | PASS |
| LX-3a | L2 | A test build: see LX-6 | Tarball's `install.sh`, then GNOME Shell | Extension active, no JS errors, `keenud` on the bus | State 1, no JS errors, `keenud` there in 3 s. On the final code | `scripts/ci/smoke-test.sh` | PASS |
| LX-3b | L2 | A test build: see LX-6 | `.deb`, then GNOME Shell | As LX-3a | As expected, **on the build before the last fixes**. The final run was cut off when Docker Desktop stopped | `scripts/ci/smoke-test.sh` | PASS |
| LX-3c | L3 | A test build: see LX-6 | `.rpm`, then GNOME Shell | As LX-3a | As expected, **on the build before the last fixes** | `scripts/ci/smoke-test.sh` | PASS |
| LX-4 | | A real GNOME desktop | Everything in the [Linux guide](linux.md), since the rename | | | | NOT RUN |
| LX-5 | | Ubuntu, a real Fedora desktop, KDE, X11 | | | KDE and others have no island | | NOT RUN |
| LX-6 | | At least 4 GB for Docker or the build | `scripts/package.sh` as it is, with speech on the graphics card | Builds | whisper.cpp's Vulkan shaders ran out of memory in 3.5 GB, with optimisation lowered too. The packages above were therefore built with `keenu-whisper --no-default-features`, on the processor only, from a scratch copy. **The packages a release would publish have not been built** | Build log | BLOCKED |
| LX-7 | L1 | | `cargo deny check` | Passes | Licences, bans and sources passed. Advisories failed on `quick-xml`, since upgraded to a fixed version. **Not run again since** | Build log | NOT RUN |
