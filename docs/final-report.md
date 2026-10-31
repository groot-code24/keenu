# Keenu 2.0.0-alpha.1: final report

State of the repository on 2026-10-31, after the first release. The individual checks are in
[the acceptance matrix](feature-acceptance-matrix.md), and what works on each system in
[what works where](platform-support.md).

**Keenu is an alpha and is not ready for production.** It is published and installs on all three systems, but only
Windows has been used on a real desktop, on one computer with one display. On Linux and macOS it has run on test
machines with no person looking at it. Nothing is code signed.

## Where to get it

- Release: <https://github.com/groot-code24/keenu/releases/tag/v2.0.0-alpha.1>, marked as a prerelease
- Windows, in PowerShell: `irm https://raw.githubusercontent.com/groot-code24/keenu/main/scripts/install/install.ps1 | iex`
- Linux and macOS: `curl -fsSL https://raw.githubusercontent.com/groot-code24/keenu/main/scripts/install/install.sh | sh`
- Website: `website/`, ready for Vercel through `vercel.json`. **Not deployed yet**: see below

## Summary by feature

| Status | Features |
|---|---|
| **Complete** | Keenu identity. Data migration, from synthetic old data. Shared core and provider interfaces. Local lyrics on Windows. CalDAV against one real server. Usage tracking on Windows. Onboarding and settings on Windows. Unsigned installers for all three systems. One-command install. Release automation. Keyboard use on Windows. The website's logic |
| **Partial** | Windows media: real session tested; artwork and seeking not exercised. Windows notifications: shown; buttons unused. Clipboard: text tested for real; rich text, images and files unit-tested. File shelf: dragging needs a person. Voice: needs a person with a microphone. Agent approvals: complete against the documented schema; real CLIs not run. Windows lifecycle: simulated; several displays need a person. Linux: real desktop not tried. macOS: starts, but nobody has used it |
| **Blocked** | Google Calendar and Microsoft 365: need client IDs. Installing updates: needs a code-signing certificate. Signing and notarizing |
| **Missing** | Screen reader support on Windows and macOS. On macOS: clipboard, agent approvals, single instance, updates. Windows and macOS lack the Linux edition's focus timer, calls, AI assistant and plan usage pages |
| **Not supported** | AppImage: it does not fit a GNOME Shell extension. An island on KDE and other Linux desktops. A Windows ARM64 installer: built by CI, not packaged |

## Builds

Built by GitHub Actions from the tag, and published. None is code signed; each has a checksum in `SHA256SUMS` and a
build provenance attestation.

| System | Result |
|---|---|
| Windows x64, MSVC | `.msi`, `-setup.exe`, `-portable.zip` |
| Windows ARM64, MSVC | Compiles; no download |
| Linux x64 and ARM64, with Vulkan | `.tar.gz`, `.deb`, `.rpm` |
| macOS Apple silicon and Intel | `.dmg`, with an ad hoc signature only |

## Tests

| System | Check | Result |
|---|---|---|
| GitHub: Windows Server, MSVC | Lint, unit tests, `smoke-test.ps1` on the runner's desktop, `installer_test.py` | Pass |
| GitHub: Ubuntu 24.04 x64 and arm64 | Lint and unit tests of the daemon, shelf, whisper, core and integrations | Pass |
| GitHub: Ubuntu 24.04, Debian 13, Fedora 43 containers | Tarball, `.deb`, `.rpm` and one-command install of x64, headless GNOME Shell: extension active, no JS errors | Pass |
| GitHub: macOS 15, arm64 and x64 | Lint, unit tests, build, disk image, one-command install, still running after 10 s, removal | Pass |
| GitHub | Formatting, ShellCheck, PowerShell parsing, JavaScript lint, `cargo deny`, `npm audit`, secret scan | Pass |
| GitHub: clean machines, **the published release** | Every download compared with the signed list; the public commands on Windows, macOS on both, Ubuntu, Debian, Fedora | Pass |
| This computer: Windows 11 26300, **the published release** | The public command: installed, Start Menu entry, responsive window, removed with data kept and agent settings unchanged | Pass |
| This computer: mingw-w64 build | Real media session; real Radicale CalDAV server, 0 failures in 100 rounds; clipboard on the real clipboard | Pass, on earlier builds of this version |
| This computer: Edge, headless | The page with the real list: a button for each system, all 11 links answer; no sideways scrolling at 360 px | Pass |

Not run, as it needs something not available here: **a person using Keenu on a Mac or on a real GNOME desktop**, Linux
ARM64 installs, Windows 10, the real Claude Code and Codex CLIs, several displays, a microphone, Google and Microsoft
accounts, real sleep and resume, other CalDAV servers, and the Vercel deployment itself.

