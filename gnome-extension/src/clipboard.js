// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Clipboard history: hands each copy to keenud, which keeps the list (Clipboard1 in docs/dbus-api.md),
// and puts items back or pastes them. Lives here: only GNOME Shell can read every app's clipboard on Wayland.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {clutterBackend} from './compat.js';

const CLIPBOARD = St.ClipboardType.CLIPBOARD;
/** Set by password managers (KeePassXC and others) on secrets they copy. */
const SECRET_HINT = 'x-kde-passwordManagerHint';
/** A secret that could not be read stands for any copy this soon after it (µs): better one copy left out. */
const UNREAD_SECRET_US = 30 * 1000 * 1000;
const UNREAD = 'unread';
/** How Files puts copied files on the clipboard. */
const FILES_TYPE = 'x-special/gnome-copied-files';
const URI_LIST_TYPE = 'text/uri-list';
const IMAGE_TYPE = 'image/png';
/** Plain text as St reads it, best first. */
const TEXT_TYPES = ['text/plain;charset=utf-8', 'UTF8_STRING', 'text/plain', 'STRING'];
/** Larger copies are not kept (keenud refuses them too). */
const MAX_TEXT_BYTES = 512 * 1024;
const MAX_IMAGE_BYTES = 16 * 1024 * 1024;
/** A password manager clears what it copied this soon after; the item then leaves the history too. */
const SECRET_CLEARED_MS = 90 * 1000;
/** Time for the window in front to get the keyboard back before pasting. */
const PASTE_DELAY_MS = 120;
/** Terminals paste with Ctrl+Shift+V. Matched against the window's class and app id. */
const TERMINALS = ['terminal', 'console', 'kgx', 'ptyxis', 'kitty', 'alacritty', 'wezterm', 'konsole',
    'xterm', 'rxvt', 'tilix', 'terminator', 'foot', 'blackbox', 'ghostty', 'warp', 'tabby', 'hyper',
    'guake', 'tilda'];
/** Keyboard layouts whose keys give Latin letters (Dvorak and Colemak are variants of these). */
const LATIN_LAYOUTS = ['us', 'gb', 'id', 'de', 'fr', 'es', 'it', 'pt', 'br', 'nl', 'be', 'ch', 'at', 'se', 'no',
    'dk', 'fi', 'is', 'pl', 'cz', 'sk', 'hu', 'ro', 'hr', 'si', 'tr', 'ee', 'lv', 'lt', 'ie', 'ca', 'latam', 'au',
    'za', 'nz', 'ph', 'my', 'vn', 'epo'];

/** Whether `window` is a terminal, which pastes with Ctrl+Shift+V. */
export function isTerminal(window) {
    const app = Shell.WindowTracker.get_default().get_window_app(window);
    const names = [window.get_wm_class(), window.get_wm_class_instance(), app?.get_id()]
        .map(name => name?.toLowerCase() ?? '').join(' ');
    return TERMINALS.some(name => names.includes(name));
}

/** The input source in use, [type, id] as GNOME keeps them: the most recently used one comes first. */
function currentInputSource() {
    const settings = new Gio.Settings({schema_id: 'org.gnome.desktop.input-sources'});
    const [first] = settings.get_value('mru-sources').deep_unpack();
    return first ?? settings.get_value('sources').deep_unpack()[0] ?? ['xkb', 'us'];
}

