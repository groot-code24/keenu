# Keenu 2.0.0-alpha.1: final report

State of the repository at the end of the work on 2026-10-31. The individual checks are in
[the acceptance matrix](feature-acceptance-matrix.md), and what works on each system in
[what works where](platform-support.md).

**Keenu is an alpha and is not ready for production.** Windows is the only system that has run for real, on one computer
with one display. Linux has run only in containers without a real desktop session. macOS has never run. Nothing is
signed, nothing is published, and no CI workflow has ever run.

## Summary by feature

| Status | Features |
|---|---|
| **Complete** | Keenu identity. Data migration, from synthetic old data. Shared core and provider interfaces. Local lyrics on Windows. CalDAV against one real server. Usage tracking on Windows. Onboarding and settings on Windows. Unsigned Windows installers. Keyboard use on Windows. The website's logic |
| **Partial** | Windows media: real session tested; artwork and seeking not exercised. Windows notifications: shown; buttons unused. Clipboard: text tested for real; rich text, images and files unit-tested. File shelf: dragging needs a person. Voice: needs a person with a microphone. Agent approvals: complete against the documented schema; real CLIs not run. Windows lifecycle: simulated; several displays need a person. Linux installers: test builds only. One-command install: rehearsed against a local server. CI and release automation: written and parsed, never run |
| **Blocked** | Google Calendar and Microsoft 365: need client IDs. Installing updates: needs a code-signing certificate. Signing and notarizing. Linux release build here: needs more memory. Publishing: needs an owner, a domain and your go-ahead |
| **Missing** | Screen reader support on Windows and macOS. On macOS: clipboard, agent approvals, single instance, updates. Windows and macOS lack the Linux edition's focus timer, calls, AI assistant and plan usage pages |
| **Never run** | Everything on macOS: it is type-checked only, never linked or started |
| **Not supported** | AppImage: it does not fit a GNOME Shell extension. An island on KDE and other Linux desktops. Windows ARM64 installer: not built |

## Builds

| System | Result | Notes |
|---|---|---|
| Windows x64, mingw-w64 in Docker | **Built**: `keenu.exe`, MSI, setup program, ZIP | Final code |
| Windows x64, MSVC | **Not run** | This computer has no Visual Studio. MSVC is what official releases use |
| Windows ARM64 | **Not run** | CI only |
| Linux x64, test build | **Built**: tarball, `.deb`, `.rpm` | Final code, with speech on the processor only, from a scratch copy |
| Linux x64, release build | **Failed here**: out of memory | whisper.cpp's Vulkan shaders do not compile in Docker's 3.5 GB. Expected to fit on a CI runner, but unproven |
| Linux ARM64 | **Not run** | CI only |
| macOS, both architectures | **Type-checked only** | Not linked, as that needs Apple's SDK |

Files in `dist/`, none signed:

| File | SHA-256 |
|---|---|
| `Keenu-2.0.0-alpha.1-windows-x64.msi` | `090d80c17e4f1de2b12f4b07c5719db87b64c7354e83b40dc4c6014e556aca6b` |
| `Keenu-2.0.0-alpha.1-windows-x64-setup.exe` | `6c76e2e4d2fd8ea67b04d9bccd715b1ca3ccc4407c43612e1d4e60e96e500f0b` |
| `Keenu-2.0.0-alpha.1-windows-x64-portable.zip` | `8096df9c7fd1383d15e27df7104279389244f225ac88de5034a3e3fa49ba6010` |
| `linux-test/keenu-2.0.0-alpha.1-x86_64-linux.tar.gz` | `b536b4ed6923607d5951a3906e0d280e74fb18a20e399bdba2087588a55f04df` |
| `linux-test/keenu_2.0.0~alpha.1_amd64.deb` | `fd8a2408b64ab4db94b3f4e9d7481732b8fc9c2b29b48899642d8914ed75caf4` |
| `linux-test/keenu-2.0.0~alpha.1-1.x86_64.rpm` | `5797decffca47de5719933153069703ac7ab19a5be1c416d55609be0b8813e20` |

