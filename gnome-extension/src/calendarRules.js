// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The Calendar page's rules that need nothing of GNOME, kept apart so tests run them anywhere (tests/).

/** A desktop reminder Keenu's alert stands in for comes at most this long before the alert, or after the start. */
export const REMINDER_EARLY_MS = 30 * 60 * 1000;
export const REMINDER_LATE_MS = 15 * 60 * 1000;

/** One [begin, end) holding each of `ranges` ([begin, end) Dates, or null); null when there are none. */
export function spanOf(ranges) {
    const known = ranges.filter(Boolean);
    if (!known.length)
        return null;
    return [new Date(Math.min(...known.map(([begin]) => begin))), new Date(Math.max(...known.map(([, end]) => end)))];
}

/** An event's occurrence as the alarms know it: GNOME's id (or the title) and its start in ms. */
export function occurrenceKey(event) {
    return `${event.id ?? event.title}@${event.start.getTime()}`;
}

/** A calendar reminder's id ("calendar\nevent\n[recurrence\n]alarm time": Evolution's, or since GNOME 51 the shell's)
 * as {calendar, uid, rid, alarmMs}; `rid` is null for a reminder of the whole event, `alarmMs` null when unknown. */
export function parseReminderId(id) {
    const lines = String(id ?? '').split('\n');
    const [calendar, uid] = lines;
    if (lines.length < 3 || !calendar || !uid)
        return null;
    const seconds = Number(lines.at(-1));
    return {calendar, uid, rid: lines.length > 3 ? lines[2] : null, alarmMs: seconds > 0 ? seconds * 1000 : null};
}

/** Whether Keenu's own alert stands in for `reminder` (see `parseReminderId`), among the `events` of today and
 * tomorrow: the same occurrence, a reminder about when its alert goes, and that alert still to come or shown. */
export function standsInFor(reminder, events, {leadMs, now, alarmed, shown}) {
    if (!reminder || !leadMs)
        return false;
    return events.some(event => {
        // GNOME's id: the calendar, the event and its recurrence (empty for a single one), a line each.
        const [calendar, uid, rid = ''] = event.id?.split('\n') ?? [];
        if (event.allDay || calendar !== reminder.calendar || uid !== reminder.uid ||
            (reminder.rid !== null && reminder.rid !== rid))
            return false;
        // An earlier heads-up (an hour, a day before) or one long after the start is the desktop's own.
        const ahead = event.start.getTime() - (reminder.alarmMs ?? now);
        if (ahead > leadMs + REMINDER_EARLY_MS || ahead <= -REMINDER_LATE_MS)
            return false;
        // Not alerted (begun before Keenu looked, or Do Not Disturb then): the desktop's reminder stays.
        const key = occurrenceKey(event);
        return shown.has(key) || (!alarmed.has(key) && event.start.getTime() > now);
    });
}