/** Whether the keyboard layout in use types Latin letters (an input method counts as not). */
export function onLatinLayout() {
    try {
        const [type, id] = currentInputSource();
        return type === 'xkb' && LATIN_LAYOUTS.includes(id.split(/[+(]/)[0]);
    } catch {
        return true;
    }
}

/** File URIs on the clipboard as Files or a URI list describes them. */
function parseUris(text, mimetype) {
    const lines = text.split(/\r?\n/).map(line => line.trim()).filter(Boolean);
    // Files starts its list with "copy" or "cut".
    const uris = mimetype === FILES_TYPE ? lines.slice(1) : lines.filter(line => !line.startsWith('#'));
    return uris.filter(uri => uri.startsWith('file://'));
}

/** Linux input key codes for Ctrl, Shift and V, for layouts whose V key types something else. */
const KEY_LEFTCTRL = 29;
const KEY_LEFTSHIFT = 42;
const KEY_V = 47;

/** Watches the clipboard for the history, and puts items back and pastes them. */
export class ClipboardHistory {
    /** Watches from the start; `enabled` decides whether a copy is kept. */
    constructor(daemon, settings) {
        this._daemon = daemon;
        this._settings = settings;
        this._clipboard = St.Clipboard.get_default();
        /** Set while we put something on the clipboard ourselves. */
        this._ownCopy = false;
        this._timeouts = new Set();
        this._cancellable = new Gio.Cancellable();
        /** The text last added, {id, at}, or a secret's salted hash, {secret}; until something else is copied. */
        this._lastText = null;
        this._salt = GLib.uuid_string_random();
        // Made now: a new virtual device joins the seat a moment later, and drops the first paste.
        this._keyboard = clutterBackend().get_default_seat().create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);

        this._selection = global.display.get_selection();
        this._ownerChanged = this._selection.connect('owner-changed', (_selection, type) => {
            if (type === Meta.SelectionType.SELECTION_CLIPBOARD)
                this._onCopied();
        });
    }

    /** On in Settings, and its Shelf page in the island: nothing kept that can't be seen. */
    get enabled() {
        return this._settings.get_boolean('clipboard-history') && !this._settings.get_strv('hidden-pages').includes('shelf');
    }

    /** A copy was made: kept unless it is ours, a secret, or from an app the user left out. */
    _onCopied() {
        if (this._ownCopy) {
            this._ownCopy = false;
            return;
        }
        if (!this.enabled)
            return;
        const mimetypes = this._clipboard.get_mimetypes(CLIPBOARD);
        if (mimetypes.includes(SECRET_HINT)) {
            this._noteSecret(mimetypes);
            return;
        }
        const app = Shell.WindowTracker.get_default().focus_app;
        // Windows without a desktop file get made-up ids ("window:5").
        const appId = app && !app.is_window_backed() ? app.get_id() : '';
        if (this._settings.get_strv('clipboard-ignored-apps').includes(appId))
            return;
        this._addCopy(mimetypes, appId).catch(e => console.warn(`Keenu: could not keep a copy: ${e.message}`));
    }

    // Files first; then plain text (a spreadsheet's cells come with a picture of them too), else an image.
    async _addCopy(mimetypes, appId) {
        const filesType = [FILES_TYPE, URI_LIST_TYPE].find(type => mimetypes.includes(type));
        if (filesType) {
            const bytes = await this._read(filesType, MAX_TEXT_BYTES);
            if (this._destroyed)
                return;
            const uris = bytes ? parseUris(decode(bytes), filesType) : [];
            if (uris.length > 0) {
                this._lastText = null;
                this._daemon.call('clipboard', 'AddFiles', uris, appId);
                return;
            }
            // e.g. a link dragged from a browser; kept as text
        }
        const textType = TEXT_TYPES.find(type => mimetypes.includes(type));
        const bytes = textType ? await this._read(textType, MAX_TEXT_BYTES) : null;
        const text = bytes ? decode(bytes) : '';
        // Only the picture's address (a browser's Copy Image gives it as text too) yields to the picture.
        const address = mimetypes.includes(IMAGE_TYPE) && /^\s*(https?|data|file):\S*\s*$/.test(text);
        if (text.replaceAll('\0', '').trim() && !address) {
            const last = this._lastText;
            const hash = last?.secret ? await last.secret : null;
            const unread = hash === UNREAD && GLib.get_monotonic_time() - last.at < UNREAD_SECRET_US;
            if (!last?.secret || (!unread && hash !== this._hash(text)))
                this._addText(text, appId);
        } else if (mimetypes.includes(IMAGE_TYPE)) {
            const png = await this._read(IMAGE_TYPE, MAX_IMAGE_BYTES);
            if (png?.get_size() && !this._destroyed) {
                this._lastText = null;
                this._daemon.call('clipboard', 'AddImage', png.get_data(), appId);
            }
        } else if (mimetypes.length === 0 || (bytes && !text.replaceAll('\0', ''))) {
            this._forgetSecret();
        }
    }

    // A secret is never kept, only its hash: when the password manager quits, GNOME puts back what it
    // held as plain text, without the hint, and that copy must still be left out.
    _noteSecret(mimetypes) {
        const textType = TEXT_TYPES.find(type => mimetypes.includes(type));
        const read = () => textType ? this._read(textType, MAX_TEXT_BYTES) : Promise.resolve(null);
        // Read again once if it failed; still unread, the next copies a moment later are taken for it.
        const secret = read().then(bytes => bytes ?? read()).then(bytes => bytes ? this._hash(decode(bytes)) : UNREAD);
        this._lastText = {secret, at: GLib.get_monotonic_time()};
    }

    /** `text`'s hash, salted afresh each session: a secret can be recognised but not looked up. */
    _hash(text) {
        return GLib.compute_checksum_for_data(GLib.ChecksumType.SHA256, new TextEncoder().encode(this._salt + text));
    }

    /** Remembered as the last text, so a password manager clearing it soon after can take it out again. */
    async _addText(text, appId) {
        if (this._destroyed)
            return;
        this._lastText = null;
        const [id] = await this._daemon.call('clipboard', 'AddText', text, appId) ?? [];
        if (id !== undefined)
            this._lastText = {id, at: Date.now()};
    }

    // The clipboard emptied soon after text was copied: a password manager clearing a password, which then
    // leaves the history too, unless pinned. Whatever app is in front: it clears on a timer, after the paste.
    _forgetSecret() {
        const last = this._lastText;
        this._lastText = null;
        // After a secret, nothing of the history's was on the clipboard to take out.
        if (last?.id === undefined || Date.now() - last.at > SECRET_CLEARED_MS || this._destroyed)
            return;
        const item = (this._daemon.state.clipboard ?? []).find(other => other.id === last.id);
        if (item && !item.pinned)
            this._daemon.call('clipboard', 'Remove', last.id);
    }

    /** At most `max` bytes of the clipboard's `mimetype`; null if there is more, or it could not be read. */
    _read(mimetype, max) {
        return new Promise(resolve => {
            const stream = Gio.MemoryOutputStream.new_resizable();
            // Read only one byte past the limit: a huge copy never comes into the shell whole.
            this._selection.transfer_async(Meta.SelectionType.SELECTION_CLIPBOARD, mimetype, max + 1, stream,
                this._cancellable, (selection, result) => {
                    try {
                        selection.transfer_finish(result);
                        stream.close(null);
                        const bytes = stream.steal_as_bytes();
                        resolve(bytes.get_size() > max ? null : bytes);
                    } catch (e) {
                        if (!e.matches(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                            console.warn(`Keenu: could not read the clipboard's ${mimetype}: ${e.message}`);
                        resolve(null);
                    }
                });
        });
    }

    // Puts `item` (from `Daemon.state.clipboard`) back on the clipboard. `touch: false` leaves it in place
    // in the history, for callers that move it later so cards don't jump under the pointer.
    async copy(item, {touch = true} = {}) {
        let bytes = null, mimetype = null, text = null;
        switch (item.kind) {
        case 'image':
            try {
                bytes = await loadBytes(item.image);
                mimetype = IMAGE_TYPE;
            } catch (e) {
                console.warn(`Keenu: could not read ${item.image}: ${e.message}`);
                return false;
            }
            break;
        case 'files':
            bytes = GLib.Bytes.new(new TextEncoder().encode(['copy', ...item.uris].join('\n')));
            mimetype = FILES_TYPE;
            break;
        default: {
            const result = await this._daemon.call('clipboard', 'GetText', item.id);
            text = result?.[0];
            if (text === undefined || this._destroyed)
                return false;
        }
        }

        this.ignoreNextCopy();
        if (text !== null)
            this._clipboard.set_text(CLIPBOARD, text);
        else
            this._clipboard.set_content(CLIPBOARD, mimetype, bytes);
        if (touch)
            this._daemon.call('clipboard', 'Touch', item.id);
        return true;
    }

    /** Leaves the next change of the clipboard out of the history; call right before making it. */
    ignoreNextCopy() {
        this._ownCopy = true;
        // If the clipboard did not change hands (same content), don't skip the user's next copy.
        this._later(500, () => (this._ownCopy = false));
    }

    /** Pastes into the window in front, once it has the keyboard again (call after the island lets go). */
    paste() {
        this._later(PASTE_DELAY_MS, () => {
            // Only into an app window: otherwise the keys would land in the wrong place. The item stays copied.
            const focus = global.stage.get_key_focus();
            if (!global.display.focus_window || (focus && focus !== global.stage))
                return;
            this.pressPaste(isTerminal(global.display.focus_window));
        });
    }

    /** The shared virtual keyboard, for typing into the window in front. */
    get keyboard() {
        return this._keyboard;
    }

    // Presses Ctrl+V (Ctrl+Shift+V in a terminal): by symbol on a Latin layout (right on Dvorak, or
    // with Ctrl and Caps Lock swapped), by key code on others, where the V key types another letter.
    pressPaste(terminal) {
        const time = GLib.get_monotonic_time();
        if (onLatinLayout()) {
            const keys = terminal ? [Clutter.KEY_Control_L, Clutter.KEY_Shift_L, Clutter.KEY_v] : [Clutter.KEY_Control_L, Clutter.KEY_v];
            keys.forEach(key => this._keyboard.notify_keyval(time, key, Clutter.KeyState.PRESSED));
            [...keys].reverse().forEach(key => this._keyboard.notify_keyval(time, key, Clutter.KeyState.RELEASED));
        } else {
            const codes = terminal ? [KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V] : [KEY_LEFTCTRL, KEY_V];
            codes.forEach(code => this._keyboard.notify_key(time, code, Clutter.KeyState.PRESSED));
            [...codes].reverse().forEach(code => this._keyboard.notify_key(time, code, Clutter.KeyState.RELEASED));
        }
    }

    /** A timeout that `destroy` removes if it hasn't run. */
    _later(ms, callback) {
        if (this._destroyed)
            return;
        const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            this._timeouts.delete(id);
            callback();
            return GLib.SOURCE_REMOVE;
        });
        this._timeouts.add(id);
    }

    /** Stops reading, drops pending timeouts and frees the virtual keyboard. */
    destroy() {
        this._destroyed = true;
        this._cancellable.cancel();
        this._selection.disconnect(this._ownerChanged);
        this._timeouts.forEach(id => GLib.source_remove(id));
        this._timeouts.clear();
        this._keyboard?.run_dispose();
        this._keyboard = null;
    }
}

/** Clipboard bytes as text, UTF-8. */
function decode(bytes) {
    return new TextDecoder().decode(bytes.get_data() ?? new Uint8Array());
}

/** The file at `path`, read off the main loop's way. */
function loadBytes(path) {
    return new Promise((resolve, reject) => {
        Gio.File.new_for_path(path).load_contents_async(null, (file, result) => {
            try {
                resolve(GLib.Bytes.new(file.load_contents_finish(result)[1]));
            } catch (e) {
                reject(e);
            }
        });
    });
}
