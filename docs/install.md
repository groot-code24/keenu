# Installing Keenu

Three steps and an optional fourth: about a minute, no root and nothing to compile.

## 1. Check your desktop

Keenu runs on **GNOME Shell 46 to 51** on a 64-bit Intel or AMD PC. Check
in a terminal:

```sh
gnome-shell --version   # GNOME Shell 46.x up to 51.x
uname -m                # x86_64
```

Each of these was installed with the command below and checked:

| GNOME Shell | Ubuntu | Fedora Workstation | Debian |
|---|---|---|---|
| 46 | 24.04 LTS | 40 | |
| 47 | 24.10 | 41 | |
| 48 | 25.04 | 42 | 13 |
| 49 | 25.10 | 43 | |
| 50 | 26.04 LTS | 44 | |
| 51 | 26.10 | 45 | |

Other systems with one of these GNOME Shell versions work the same way.
Other desktops (KDE Plasma, Cinnamon, XFCE, COSMIC, …) cannot load GNOME
Shell extensions, so Keenu does not run there; nor does it run on ARM
computers (Raspberry Pi and other ARM machines), for which there are no ready-built
programs.

## 2. Install

Paste this into a terminal and press Enter:

```sh
curl -fsSL https://raw.githubusercontent.com/groot-code24/keenu/main/scripts/install/install.sh | sh
```

It downloads the latest release, checks its checksum and installs it for
your user. It ends with `==> Keenu 1.x.y is installed` and what to do next:
log out and back in, and how to check or remove Keenu. Run it as yourself,
**without sudo**: everything goes in your own home folder, and it refuses to
run as root. On a GNOME Shell version Keenu was not made for it stops before
changing anything.

## 3. Log out and back in

GNOME Shell on Wayland only loads a new extension at login, so log out and
back in once. (On GNOME 46 to 48 with an X11 session you can instead press
Alt+F2, type `r` and press Enter.)

The island now sits in the middle of the top bar, in place of the clock.
Rest the pointer on it to open it.

## 4. The voice assistant (optional)

Everything else works without it. To ask the island things out loud with
Super+I:

