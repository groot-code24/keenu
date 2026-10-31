// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Turns Do Not Disturb on for focus sessions, and only ever undoes what it did (see focusRules.js).
// That is saved, so it survives the lock screen (DND stays on) and shell crashes.

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import {focusSession, focusStep} from './focusRules.js';

/** Keeps Do Not Disturb in step with the focus timer. */
export class FocusMode {
    /** Follows the pomodoro phase, the DND switch and the focus-dnd setting. */
    constructor(daemon, notifications, settings) {
        this._daemon = daemon;
        this._notifications = notifications;
        this._settings = settings;
        this._changing = false;
        // Turned off by hand while Keenu was not running: it is theirs again.
        if (!notifications.dnd)
            this._owned = false;

        this._daemonId = daemon.connect('changed', (_daemon, key) => {
            if (key === 'pomodoro')
                this._sync();
        });
        this._dndId = notifications.connect('dnd-changed', () => {
            // The user turned it off themselves: it is theirs again, and stays off for the rest of the session.
            if (this._changing || notifications.dnd)
                return;
            this._owned = false;
            const session = this._session();
            if (session !== null)
                this._declined = session;
        });
        this._settingsId = settings.connect('changed::focus-dnd', () => this._sync());
        this._sync();
    }

    /** Whether Keenu turned Do Not Disturb on and owes turning it off. */
    get _owned() {
        return this._settings.get_boolean('focus-dnd-owned');
    }

    /** Saves the claim, writing only when it changes. */
    set _owned(owned) {
        if (owned !== this._owned)
            this._settings.set_boolean('focus-dnd-owned', owned);
    }

    /** The session (its start) the user turned Do Not Disturb off in, saved past the lock screen. */
    get _declined() {
        return this._settings.get_int64('focus-dnd-declined');
    }

    /** Saves it, writing only when it changes. */
    set _declined(session) {
        if (session !== this._declined)
            this._settings.set_int64('focus-dnd-declined', session);
    }

    /** The focus session under way that wants Do Not Disturb (see `focusSession`), or null. */
    _session() {
        return this._settings.get_boolean('focus-dnd') ? focusSession(this._daemon.state.pomodoro) : null;
    }

    /** Turns DND on as a focus session starts and off when Keenu's own claim ends. */
    _sync() {
        // Not known until keenud answers; don't switch anything off before.
        if (this._daemon.state.pomodoro?.phase === undefined)
            return;
        const step = focusStep({session: this._session(), dnd: this._notifications.dnd, owned: this._owned,
            declined: this._declined});
        this._owned = step.owned;
        this._declined = step.declined;
        if (step.dnd !== null)
            this._setDnd(step.dnd);
    }

    /** Sets DND without mistaking the change for the user's own. */
    _setDnd(on) {
        this._changing = true;
        this._notifications.dnd = on;
        this._changing = false;
    }

    /** Disconnects, and gives DND back unless the screen is locked. */
    destroy() {
        this._daemon.disconnect(this._daemonId);
        this._notifications.disconnect(this._dndId);
        this._settings.disconnect(this._settingsId);
        // Stay quiet on the lock screen during a focus session; otherwise
        // leave the desktop as the user had it.
        if (this._owned && !Main.sessionMode.isLocked) {
            this._owned = false;
            this._setDnd(false);
        }
    }
}