`vercel.json` has only been checked against Vercel's published schema and its headers served locally, as deploying needs
your account.

## What the first real CI runs found

All fixed, and none could have been found on one Windows computer:

- No script in the repository was executable, so nothing could be packaged on Linux or macOS.
- `macos-13` no longer exists, and `macos-14` goes on 2026-11-02: CI now uses `macos-15` and `macos-15-intel`.
- **Start at login failed on a Windows profile with no Run key.** A real bug in the program, with a new test.
- The smoke test's own copy could deadlock with the island; it now copies as a native program does.
- The install scripts refused to install when only a test version existed, closed the window on failure under `iex`, and
  on macOS would have run the stub `python3`, which only offers to install the developer tools.

## Security

[The threat model](threat-model.md) has the full list.

| Finding | Severity | Status |
|---|---|---|
| Installers and programs are unsigned | High for a stable release | **Open**: needs certificates. People see warnings, and that is the honest state |
| The published 2.0.0-alpha.1 for Windows says updates are available, then refuses any it finds | Low: nothing unverified installs | **Open in the release, fixed on `main`**. Updates are off by default |
| The release list's signature is checked by `install.sh` only on Linux with OpenSSL 3 | Medium | **Open**: `install.ps1` and macOS check the address and each file's SHA-256, not the signature, as they have no Ed25519 to hand. Files come over HTTPS from GitHub, as do the scripts |
| One antivirus product blocked a PowerShell test script as malicious, on its behaviour | Distribution risk | **Open**. The installers have not been checked with antivirus products |

`cargo deny`, `npm audit` and the secret scan pass. No credentials are in the repository.

## What only you can do

1. **Deploy the website.** At [vercel.com/new](https://vercel.com/new), import `groot-code24/keenu`, change nothing, and
   deploy. Then open the page, and run `curl -fsSL https://<your-site>/install.sh | head -n 3`.
2. **Use it on a real Mac and a real GNOME desktop.** No test can say whether it looks right.
3. **Back up the signing key:** `%USERPROFILE%\.keenu-release\update-signing-secret.txt`. Without it, later release lists
   cannot be signed, and installers will refuse them. It is not on GitHub.
4. **For a stable release:** a Windows code-signing certificate, an Apple Developer ID, and your publisher name.
5. **For Google and Microsoft calendars:** OAuth client IDs.

## Making the next release

Set the version in `Cargo.toml` and `gnome-extension/metadata.json`, update `Cargo.lock` and the changelog, then tag and
push. [Release engineering](release-engineering.md) has the steps. The workflow does not see a repository secret
`KEENU_UPDATE_SECRET_KEY` at present, so it publishes the files and offers them to nobody; then, from `main`:

```sh
node scripts/sign-release-list.mjs <version> "$USERPROFILE/.keenu-release/update-signing-secret.txt"
git add website/keenu-release.json website/keenu-release.json.sig
git commit -m "Release list for v<version>" && git push
```

That push starts the checks of what was published. If the secret is added, the workflow does all of this itself.

## Reproducing the builds

GitHub Actions is the reference: see [the workflows](../.github/workflows). Locally, see
[release engineering](release-engineering.md#reproducing-the-builds). This computer cannot build the Linux packages: whisper.cpp's
Vulkan shaders do not compile in Docker's 3.5 GB here.

## What this work did to this computer

- **Docker.** Compiling whisper.cpp ran Docker's 3.5 GB machine out of memory and very probably brought its engine down.
  When it came back, these containers had exited with code 255: `context-commerce-postgres-1`, `-s3-1` and `-mailpit-1`,
  with `-api-1` and the workers restarting without them. They were left alone. Docker Desktop was later found fully stopped
  at about 15:00; nothing here stopped it, and whether it crashed or was closed is not known. It has stayed stopped.
- **Left behind on purpose:** the signing key in `%USERPROFILE%\.keenu-release`, tools and logs in `D:\tmp`, Docker images and
  volumes named `keenu-*`, pulled `debian:13`, `fedora:43` and `python:3-slim`, and built files in `dist/`, which git ignores.
  `dist/` holds older local builds: the real downloads are on the release page.
- **Registry:** `HKCU\Software\Classes\AppUserModelId\Keenu.Desktop`, from showing a toast.
- **Data:** `%LOCALAPPDATA%\Keenu` stays, as removal keeps data. It holds only what the tests made.
- **On your desktop during tests:** approval prompts for a project called `demo`, notifications, and Keenu briefly installed
  and removed. Earlier runs used the clipboard: text was put back, and anything else on it then was lost.
- **Not touched:** your real `~/.claude/settings.json` and `~/.codex`, checked after the last removal.
