// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The rules of an app usage session, apart from GNOME: which app is timed and for how long. The
// extension wires this to focus signals and keenud (see appUsage.js); `node --test` checks it here.
// Input, or its absence, never pauses the clock by itself: appUsage.js ends the session after five
// idle minutes, or keeps it while something holds the session awake.

/** A focused app's running time; `now` is a monotonic clock and `wall` the epoch clock, in milliseconds. */
export class AppUsageSession {
    constructor(now, wall = now) {
        this._now = now;
        this._wall = wall;
        this._id = null;
        this._name = '';
        this._since = 0;
        this._sinceWall = 0;
    }

    /**
     * Times `app` ({id, name}), or nothing with null. The same app keeps the session it has, so two
     * of its windows do not split the time. Returns the previous app's stretch, if any.
     */
    switchTo(app, now = this._now(), wall = this._wall()) {
        const id = app?.id ?? null;
        if (id === this._id)
            return null;
        const ended = this.flush(now, wall);
        this._id = id;
        this._name = app?.name ?? '';
        this._since = id === null ? 0 : now;
        this._sinceWall = id === null ? 0 : wall;
        return ended;
    }

    /** The time since the last report, in ms, and goes on timing; null when nothing is timed. */
    flush(now = this._now(), wall = this._wall()) {
        if (this._id === null)
            return null;
        const ms = Math.max(0, Math.round(now - this._since));
        const stretch = {id: this._id, name: this._name, start: this._sinceWall, ms};
        this._since = now;
        this._sinceWall = wall;
        return stretch;
    }

    /** Ends the session, returning its last stretch; nothing is timed afterwards. */
    end(now = this._now(), wall = this._wall()) {
        const ended = this.flush(now, wall);
        this._id = null;
        this._name = '';
        this._since = 0;
        this._sinceWall = 0;
        return ended;
    }
}
