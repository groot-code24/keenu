// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Shows keenud's reminders when they are due. keenud keeps them until shown, so one that fell due
// while the screen was locked (and Keenu was off) shows as soon as Keenu is back.

import GLib from 'gi://GLib';

/** Waits are cut into naps this long: GLib's clock stops during suspend, the wall clock does not. */
const MAX_NAP_MS = 30 * 1000;

/** Watches keenud's reminder list and hands each due one to the island once. */
export class ReminderWatcher {
    /** `onDue({id, text}, done)` shows a reminder that is due, and calls `done` once it was on screen. */
    constructor(daemon, onDue) {
        this._daemon = daemon;
        this._onDue = onDue;
        this._timeout = 0;
        this._shown = new Set();
        this._changedId = daemon.connect('changed', (_daemon, key) => {
            if (key === 'reminders')
                this._sync();
        });
        this._sync();
    }

    /** Shows what is due now and sets a timer for the next one. */
    _sync() {
        if (this._timeout) {
            GLib.source_remove(this._timeout);
            this._timeout = 0;
        }
        const items = this._daemon.state.reminders ?? [];
        // Only those keenud still has: the rest were dismissed for good.
        this._shown = new Set([...this._shown].filter(id => items.some(item => item.id === id)));
        const now = Date.now();
        for (const item of items) {
            // Dismissing takes a round trip; don't show it twice meanwhile.
            if (item.due_ms > now || this._shown.has(item.id))
                continue;
            this._shown.add(item.id);
            // keenud keeps it until it was really shown (the island may be hidden by a fullscreen window).
            this._onDue(item, () => this._daemon.call('reminders', 'Dismiss', item.id));
        }
        const next = items.find(item => item.due_ms > now);
        if (!next)
            return;
        const wait = Math.min(MAX_NAP_MS, next.due_ms - now + 50);
        this._timeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, wait, () => {
            this._timeout = 0;
            this._sync();
            return GLib.SOURCE_REMOVE;
        });
    }

    /** Stops the timer and lets go of keenud's signal. */
    destroy() {
        if (this._timeout)
            GLib.source_remove(this._timeout);
        this._daemon.disconnect(this._changedId);
    }
}
