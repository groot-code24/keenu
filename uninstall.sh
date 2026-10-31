#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 zickrian
# Removes everything install.sh and keenu-setup-voice put in place. Your shelf, clipboard history,
# tasks, timer settings and app usage in ~/.local/share/keenu are kept; delete that folder to remove them.
set -uo pipefail

UUID="isle@zickrian.github.io"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
# A system-wide package (.deb, .rpm, AUR) stays in use: it keeps the extension on and the voice set-up.
SYSTEM=0
[ -d "/usr/share/gnome-shell/extensions/$UUID" ] && SYSTEM=1

if [ "$SYSTEM" = 0 ]; then
    gnome-extensions disable "$UUID" 2>/dev/null
    # Drop it from the enabled list, in case GNOME Shell never loaded it, and from the disabled
    # one, where disabling put it: left there, it would keep a later install off.
    for key in enabled-extensions disabled-extensions; do
        current=$(gsettings get org.gnome.shell "$key")
        updated=$(printf '%s' "$current" | sed -e "s/, '$UUID'//" -e "s/'$UUID', //" -e "s/\['$UUID'\]/@as []/")
        [ "$updated" != "$current" ] && gsettings set org.gnome.shell "$key" "$updated"
    done
fi
systemctl --user disable --now keenud.service 2>/dev/null
pkill -x keenu-shelf 2>/dev/null

rm -rf "$DATA/gnome-shell/extensions/$UUID" "$DATA/doc/keenu" "$DATA/fonts/keenu"
fc-cache "$DATA/fonts" >/dev/null 2>&1
if [ "$SYSTEM" = 0 ]; then
    # The voice assistant's speech models (and an older setup's Python venv, up to 3 GB with CUDA), Keenu's
    # caches and its AIs' own homes.
    rm -rf "$DATA/keenu/models" "$DATA/keenu/voice-venv" "$DATA/keenu/keenu-whisper.py" "$DATA/keenu/assistant" "$DATA/keenu/codex" \
           "$DATA/keenu/agy" "${XDG_CACHE_HOME:-$HOME/.cache}/keenu" "${XDG_RUNTIME_DIR:-/nonexistent}/keenu-covers"
    # The battery charge limit, set with keenu-charge-limit, would outlive Keenu: lifted (asks for the password).
    if [ -e /etc/systemd/system/keenu-charge-limit.service ]; then
        echo "Lifting the battery charge limit Keenu set (sudo asks for your password)."
        sudo systemctl disable --now keenu-charge-limit.service 2>/dev/null
        sudo rm -f /etc/systemd/system/keenu-charge-limit.service
        for file in /sys/class/power_supply/BAT*/charge_control_end_threshold; do
            [ -e "$file" ] && echo 100 | sudo tee "$file" >/dev/null
        done
    fi
fi
# The AI alerts' hooks in Claude Code's and Codex's own settings would run an keenud that is gone.
[ -x "$HOME/.local/bin/keenud" ] && "$HOME/.local/bin/keenud" --unhook-all 2>/dev/null
rm -f "$HOME/.local/bin/keenu" "$HOME/.local/bin/keenud" "$HOME/.local/bin/keenu-shelf" "$HOME/.local/bin/keenu-whisper" "$HOME/.local/bin/keenu-charge-limit" \
      "$HOME/.local/bin/keenu-setup-voice" "$HOME/.local/bin/keenu-uninstall" \
      "$DATA/dbus-1/services/io.github.zickrian.isle.Daemon.service" \
      "$DATA/dbus-1/services/io.github.zickrian.isle.ShelfWindow.service" \
      "$DATA/dbus-1/services/io.github.isle.Daemon.service" \
      "$DATA/dbus-1/services/io.github.isle.ShelfWindow.service" \
      "$CONFIG/systemd/user/keenud.service"
systemctl --user daemon-reload 2>/dev/null
if [ "$SYSTEM" = 1 ]; then
    echo "Keenu in your home folder has been removed; the system-wide package now runs instead (remove it with apt, dnf or pacman)."
    exit 0
fi
echo "Keenu has been removed."
HUB="${HF_HUB_CACHE:-${HF_HOME:-${XDG_CACHE_HOME:-$HOME/.cache}/huggingface}/hub}"
if ls -d "$HUB"/models--Systran--faster-whisper-* >/dev/null 2>&1; then
    echo "An older speech model stays in $HUB (models--Systran--faster-whisper-*); delete it to free its space."
fi
