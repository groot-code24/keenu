#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Copyright (C) 2026 zickrian
# Sets up speech recognition for Keenu's voice assistant: downloads the whisper.cpp model, checking its SHA-256.
# Usage: keenu-setup-voice [tiny|base|small|medium] [--remove-old]
#   The model given (then also chosen in Settings → Assistant), else the one chosen there, else small.
#   --remove-old  also deletes what the older Python setup left: its venv and faster-whisper models.
set -euo pipefail

DATA="${XDG_DATA_HOME:-$HOME/.local/share}/keenu"
MODELS="$DATA/models"
SCHEMAS="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/isle@zickrian.github.io/schemas"
WHISPER_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"
VAD_URL="https://huggingface.co/ggml-org/whisper-vad/resolve/main"
# What an older Keenu's faster-whisper setup left.
OLD_VENV="$DATA/voice-venv"
OLD_HELPER="$DATA/keenu-whisper.py"
HUB="${HF_HUB_CACHE:-${HF_HOME:-${XDG_CACHE_HOME:-$HOME/.cache}/huggingface}/hub}"

# A step's heading, in bold.
step() { printf '\033[1m==> %s\033[0m\n' "$*"; }
# Says what went wrong and stops.
fail() { printf 'keenu-setup-voice: %s\n' "$*" >&2; exit 1; }

MODEL=""
GIVEN=0
REMOVE_OLD=0
for arg in "$@"; do
    case "$arg" in
        tiny | base | small | medium) MODEL="$arg" GIVEN=1 ;;
        --remove-old) REMOVE_OLD=1 ;;
        -h | --help) sed -n '4,7s/^# \{0,1\}//p' "$0"; exit 0 ;;
        *) fail "unknown argument '$arg' (a model is tiny, base, small or medium)" ;;
    esac
done
if [ -z "$MODEL" ]; then
    # Keenu in the home folder, else a system package's (its schema is the system's).
    MODEL="$({ gsettings --schemadir "$SCHEMAS" get org.gnome.shell.extensions.isle voice-speech-model ||
        gsettings get org.gnome.shell.extensions.isle voice-speech-model; } 2>/dev/null | tr -d "'" || true)"
fi
MODEL="${MODEL:-small}"

# The official whisper.cpp files (8-bit), as keenu-whisper names them: file, SHA-256, size.
case "$MODEL" in
    tiny) set -- ggml-tiny-q8_0.bin c2085835d3f50733e2ff6e4b41ae8a2b8d8110461e18821b09a15c40c42d1cca 43537433 ;;
    base) set -- ggml-base-q8_0.bin c577b9a86e7e048a0b7eada054f4dd79a56bbfa911fbdacf900ac5b567cbb7d9 81768585 ;;
    small) set -- ggml-small-q8_0.bin 49c8fb02b65e6049d5fa6c04f81f53b867b5ec9540406812c643f177317f779f 264464607 ;;
    medium) set -- ggml-medium-q8_0.bin 42a1ffcbe4167d224232443396968db4d02d4e8e87e213d3ee2e03095dea6502 823369779 ;;
    *) fail "Settings name an unknown model '$MODEL'; give one: keenu-setup-voice small" ;;
esac
FILE="$1" SHA="$2" SIZE="$3"
# Silero, which finds where someone speaks so Whisper hears only that.
VAD_FILE=ggml-silero-v5.1.2.bin
VAD_SHA=29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf

command -v curl >/dev/null || fail "please install curl first."
command -v sha256sum >/dev/null || fail "please install sha256sum (coreutils) first."

# Whether file $1 is there with SHA-256 $2.
verified() { [ -f "$1" ] && [ "$(sha256sum "$1" | cut -d' ' -f1)" = "$2" ]; }

