# Implementation audit

The repository as received on 2026-10-31, and what was found. Baseline: Neeru 1.5.0, 330 files, no git history (the
folder was not a repository). What it became is in [the acceptance matrix](feature-acceptance-matrix.md).

## As received

| Part | State |
|---|---|
| `crates/isle-core` | Sound: pure logic, 78 tests passing |
| `crates/isle-daemon` | Sound, for Linux only: 216 tests passing, 5 ignored |
| `crates/isle-shelf`, `crates/isle-whisper` | Linux only; need GTK 4 and whisper.cpp |
| `gnome-extension/` | Mature: 56 modules, 39 logic tests passing |
| `crates/neeru-windows` | **Did not compile**: 8 errors |
| `scripts/windows/adapters/*.ps1` | Never run |
| `packaging/windows/Package.wxs` | Never built |
| macOS | **Nothing** |
| CI | Linux thorough; Windows defined, never able to succeed |
| Docs | Good for Linux, and candid that Windows was unverified |

## The Windows prototype

Its own documentation said it had never been run, and it could not have been built:

```
error[E0425]: cannot find function `InvalidateRect` in module `windows_sys::Win32::UI::WindowsAndMessaging` (5 times)
error[E0308]: mismatched types: `hwnd == 0`; a brush passed where an object was wanted
error[E0428]: the name `tests` is defined multiple times (store.rs)
```

Beyond that, reading it showed:

| Finding | Severity | Outcome |
|---|---|---|
| `update.ps1` defaulted to fetching installers from `zickrian/isle`, the upstream author's repository | High | Replaced: nothing is fetched unless a build says where |
| Each feature shelled out to PowerShell, with a 5 s poll of media through a new process | Medium | Replaced with native WinRT and Win32 |
| `--agent-hook` sent `message` when allowing, a field Codex fails on | Medium | Only documented fields are sent |
| Approvals by a modal dialog in the hook's own process, with no timeout, so an unanswered dialog held the agent for its whole ten minutes | Medium | Replaced by the island, with a timeout and arming |
| Clipboard written by `Set-Clipboard` with the value on the command line, visible to other programs | Medium | Native |
| `powershell.exe` and `msiexec` looked up on `PATH` | Medium | Full paths |
| Store saved by delete-then-rename: a crash between them lost all data | Medium | Atomic replace |
| ICS import ignored `TZID`, `VALUE=DATE`, folding and repeats | Medium | Full parser |
| CalDAV password read by prompt in a console the host did not have | Low | Credential store |
| Text-only UI in one font, no tray, no settings page, right-click to quit | Low | Rebuilt |
| Update installed by `Stop-Process -Force`, losing unsaved state | Low | Asks it to close |

Nothing of it could be reused except the MSI's UpgradeCode, the store's shape, for migrating from it, and its ideas.

## Other findings

| Finding | Severity | Outcome |
|---|---|---|
| Package metadata named the upstream author as maintainer, with a personal email | Medium | Placeholder until the real maintainer is set |
| Vulnerability reports directed to the upstream author | Medium | Directed to this repository |
| README asked for donations to the upstream author, as if for this fork | Low | Removed; attribution moved to [NOTICE](../NOTICE), with the author's page in a comment |
| The extension's Update fetched a script from `raw.githubusercontent.com/zickrian/isle` | High | Fails closed without a release address, after the placeholder itself turned out to be a risk: see the [threat model](threat-model.md) |
| No GPL notice of modification | Medium | [NOTICE](../NOTICE) |

Late checks then found these in this work itself, each fixed:

| Finding | Found by | Severity | Outcome |
|---|---|---|---|
| CalDAV requests failed at random: 4 runs in 30 | Repeating the real-server test | High | Connections are no longer reused; 0 runs in 100 |
| `quick-xml` 0.37 had two published denial-of-service advisories, RUSTSEC-2026-0194 and -0195 | `cargo deny` | Medium: Keenu uses neither affected path, but parses XML from servers | Upgraded to 0.42 |
| Deeply nested XML from a server could overflow the stack, ending the program | Reading the parser during that upgrade | Medium | Limited to 64 levels, with a test |
| `install.sh --uninstall` could not work: the command it runs was never installed | Running it | Medium | Installed again |
| `keenu-shelf` did not compile: the rename changed a file name it embeds, not the file | The first release build | High | Fixed. Debug tests had never built that crate |
| `keenu-macos` did not compile: 2 errors | Its first type-check | High | Fixed |
| Clippy's automatic fix changed the window icon's resource number from 1 to 2 | Reading the fix before keeping it | Low | Restored |
| `package-system.sh` wrote `amd64` into every package | Reading it | Medium: it would have broken ARM64 | Follows the tarball's architecture |

