// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// When a focus session silences notifications, needing nothing of GNOME, kept apart so tests run it anywhere (tests/).

/** No session in which the user turned Do Not Disturb off. */
export const NONE = -1;

/** The focus session under way, by when it started (epoch ms; 0 when keenud doesn't say), or null. A focus
 * phase waiting for "Start focus" after a break is none yet; one paused part way still is. */
export function focusSession(pomodoro) {
    if (pomodoro?.phase !== 'focus')
        return null;
    if (pomodoro.started_at_ms)
        return pomodoro.started_at_ms;
    return pomodoro.running || pomodoro.remaining_ms < pomodoro.duration_ms ? 0 : null;
}

/** What focus mode does now: Do Not Disturb `dnd` on (true), off (false) or left (null), with the claim it
 * keeps (`owned`) and the session the user turned it off in (`declined`), which stays quiet for the rest. */
export function focusStep({session, dnd, owned, declined}) {
    if (session === null)
        return {dnd: owned ? false : null, owned: false, declined: NONE};
    if (declined === session)
        return {dnd: null, owned, declined};
    if (!owned && !dnd)
        return {dnd: true, owned: true, declined: NONE};
    return {dnd: null, owned, declined: NONE};
}
