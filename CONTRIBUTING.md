# Contributing to Keenu

Thanks for helping! This guide covers how to build, test and send changes.
By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
Security problems go privately to the maintainer; see
[SECURITY.md](SECURITY.md).

## Ground rules

- **The spec leads.** [`docs/product-spec.md`](docs/product-spec.md) defines
  how the island looks and behaves. If a change alters behaviour, update the
  spec in the same pull request.
- **Small details matter.** Every state change shows in the icon, not only in
  colour; disabled controls dim in place; nothing jumps. See "State details"
  in the spec.
- **Stay light.** No polling, no timers that run while nothing is visible, and
  animations that stop when they settle.
- **Logic goes in Rust.** The extension draws; rules that can be unit tested
  belong in `crates/keenu-core`.
- **Nothing goes online unasked.** A feature that uses the network says so in
  [SECURITY.md](SECURITY.md#what-leaves-your-computer) in the same pull
  request, and can be turned off.
- **Keenu's own words.** Describe Keenu, its look and its settings in their own
  terms, not by comparison with other products. What differs between systems goes
  in [what works where](docs/platform-support.md).
- **A claim needs evidence.** A feature is not "done" because it compiles. Say what
  was run, on what, in [the acceptance matrix](docs/feature-acceptance-matrix.md), and
  mark mocks as mocks.
- **Decisions in shared code.** What the island does goes in `keenu-app` and `keenu-ui`,
  tested without a display. A host only connects them to the operating system.

## Building

`./dev.sh` or `.\dev.ps1` checks your tools, tells you what is missing, builds and starts Keenu. `./build.sh`,
`.\build.ps1`, `./test.sh` and `.\test.ps1` do what they say. None of them install system software.

On Windows you need either Rust with the Visual Studio C++ tools, or only Docker Desktop, which builds in a container
and runs the tests natively. By hand, on Linux:

```sh
cargo build                     # the shared crates
cargo build -p keenu-daemon     # keenud
cargo build -p keenu-shelf      # needs libgtk-4-dev
./install.sh                    # build and install for your user
```

[The architecture](docs/architecture.md) says what is where.

## Checks

Run these before sending a pull request (CI runs the same). The workspace
includes `keenu-shelf` and `keenu-whisper`, so they need GTK 4
(`libgtk-4-dev`) and, for whisper.cpp, `cmake`, `g++`, `clang` with
`libclang-dev`, the Vulkan headers (`libvulkan-dev`) and `glslc`. Without
those, add `--exclude keenu-shelf --exclude keenu-whisper` to check the rest.

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm ci && npx eslint                                  # the extension (eslint.config.mjs)
node --test tests/extension/*.test.js tests/website/*.test.js   # its parts that need no GNOME Shell, and the download page
gjs -m tests/extension/format.gjs.js                  # and those that need GLib
glib-compile-schemas --strict --dry-run gnome-extension/schemas
shellcheck -x install.sh uninstall.sh dev.sh build.sh test.sh scripts/*.sh scripts/install/install.sh scripts/keenu-charge-limit scripts/ci/*.sh packaging/aur/update.sh
cargo deny check                                      # advisories and licences (deny.toml)
```

CI also builds the release packages and installs each into a headless GNOME Shell on Ubuntu
24.04, Debian 13 and Fedora, failing on any `JS ERROR` or if `keenud` never starts. On Windows it drives the real
program and puts the real installers through install, upgrade and removal.
Run that smoke test locally with Docker after `scripts/package.sh`:

```sh
docker run --rm -v "$PWD:/src:ro" ubuntu:24.04 /src/scripts/ci/smoke-test.sh /src/dist/keenu-<version>-x86_64-linux.tar.gz
```

## Trying the extension without logging out

GNOME Shell can run nested inside your session:

```sh
dbus-run-session -- gnome-shell --nested --wayland     # GNOME 46 to 48
dbus-run-session -- gnome-shell --devkit --wayland     # GNOME 49 and newer
```

Keenu supports GNOME Shell 46 to 51. Where they differ (see the porting
guides on [gjs.guide](https://gjs.guide/extensions/upgrading/)), the
difference goes in `src/compat.js` so the rest of the code reads the same on
all of them; check a change on the oldest and the newest.

GNOME Shell caches extension code, so restart the nested shell after each
change. Logs:

```sh
journalctl --user -u keenud -f                        # the daemon
journalctl -f /usr/bin/gnome-shell | grep -i keenu    # the extension
```

## Code style

- Rust: `rustfmt` defaults, `clippy` clean, doc comments on public items.
- JavaScript: GNOME Shell's style (4 spaces, single quotes, ES modules);
  match the surrounding code.
- Comments explain *why*, not *what*.
- Every source file starts with `SPDX-License-Identifier: GPL-3.0-or-later`.

## Commits and pull requests

- One topic per pull request: two fixes are two pull requests, so each can be
  reviewed, tested and taken on its own. Describe what changed and how you
  tested it.
- When `main` moves on, rebase on it rather than merging it in.
- For visual changes, include screenshots, and try them on GNOME 46 and the
  newest GNOME Shell (nested, or in a virtual machine).
- Your commits keep your name. What ships is credited in the changelog
  ("Thanks to @you") and you are listed under
  [Contributors](README.md#contributors) in the README.
- By contributing you agree that your work is licensed under the
  GPL-3.0-or-later, like the rest of Keenu.

## Releasing

1. Move the `[Unreleased]` notes in `CHANGELOG.md` under a new version and
   date, and set that version in `Cargo.toml` and in
   `gnome-extension/metadata.json` (`version-name`).
2. Commit, then tag and push: `git tag -a v1.2.3 -m "Keenu 1.2.3"` and
   `git push origin main v1.2.3`.
3. The Release workflow builds `keenu-1.2.3-x86_64-linux.tar.gz` with the
   programs ready-built, and from it `keenu_1.2.3_amd64.deb` and
   `keenu-1.2.3-1.x86_64.rpm`, each with a build provenance attestation. It
   attaches them and their checksums to the tag's GitHub release, which
   `get.sh` installs from, with the changelog's section as the notes.
4. Point the AUR package at the release: `packaging/aur/update.sh 1.2.3`,
   then commit and push `PKGBUILD` and `.SRCINFO` to the AUR's `keenu-bin`.

`scripts/package.sh` builds the tarball locally, in `dist/`, and
`scripts/package-system.sh dist/keenu-….tar.gz` the .deb and .rpm from it
(it needs `dpkg-dev`, `rpm` and `file`).
