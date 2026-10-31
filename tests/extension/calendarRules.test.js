// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The Calendar page's rules that need no GNOME: `node --experimental-default-type=module --test tests/extension/*.test.js`.

import assert from 'node:assert/strict';
import {test} from 'node:test';

import {REMINDER_LATE_MS, occurrenceKey, parseReminderId, spanOf, standsInFor} from '../../gnome-extension/src/calendarRules.js';

const MINUTE = 60 * 1000;
const NOW = new Date(2026, 9, 28, 8, 45).getTime();
const at = (hours, minutes = 0, day = 28) => new Date(2026, 9, day, hours, minutes);
const seconds = date => Math.floor(date.getTime() / 1000);

/** A desktop event as the Calendar page lists it; GNOME's id is "calendar\nevent\nrecurrence". */
const event = (start, {uid = 'standup', rid = '', allDay = false} = {}) =>
    ({id: `work\n${uid}\n${rid}`, title: 'Standup', start, allDay});
/** What standsInFor knows of Keenu's alerts: a 5 minute lead, nothing alarmed or shown yet. */
const state = (more = {}) => ({leadMs: 5 * MINUTE, now: NOW, alarmed: new Set(), shown: new Set(), ...more});

test('the span asked for holds every range given', () => {
    assert.equal(spanOf([null, null]), null);
    const grid = [at(0, 0, 1), at(0, 0, 40)];
    const isle = [at(0, 0, 28), at(0, 0, 30)];
    assert.deepEqual(spanOf([grid, null]), grid);
    assert.deepEqual(spanOf([grid, isle]), grid);
    // Keenu on next month while GNOME's grid shows this one: both, so neither loses its events.
    const next = [at(0, 0, 32), at(0, 0, 62)];
    assert.deepEqual(spanOf([grid, next]), [grid[0], next[1]]);
    // The same span again for the same ranges, so GNOME's source sees no change.
    assert.equal(spanOf([grid, isle])[0].getTime(), spanOf([grid, isle])[0].getTime());
});

test('Evolution\'s reminder id is read with or without its recurrence', () => {
    assert.deepEqual(parseReminderId(`work\nstandup\n${seconds(at(8, 45))}`),
        {calendar: 'work', uid: 'standup', rid: null, alarmMs: at(8, 45).getTime()});
    assert.deepEqual(parseReminderId(`work\nstandup\n20261029T090000\n${seconds(at(9, 0))}`),
        {calendar: 'work', uid: 'standup', rid: '20261029T090000', alarmMs: at(9, 0).getTime()});
    // No alarm instance: Evolution writes -1.
    assert.equal(parseReminderId('work\nstandup\n-1').alarmMs, null);
    for (const id of [undefined, '', 'work', 'work\nstandup', '\nstandup\n1', 'work\n\n1'])
        assert.equal(parseReminderId(id), null);
});

test('a reminder of an event whose alert is still to come is Keenu\'s', () => {
    const reminder = parseReminderId(`work\nstandup\n${seconds(at(8, 45))}`);
    assert.equal(standsInFor(reminder, [event(at(9, 0))], state()), true);
    // Another event, another calendar, an all-day one, or alerts off: the desktop's.
    assert.equal(standsInFor(reminder, [event(at(9, 0), {uid: 'review'})], state()), false);
    assert.equal(standsInFor(parseReminderId(`home\nstandup\n${seconds(at(8, 45))}`), [event(at(9, 0))], state()), false);
    assert.equal(standsInFor(reminder, [event(at(0, 0), {allDay: true})], state()), false);
    assert.equal(standsInFor(reminder, [event(at(9, 0))], state({leadMs: 0})), false);
    assert.equal(standsInFor(null, [event(at(9, 0))], state()), false);
});

test('an earlier heads-up stays the desktop\'s', () => {
    // An hour before, with a 5 minute lead: not about when Keenu's alert goes.
    const hour = parseReminderId(`work\nstandup\n${seconds(at(8, 0))}`);
    assert.equal(standsInFor(hour, [event(at(9, 0))], state({now: at(8, 0).getTime()})), false);
    // Seen only later (Keenu started after it came): still an hour before by its own time.
    assert.equal(standsInFor(hour, [event(at(9, 0))], state()), false);
});

test('an event Keenu never alerted keeps its reminder', () => {
    // Logged in during the meeting: begun before Keenu looked, so no alert of Keenu's.
    const late = parseReminderId(`work\nstandup\n${seconds(at(8, 45))}`);
    const begun = state({now: at(9, 5).getTime()});
    assert.equal(standsInFor(late, [event(at(9, 0))], begun), false);
    // Alarmed but never on screen (Do Not Disturb then, or cleared at a lock): the desktop's reminder stays.
    const startsSoon = event(at(9, 0));
    const alarmed = new Set([occurrenceKey(startsSoon)]);
    const reminder = parseReminderId(`work\nstandup\n${seconds(at(8, 56))}`);
    assert.equal(standsInFor(reminder, [startsSoon], state({now: at(8, 56).getTime(), alarmed})), false);
    // On screen: one at the start is Keenu's too, but not one long after it.
    const shown = new Set(alarmed);
    const atStart = parseReminderId(`work\nstandup\n${seconds(at(9, 0))}`);
    assert.equal(standsInFor(atStart, [startsSoon], state({now: at(9, 0).getTime(), alarmed, shown})), true);
    const after = parseReminderId(`work\nstandup\n${seconds(new Date(at(9, 0).getTime() + REMINDER_LATE_MS))}`);
    assert.equal(standsInFor(after, [startsSoon], state({now: at(9, 15).getTime(), alarmed, shown})), false);
});

test('a recurring event\'s reminder is matched to its own occurrence', () => {
    const today = event(at(9, 0), {rid: '20261028T090000'});
    const tomorrow = event(at(9, 0, 29), {rid: '20261029T090000'});
    // "A day before" tomorrow's, coming as today's starts: not today's to take.
    const dayBefore = parseReminderId(`work\nstandup\n20261029T090000\n${seconds(at(9, 0))}`);
    const now = state({now: at(9, 0).getTime() - MINUTE});
    assert.equal(standsInFor(dayBefore, [today, tomorrow], now), false);
    // Today's own: Keenu's.
    const own = parseReminderId(`work\nstandup\n20261028T090000\n${seconds(at(8, 45))}`);
    assert.equal(standsInFor(own, [today, tomorrow], state()), true);
    // Without a recurrence line, the event as a whole.
    const whole = parseReminderId(`work\nstandup\n${seconds(at(8, 45))}`);
    assert.equal(standsInFor(whole, [today, tomorrow], state()), true);
});
