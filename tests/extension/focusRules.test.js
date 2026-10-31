// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// When a focus session silences notifications: `node --experimental-default-type=module --test tests/extension/*.test.js`.

import assert from 'node:assert/strict';
import {test} from 'node:test';

import {NONE, focusSession, focusStep} from '../../gnome-extension/src/focusRules.js';

const MINUTES_25 = 25 * 60000;
const focus = state => ({phase: 'focus', running: false, started_at_ms: null, remaining_ms: MINUTES_25,
    duration_ms: MINUTES_25, ...state});

test('a focus session counts once it runs, paused part way too', () => {
    assert.equal(focusSession(focus({running: true, started_at_ms: 1000})), 1000);
    assert.equal(focusSession(focus({started_at_ms: 1000, remaining_ms: 60000})), 1000);
    assert.equal(focusSession({phase: 'short_break', running: true, started_at_ms: 1000}), null);
    assert.equal(focusSession({phase: 'idle'}), null);
    assert.equal(focusSession(undefined), null);
});

test('a focus phase waiting for Start after a break is no session yet', () => {
    assert.equal(focusSession(focus()), null);
});

test('an keenud that names no start still tells a session from one waiting', () => {
    const older = state => {
        const pomodoro = focus(state);
        delete pomodoro.started_at_ms;
        return pomodoro;
    };
    assert.equal(focusSession(older({running: true})), 0);
    assert.equal(focusSession(older({remaining_ms: 60000})), 0);
    assert.equal(focusSession(older()), null);
});

test('a session turns Do Not Disturb on, and its end off again', () => {
    assert.deepEqual(focusStep({session: 1000, dnd: false, owned: false, declined: NONE}),
        {dnd: true, owned: true, declined: NONE});
    assert.deepEqual(focusStep({session: 1000, dnd: true, owned: true, declined: NONE}),
        {dnd: null, owned: true, declined: NONE});
    assert.deepEqual(focusStep({session: null, dnd: true, owned: true, declined: NONE}),
        {dnd: false, owned: false, declined: NONE});
});

test('Do Not Disturb the user had on is theirs: left on, and on after the session', () => {
    assert.deepEqual(focusStep({session: 1000, dnd: true, owned: false, declined: NONE}),
        {dnd: null, owned: false, declined: NONE});
    assert.deepEqual(focusStep({session: null, dnd: true, owned: false, declined: NONE}),
        {dnd: null, owned: false, declined: NONE});
});

test('turned off during a session, it stays off for the rest of it, and the next turns it on', () => {
    // Paused, resumed, or Keenu built again after unlocking: the same session.
    assert.deepEqual(focusStep({session: 1000, dnd: false, owned: false, declined: 1000}),
        {dnd: null, owned: false, declined: 1000});
    // A break between: forgotten.
    assert.deepEqual(focusStep({session: null, dnd: false, owned: false, declined: 1000}),
        {dnd: null, owned: false, declined: NONE});
    // Another session began while Keenu wasn't running (the screen locked).
    assert.deepEqual(focusStep({session: 9000, dnd: false, owned: false, declined: 1000}),
        {dnd: true, owned: true, declined: NONE});
});