The files in `linux-test/` are for testing only, and must not be published: they lack the graphics card support, and hold
the placeholder repository.

## Tests

| System | Check | Result | On the final code? |
|---|---|---|---|
| Windows 11 26300 x64 | Unit tests, run natively: app 53, core 124, integrations 113, release 6, security 11, UI 24, Windows 25 | **356 passed, 0 failed** | Yes |
| Windows | `smoke-test.ps1` against the release build, 3 runs, at up to 87% CPU | **44 passed, 0 failed, 1 skipped**, each run | Yes |
| Windows | The same test's clipboard checks, on the real clipboard | Passed in an earlier run of 52 of 52 | **No**: skipped so as not to lose what was on the clipboard |
| Windows | `installer_test.py`: MSI, upgrade, downgrade refused, setup, one-command, tampering | **36 of 36** | Yes |
| Windows | Real media session: metadata, play and pause both ways, removal | Passed | Yes |
| Windows | Real CalDAV server (Radicale 3.5), full cycle | **0 failures in 100 rounds** | Yes |
| Windows | Formatting; clippy with `-D warnings` | Clean | Yes |
| Linux, Debian 13 container | `test.sh`: app 53, core 125, daemon 207 (5 ignored), integrations 109, release 6, security 10, UI 24 | **534 passed, 0 failed** | **No**: before the last fixes |
| Linux | Clippy with `-D warnings`, including tests and the shelf; shellcheck | Clean | Yes |
| Linux, Debian 13, GNOME Shell 48.7 | Tarball installed, extension active, no JS errors | **Passed** | Yes |
| Linux, Debian 13, GNOME Shell 48.7 | `.deb` the same way | **Passed** | **No**: before the last fixes; the final run was cut off |
| Linux, Fedora 43, GNOME Shell 49.10 | `.rpm` the same way | **Passed** | **No**: before the last fixes |
| Linux, Debian 13 | `scripts/install/install.sh`, end to end | **20 of 20** | Yes |
| Node 24 | Extension 39, website 6 | **45 of 45** | Yes |
| macOS | Clippy with `-D warnings`, both architectures | Clean | Yes |
| GitHub Actions | All workflows | **Never run**: parsed only | |
| One-command scripts | `install.ps1` refusing without an address and with an insecure one; `dev.ps1 -Check`; `test.sh`; both `install.sh` | Passed | `test.sh`: before the last fixes |
| One-command scripts | `dev.ps1`, `build.ps1` and `test.ps1` in full; `dev.sh`, `build.sh` | **Not run** as themselves: the same steps were run through the commands below | |

The matrix's totals: **86 PASS, 0 FAIL, 28 NOT RUN, 8 BLOCKED, 1 NOT SUPPORTED**. A PASS there may be from before the last
fixes: its section "How current each result is" says which.

Not run, as it needs something this computer does not have: anything on a Mac, the real Claude Code and Codex CLIs,
several displays, a microphone, Google and Microsoft accounts, real sleep and resume, a person using the program, and
other CalDAV servers.

## Security findings

All fixed unless stated. [The threat model](threat-model.md) has the full list.

| Finding | Severity | Status |
|---|---|---|
| Extension's Update ran a script from a placeholder GitHub name anyone could register | High | Fixed: it does nothing until a real repository is set, and a release refuses to publish the placeholder |
| Old Windows edition fetched updates from the upstream author's repository | High | Fixed |
| CalDAV requests lost at random, which could lose an edit | High | Fixed. The daemon's own HTTP agent on Linux still reuses connections: it only reads, and tries again later, so it was left alone |
| `quick-xml` advisories RUSTSEC-2026-0194 and -0195 | Medium | Fixed by upgrading to 0.42; neither path was in use. **`cargo deny` has not been run again** |
| Deeply nested XML could end the program | Medium | Fixed |
| A prompt could appear under the pointer and take a stray click on Allow | Medium | Fixed: Allow ignores presses for its first 700 ms, and focus resets |
| `powershell.exe` and `msiexec` looked up by name | Medium | Fixed |
| Installers and programs are unsigned | High for a release | **Open**: needs certificates |
| Automatic updates cannot install without a certificate | | **Open**, and fails closed |
| One antivirus product blocked a PowerShell test script as malicious, on its behaviour | Distribution risk | **Open**: the test moved to Python, with no attempt to evade. `install.ps1` ran; signed installers need checking |

