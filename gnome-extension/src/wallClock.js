// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The time of day as GNOME's own clock keeps it: each minute, on waking from suspend, and when the time
// zone or the 12/24-hour setting changes. One clock for the whole island, made while anything watches it.

import GnomeDesktop from 'gi://GnomeDesktop';

let clock = null;
let watchers = 0;

/** Calls `callback` whenever the time shown should change; returns how to stop. */
export function watchClock(callback) {
    clock ??= new GnomeDesktop.WallClock({time_only: true});
    watchers++;
    const id = clock.connect('notify::clock', () => callback());
    let stopped = false;
    return () => {
        if (stopped)
            return;
        stopped = true;
        clock.disconnect(id);
        if (--watchers === 0) {
            clock.run_dispose();
            clock = null;
        }
    };
}