1. Install and sign in to one of Claude Code, Codex, Grok or Antigravity (the
   [README](../README.md#voice-assistant) says how; each uses your own plan).
2. Set up speech recognition once, in a terminal. It downloads the speech
   model picked in Settings → Assistant (`small`, 265 MB, by default; or name
   one, which Settings then uses: `keenu-setup-voice base`) and needs nothing
   else installed. A graphics
   card is used through Vulkan when there is one, else the processor:

   ```sh
   ~/.local/bin/keenu-setup-voice
   ```

   Set up with an older Keenu, speech recognition used a Python environment
   of up to 3 GB; updating Keenu with the installer downloads the new model
   and deletes it by itself (`keenu-setup-voice --remove-old` does the same by
   hand). In the assistant, a **Set up** button opens a terminal that runs
   `keenu-setup-voice` for you.

3. Pick the AI in Keenu's Settings → Assistant, or leave it on Automatic,
   and press Super+I. An AI installed later is found by itself.

`keenu-setup-voice` ends by checking what Keenu itself finds; run the same check
any time:

```sh
~/.local/bin/keenud --doctor
```

(Keenu's commands are in `~/.local/bin`. When that folder is on your PATH,
which the installer checks, their names alone do.)

It lists speech recognition (its models, and the GPU or processor it runs
on) and each AI, where it is installed and whether
you are signed in. Keenu finds AIs installed with npm (also a prefix of your
own), nvm, volta, fnm, pnpm, bun, Homebrew, mise, asdf, Nix or their own
installers, and takes what your login shell sets for them
(`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, an API key, a proxy).

## Updating

Keenu tells you in the island when a new version is out.

1. Click **Update** in that alert, or open Keenu's Settings → **About**.
2. Click **Update**. It takes a few seconds; no password is needed, and you
   can close Settings meanwhile.
3. Click **Log Out** and log back in.

Your tasks, calendar events, clipboard history and settings stay.

**Keenu 1.3 or older**, or the button didn't work? Paste this in a terminal,
press Enter, then log out and back in:

```sh
curl -fsSL https://raw.githubusercontent.com/groot-code24/keenu/main/scripts/install/install.sh | sh
```

**Installed from a package?** Update it the way you installed it: download
the new `.deb` or `.rpm` from [Releases](https://github.com/groot-code24/keenu/releases/latest)
and open it, or on Arch run `git pull` and `makepkg -si` again in
`packaging/aur`. Then log out and back in. Don't also run the command above: it would add a second copy (see
[below](#both-a-package-and-the-one-line-install)).

To install a particular version, put `KEENU_VERSION` before `sh`:
`… | KEENU_VERSION=2.0.0-alpha.1 sh`.

## Uninstalling

```sh
~/.local/bin/keenu-uninstall
```

This removes the extension, its programs, its service and its font, the
voice assistant's speech recognition and its models, Keenu's caches, and a
battery charge limit Keenu set (asking for your password), and takes Keenu's
hooks out of Claude Code's and Codex's settings. Your tasks,
calendar events, clipboard history, timer settings and app usage (if you
turned the Apps page on) stay in `~/.local/share/keenu`; delete it too to
remove everything.

Remove Keenu this way, not with the Extensions app's Remove button: that
deletes only the extension, and leaves `keenud`, its service and its hooks.
A system-wide package is removed as [below](#both-a-package-and-the-one-line-install).

## Without internet on the computer

Download `keenu-<version>-x86_64-linux.tar.gz` from
[Releases](https://github.com/groot-code24/keenu/releases) on another computer,
copy it over, then:

```sh
tar -xzf keenu-*-x86_64-linux.tar.gz
cd keenu-*-x86_64-linux
./install.sh
```

## Other ways to install

### Ubuntu and Debian (.deb), Fedora (.rpm)

Each [release](https://github.com/groot-code24/keenu/releases) also has
system-wide packages, installed for every user of the computer with the
system's package manager (so they update and uninstall with it):

```sh
sudo apt install ./keenu_*_amd64.deb        # Ubuntu 24.04 and newer, Debian 13
sudo dnf install ./keenu-*.x86_64.rpm       # Fedora 40 and newer
```

They put the programs in `/usr/bin`, the extension in
`/usr/share/gnome-shell/extensions`, and the rest where the system expects
it. GNOME Shell only finds a new extension at login, and turns extensions
on per user: so each user logs out and back in, then turns Keenu on once, in
the Extensions app or with:

```sh
gnome-extensions enable isle@zickrian.github.io
```

`keenud` starts on its own when the extension first calls it. The voice
assistant is set up per user as above, with `keenu-setup-voice`.

### Arch Linux

Keenu is not in the AUR yet, so `yay -S keenu-bin` or `paru -S keenu-bin` finds nothing. Either use
the one-line install (step 2), or build the `keenu-bin` package from the `PKGBUILD` in
[`packaging/aur`](../packaging/aur), made from the release tarball:

```sh
sudo pacman -S --needed base-devel git
git clone --depth 1 https://github.com/groot-code24/keenu.git
cd keenu/packaging/aur
makepkg -si
# Log out and back in, then:
gnome-extensions enable isle@zickrian.github.io
```

To update, run `git pull` in that folder and `makepkg -si` again.

### Both a package and the one-line install

A copy installed with `install.sh`, in your home folder, takes precedence for
you over a system-wide package: GNOME Shell, D-Bus and systemd all look in
your home folder first. Run `keenu-uninstall` to remove your own copy; the
package's then takes over, with your settings and data kept.

Before removing the package itself, each user who ran Keenu takes its alert
hooks out of Claude Code's and Codex's settings, which would otherwise call
an `keenud` that is gone. A battery charge limit set with the assistant
stays after the package goes, so lift it first too:

```sh
keenud --unhook-all
keenu-charge-limit 100       # only if the assistant set a charge limit
sudo apt remove keenu        # or: sudo dnf remove keenu, sudo pacman -R keenu-bin
```

### Checking a download

Release files are built by GitHub Actions from the tagged source, with a
build provenance attestation. With the [GitHub CLI](https://cli.github.com):

```sh
gh attestation verify keenu-*-x86_64-linux.tar.gz --repo groot-code24/keenu
```

`install.sh` checks each download against the SHA-256 in the release manifest already.

## What the installer changes

Everything goes in your home folder:

| Where | What |
|---|---|
| `~/.local/share/gnome-shell/extensions/isle@zickrian.github.io` | The GNOME Shell extension |
| `~/.local/bin` | `keenud` (the background service), `keenu-shelf` (drag and drop), `keenu-whisper` (speech recognition), `keenu-uninstall`, `keenu-setup-voice`, `keenu-charge-limit` |
| `~/.config/systemd/user`, `~/.local/share/dbus-1/services` | Starting `keenud` with your session |
| `~/.local/share/fonts/keenu` | Inter, the island's typeface |
| `~/.local/share/keenu` | Your data, and the speech models `keenu-setup-voice` downloads (`models`) |
| `~/.local/share/doc/keenu` | This README |

Once running, Keenu adds its hooks to Claude Code's `~/.claude/settings.json`
and Codex's `~/.codex/hooks.json` when it finds them, so they ask you on
the island (Codex asks once, at its next start, to trust them); turning
them off in Settings → AI Usage → AI Agents, or uninstalling, takes them
out. Commands you run from the assistant keep their logs in
`~/.cache/keenu/runs` until they end.

On a first install only, it sets Keenu's look (a slim glass top bar with the
island in it); updates leave it as you have it. It never changes other
extensions: when one would fight with the island (Blur my Shell's top bar
blur, or an extension that takes the same place in the top bar, such as Hide
Top Bar), it ends with a recommendation and the command to turn that off. If
GNOME's extensions were switched off altogether (the Extensions app's main
switch), it switches them on.

## If something goes wrong

**The island does not appear.** Log out and back in once more, then check:

```sh
gnome-extensions info isle@zickrian.github.io   # State: ACTIVE (or ENABLED)
```

If it says `OUT OF DATE`, your GNOME Shell version is outside 46 to 51. If
it says `ERROR`, that is most likely a bug in Keenu: please
[open an issue](https://github.com/groot-code24/keenu/issues) with GNOME Shell's
log (see **Anything else** below).

**Music does not show.** Check the background service:
`systemctl --user status keenud`.

**The voice assistant does not answer.** Run `~/.local/bin/keenud --doctor` (with a .deb, .rpm
or AUR package, `keenud --doctor`): it says what is missing (speech recognition, an AI, or its
sign-in) and what to run.

**Anything else.** GNOME Shell's log shows extension errors:
`journalctl -b -o cat /usr/bin/gnome-shell | grep -i keenu`. Please
[open an issue](https://github.com/groot-code24/keenu/issues) with what it says
and the output of `gnome-shell --version`.