# Downloads $1 to $MODELS/$2 unless it is there already, and checks it is the file published.
fetch() {
    local url="$1" file="$MODELS/$2" sha="$3"
    if verified "$file" "$sha"; then
        echo "Already downloaded: $file"
        return
    fi
    # A .part file is carried on from where an interrupted download stopped.
    curl -fL --retry 3 --progress-bar -C - -o "$file.part" "$url" ||
        curl -fL --retry 3 --progress-bar -o "$file.part" "$url" ||
        fail "could not download $url; check the internet connection and try again."
    if ! verified "$file.part" "$sha"; then
        rm -f "$file.part"
        fail "$2 did not download correctly (its SHA-256 differs); run keenu-setup-voice again."
    fi
    mv -f "$file.part" "$file"
}

mkdir -p "$MODELS"
step "Downloading the '$MODEL' speech model ($((SIZE / 1000000)) MB) to $MODELS"
fetch "$WHISPER_URL/$FILE" "$FILE" "$SHA"
fetch "$VAD_URL/$VAD_FILE" "$VAD_FILE" "$VAD_SHA"
# keenud runs the model chosen in Settings, so a model named here becomes that choice.
if [ "$GIVEN" = 1 ]; then
    if { gsettings --schemadir "$SCHEMAS" set org.gnome.shell.extensions.isle voice-speech-model "$MODEL" ||
        gsettings set org.gnome.shell.extensions.isle voice-speech-model "$MODEL"; } 2>/dev/null; then
        echo "Settings → Assistant → Speech recognition is now '$MODEL'."
    else
        echo "Pick '$MODEL' in Settings → Assistant → Speech recognition so Keenu uses it."
    fi
fi

# keenu-whisper beside this script (both in ~/.local/bin), on the PATH, or built in the source folder.
HERE="$(dirname "$(readlink -f "$0")")"
WHISPER=""
for candidate in "$HERE/keenu-whisper" "$(command -v keenu-whisper || true)" "$HERE/../target/release/keenu-whisper"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then
        WHISPER="$candidate"
        break
    fi
done
if [ -z "$WHISPER" ]; then
    echo "keenu-whisper, which runs the model, is not installed: install Keenu again" \
        "(from source it needs cmake and a C++ compiler; see the README)."
fi

# The older setup, which keenu-whisper replaces: only offered once keenu-whisper is there to take over.
OLD=()
[ -d "$OLD_VENV" ] && OLD+=("$OLD_VENV")
[ -f "$OLD_HELPER" ] && OLD+=("$OLD_HELPER")
for size in tiny base small medium; do
    [ -d "$HUB/models--Systran--faster-whisper-$size" ] && OLD+=("$HUB/models--Systran--faster-whisper-$size")
done
if [ -n "$WHISPER" ] && [ "${#OLD[@]}" -gt 0 ]; then
    step "The older Python speech setup is no longer needed"
    # -L: the model files can be links into a shared cache.
    du -shL "${OLD[@]}" 2>/dev/null || true
    if [ "$REMOVE_OLD" = 0 ] && [ -t 0 ] && [ -t 1 ]; then
        read -r -p "Delete them? [y/N] " answer
        case "$answer" in [yY]*) REMOVE_OLD=1 ;; esac
    fi
    if [ "$REMOVE_OLD" = 1 ]; then
        for old in "${OLD[@]}"; do
            # A model file linked elsewhere in the cache goes too, not only the link.
            if [ -d "$old" ] && [ "${old#"$HUB"/}" != "$old" ]; then
                find "$old" -type l -print0 | while IFS= read -r -d '' link; do
                    target="$(readlink -f "$link" || true)"
                    case "$target" in "$HUB"/*) rm -f "$target" ;; esac
                done
            fi
            rm -rf "$old"
        done
        echo "Deleted."
    else
        echo "To delete them later: keenu-setup-voice --remove-old"
    fi
fi

# What keenud itself finds: the speech model, where it runs, and the AI it answers with.
step "Checking"
ISLED="$(command -v keenud || echo "$HOME/.local/bin/keenud")"
if [ -x "$ISLED" ]; then
    "$ISLED" --doctor
else
    echo "keenud is not installed yet; install Keenu, then run keenud --doctor."
fi
echo "Done. Pick the AI in Settings → Assistant if you have more than one, then press Super+I and speak."
