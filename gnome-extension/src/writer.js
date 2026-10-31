// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Writes what the voice assistant was asked to write into the window in front: short text typed key by
// key through a virtual keyboard, longer text pasted through the clipboard, which is then put back.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import St from 'gi://St';

import {isTerminal, onLatinLayout} from './clipboard.js';
import {clutterBackend} from './compat.js';

const CLIPBOARD = St.ClipboardType.CLIPBOARD;
/** Longer text is pasted, as is text with a newline: Return would send a chat message. */
const MAX_TYPED = 200;
/** Time for the app to read the pasted text before the clipboard is put back. */
const RESTORE_MS = 1000;
/** Set by password managers on secrets; those are not put back, so they still expire. */
const SECRET_HINT = 'x-kde-passwordManagerHint';
/** Clipboard contents put back as they were; anything else is put back as text. */
const RICH_TYPES = ['x-special/gnome-copied-files', 'text/uri-list', 'image/png'];
// Whether keys typed now come out as `text`: plain ASCII, no Caps Lock, a Latin layout and no input
// method composing.
function canType(text) {
    if (text.length > MAX_TYPED || !/^[\x20-\x7e]+$/.test(text))
        return false;
    try {
        if (clutterBackend().get_default_seat().get_keymap().get_caps_lock_state())
            return false;
        return onLatinLayout();
    } catch {
        return false;
    }
}

/** Types or pastes assistant text into the focused window. */
export class TextWriter {
    // `history` is the ClipboardHistory: told to leave out what is put on the clipboard here, and
    // lending its virtual keyboard (made at start, as a new one drops the first keys it gets).
    constructor(history) {
        this._history = history;
        this._clipboard = St.Clipboard.get_default();
        this._timeouts = new Set();
    }

    /** Writes `text` into `target`, a window, if it is still the one in front. */
    write(text, target) {
        try {
            if (!text || this._destroyed || !this._isFront(target))
                return;
            if (isTerminal(target)) {
                console.warn('Keenu: not writing into a terminal, where a newline could run a command');
                return;
            }
            if (canType(text))
                this._type(text);
            else
                this._paste(text.replace(/\r\n?/g, '\n'), target);
        } catch (e) {
            console.warn(`Keenu: could not write the text: ${e.message}`);
        }
    }

    // Keys reach the window only while it has the focus and nothing in the shell has the keyboard.
    _isFront(target) {
        const focus = global.stage.get_key_focus();
        return Boolean(target) && global.display.focus_window === target && (!focus || focus === global.stage);
    }

    // Sends each character as a key press and release on the shared virtual keyboard.
    _type(text) {
        const keyboard = this._history.keyboard;
        for (const char of text) {
            // In printable ASCII a keysym is the character's own code.
            const keyval = char.codePointAt(0);
            keyboard.notify_keyval(GLib.get_monotonic_time(), keyval, Clutter.KeyState.PRESSED);
            keyboard.notify_keyval(GLib.get_monotonic_time(), keyval, Clutter.KeyState.RELEASED);
        }
    }

    // Puts `text` on the clipboard, presses Ctrl+V, and soon after puts back what was there before.
    _paste(text, target) {
        const mimetypes = this._clipboard.get_mimetypes(CLIPBOARD);
        const paste = previous => {
            if (this._destroyed || !this._isFront(target))
                return;
            this._history.ignoreNextCopy();
            this._clipboard.set_text(CLIPBOARD, text);
            this._history.pressPaste(false);
            if (previous)
                this._later(RESTORE_MS, () => this._restore(text, previous));
        };
        if (mimetypes.length === 0 || mimetypes.includes(SECRET_HINT)) {
            paste(null);
            return;
        }
        const rich = RICH_TYPES.find(type => mimetypes.includes(type));
        if (rich) {
            this._clipboard.get_content(CLIPBOARD, rich, (_clipboard, bytes) =>
                this._guard(() => paste(bytes?.get_size() ? {mimetype: rich, bytes} : null)));
        } else {
            this._clipboard.get_text(CLIPBOARD, (_clipboard, previous) =>
                this._guard(() => paste(previous !== null ? {text: previous} : null)));
        }
    }

    // Unless something else was copied meanwhile.
    _restore(text, previous) {
        this._clipboard.get_text(CLIPBOARD, (_clipboard, now) => this._guard(() => {
            if (now !== text || this._destroyed)
                return;
            this._history.ignoreNextCopy();
            if (previous.text !== undefined)
                this._clipboard.set_text(CLIPBOARD, previous.text);
            else
                this._clipboard.set_content(CLIPBOARD, previous.mimetype, previous.bytes);
        }));
    }

    // Clipboard callbacks come from GLib; nothing may throw out of them.
    _guard(callback) {
        try {
            callback();
        } catch (e) {
            console.warn(`Keenu: could not write the text: ${e.message}`);
        }
    }

    // Runs `callback` once after `ms`, cancelled on destroy.
    _later(ms, callback) {
        const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            this._timeouts.delete(id);
            this._guard(callback);
            return GLib.SOURCE_REMOVE;
        });
        this._timeouts.add(id);
    }

    /** Stops any pending clipboard restore. */
    destroy() {
        this._destroyed = true;
        this._timeouts.forEach(id => GLib.source_remove(id));
        this._timeouts.clear();
    }
}