## Licence

GPL-3.0-or-later, unchanged. Dependencies are checked by `cargo deny` against `deny.toml`, whose list allows only licences
compatible with it. New dependencies, each with a reason in its `Cargo.toml`:

| Crate | Why |
|---|---|
| `windows` and companions | WinRT and Direct2D bindings, Microsoft's own |
| `chrono-tz` | Time zones by name: repeating events cannot be right without them |
| `quick-xml` | CalDAV's XML, with no DTDs |
| `ed25519-dalek`, `sha2`, `base64`, `getrandom` | Release signatures and tokens |
| `embed-resource` | The icon and version in `keenu.exe`, at build time |
| `objc2` and companions | AppKit bindings |
| `security-framework` | The Keychain |

## Classification of each feature

| Feature | Before | Now |
|---|---|---|
| Keenu identity | MISSING | COMPLETE |
| Data migration | MISSING | COMPLETE: tested from a synthetic old store |
| Shared core and provider interfaces | PARTIAL | COMPLETE |
| Media: Windows | PARTIAL (did not compile) | PARTIAL: real session tested; artwork and seeking not exercised |
| Media: Linux | COMPLETE | COMPLETE |
| Media: macOS | MISSING | PARTIAL: type-checks; limited by macOS; **never run** |
| Lyrics | PARTIAL | COMPLETE on Windows, for local and for LRCLIB through mocks |
| Notifications: Windows | PARTIAL (did not compile) | PARTIAL: shown; buttons unused |
| Calendar: CalDAV | PARTIAL (did not compile) | COMPLETE against one real server; others untested |
| Calendar: Google, Microsoft | MISSING | BLOCKED: code and mocks done; needs client IDs |
| Clipboard: Windows | PARTIAL (did not compile) | PARTIAL: text fully tested; other formats unit-tested |
| File shelf: Windows | PARTIAL (did not compile) | PARTIAL: drag itself needs a person |
| Voice: Windows | PARTIAL (did not compile) | PARTIAL: needs a person with a microphone |
| Agent approvals: Windows | PARTIAL (did not compile) | PARTIAL: complete against the documented schema; real CLIs not run |
| Agent approvals: Linux | COMPLETE | COMPLETE |
| Usage | PARTIAL (did not compile) | COMPLETE on Windows |
| Updates | PARTIAL (did not compile) | BLOCKED: built and tested up to installing; needs a certificate |
| Native lifecycle: Windows | PARTIAL (did not compile) | PARTIAL: simulated; multi-monitor needs a person |
| Onboarding and settings: Windows | MISSING | COMPLETE |
| Installers: Windows | PARTIAL (never built) | COMPLETE, for unsigned |
| Installers: Linux | COMPLETE | PARTIAL: installed on a headless GNOME Shell as test builds; a release's own build has not finished |
| Installers: macOS | MISSING | PARTIAL: written; **never run**, and neither it nor the program has been linked |
| AppImage | MISSING | UNSUPPORTED: does not fit a shell extension |
| One-command install | PARTIAL (Linux) | PARTIAL: rehearsed in full against a local server on Windows and Linux; no real address yet, and never on macOS |
| CI and release automation | PARTIAL | PARTIAL: written, parsed; **not yet run on GitHub** |
| Website | MISSING | COMPLETE: logic tested; served once a release exists |
| KDE and other desktops | UNSUPPORTED | UNSUPPORTED: no island |
| Accessibility: keyboard | MISSING | COMPLETE on Windows |
| Accessibility: screen reader | MISSING | MISSING on Windows and macOS |