No credentials were committed, and no secrets are stored in plain text. Tokens are in the system's credential store.

## What is blocking a release

Only you can provide these:

1. **The GitHub owner**, to replace `YOUR-GITHUB-OWNER`, and the `KEENU_RELEASE_REPOSITORY` variable.
2. **The website's domain**, to replace `YOUR-KEENU-DOMAIN`.
3. **The publisher's legal name and maintainer contact**: `KEENU_PUBLISHER`, `KEENU_MAINTAINER`.
4. **A Windows code-signing certificate**, and its thumbprint.
5. **An Apple Developer ID**, and access to a Mac.
6. **OAuth client IDs** for Google and Microsoft.
7. **`git init`, and permission to push.** The folder has no repository, and nothing has been committed.

[Release engineering](release-engineering.md#setting-up-once) has the details. Then, needing people or other
machines: MSVC and Linux release builds, through CI; a real GNOME desktop; a Mac; two displays; a microphone; and the real
agent CLIs.

## Reproducing the builds

From the repository's root, with Docker. Add `--memory` as [release engineering](release-engineering.md#reproducing-the-builds)
explains if Docker has little memory.

```sh
# Windows: keenu.exe and its installers, in dist/
docker build -t keenu-windows-cross -f packaging/docker/windows-cross.Dockerfile packaging/docker
docker run --rm -v "$PWD:/src" -v keenu-cross-target:/target -w /src keenu-windows-cross bash -c \
  'cargo build --release --locked --target x86_64-pc-windows-gnu -p keenu-windows -p keenu-release &&
   cp /target/x86_64-pc-windows-gnu/release/keenu.exe /target/x86_64-pc-windows-gnu/release/keenu-release.exe dist/ &&
   bash scripts/windows/package.sh dist/keenu.exe &&
   bash scripts/windows/package.sh dist/keenu.exe 1.9.0'

# Linux: needs more than 3.5 GB
docker build -t keenu-linux-build -f packaging/docker/linux-build.Dockerfile packaging/docker
docker run --rm -v "$PWD:/src" -v keenu-linux-target:/target -e CARGO_TARGET_DIR=/target -w /src keenu-linux-build \
  bash -c 'tarball="$(scripts/package.sh | tail -n1)" && scripts/package-system.sh "$tarball"'

# Linux tests, and an install on a headless GNOME Shell
docker run --rm -v "$PWD:/src" -v keenu-linux-target:/target -e CARGO_TARGET_DIR=/target -w /src keenu-linux-build bash test.sh
docker run --rm -v "$PWD:/src:ro" debian:13 bash /src/scripts/ci/smoke-test.sh /src/dist/<tarball or .deb>
docker run --rm -v "$PWD:/src:ro" fedora:43 bash /src/scripts/ci/smoke-test.sh /src/dist/<.rpm>
```

The second `package.sh` above makes the "older" installers the upgrade test needs. They hold the same program, labelled
1.9.0, so do not publish them.

```powershell
# Windows, on the desktop: the smoke test shows prompts and a notification, and -SkipClipboard spares your clipboard
.\scripts\windows\smoke-test.ps1 -Exe .\dist\keenu.exe -SkipClipboard

# Installs and removes for real, under your user, on a computer without Keenu
python scripts\windows\installer_test.py --msi dist\Keenu-2.0.0-alpha.1-windows-x64.msi `
  --old-msi dist\Keenu-1.9.0-windows-x64.msi --setup dist\Keenu-2.0.0-alpha.1-windows-x64-setup.exe `
  --zip dist\Keenu-2.0.0-alpha.1-windows-x64-portable.zip --release-tool dist\keenu-release.exe --version 2.0.0-alpha.1
```

```sh
# Anywhere with Node, and for formatting and the macOS type-check, Rust with those targets added
node --test tests/extension/*.test.js tests/website/*.test.js
cargo fmt --all --check
cargo clippy --locked --target aarch64-apple-darwin -p keenu-macos -- -D warnings
cargo clippy --locked --target x86_64-apple-darwin -p keenu-macos -- -D warnings
```

## To finish the checks that were cut off

Docker Desktop stopped during the last run. When it is running again, these bring every result up to the final code:

```sh
docker run --rm --memory 2500m --memory-swap 2500m -v "$PWD:/src" -v keenu-cargo:/usr/local/cargo/registry \
  -v keenu-linux-target:/target -e CARGO_TARGET_DIR=/target -w /src keenu-linux-build bash test.sh
docker run --rm --memory 2500m --memory-swap 2500m -v "$PWD:/src:ro" debian:13 \
  bash /src/scripts/ci/smoke-test.sh '/src/dist/linux-test/keenu_2.0.0~alpha.1_amd64.deb'
docker run --rm --memory 2500m --memory-swap 2500m -v "$PWD:/src:ro" fedora:43 \
  bash /src/scripts/ci/smoke-test.sh '/src/dist/linux-test/keenu-2.0.0~alpha.1-1.x86_64.rpm'
docker run --rm --memory 2500m --memory-swap 2500m -v "$PWD:/src" -v keenu-cargo:/usr/local/cargo/registry \
  -v keenu-linux-target:/target -w /src keenu-linux-build /target/tools/bin/cargo-deny check
```

Then update LX-1, LX-3b, LX-3c and LX-7 in the matrix.

## Before the first commit

The folder came with no git history, and scripts edited on Windows carry no executable bit. After `git init`:

```sh
git add -A
git update-index --chmod=+x install.sh uninstall.sh dev.sh build.sh test.sh scripts/*.sh scripts/ci/*.sh \
  scripts/keenu-charge-limit scripts/install/install.sh scripts/windows/package.sh packaging/macos/build-app.sh \
  packaging/aur/update.sh
```

## What this work did to this computer

- **Docker.** Compiling whisper.cpp ran Docker's 3.5 GB machine out of memory and very probably brought its engine down.
  When it came back, these containers of yours had exited with code 255: `context-commerce-postgres-1`, `-s3-1` and
  `-mailpit-1`, with `-api-1` and the workers restarting without them. They were left alone. Later builds had a memory limit,
  and one that ran out stayed inside its limit. Even so, Docker Desktop was found fully stopped at about 15:00. Nothing here
  stopped it, and whether it crashed or was closed is not known.
- **Left behind on purpose:** tools and logs in `D:\tmp` (`keenu-*`, `khelper`, `kbuild.ps1`, `kenv.ps1`, `radicale-native`,
  `propfind.xml`, and a shadow git history in `keenu-baseline.git`, outside the project). Docker images and volumes named
  `keenu-*`, and pulled `debian:13`, `fedora:43` and `python:3-slim`. Built files in `dist/`, which git ignores.
- **Registry:** `HKCU\Software\Classes\AppUserModelId\Keenu.Desktop`, written when showing a toast. The Run key was set and
  restored by a test.
- **On your desktop during tests:** approval prompts for a project called `demo`, and notifications. Those came from the
  tests, and approved nothing real. Earlier runs used the clipboard: text was put back, and anything else on it then was
  lost.
- **Not touched:** your real `~/.claude/settings.json` and `~/.codex`, checked at the end. No Keenu is left installed or
  running.
- **Nothing was committed, pushed or published**, and no owner, domain, publisher or credential was invented.
