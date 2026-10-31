// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The app usage session and report buffer, with no GNOME:
// `node --experimental-default-type=module --test tests/extension/*.test.js`.

import assert from 'node:assert/strict';
import {test} from 'node:test';

import {AppUsageReports} from '../../gnome-extension/src/appUsageReports.js';
import {AppUsageSession} from '../../gnome-extension/src/appUsageSession.js';

const BRAVE = {id: 'brave-browser.desktop', name: 'Brave Web Browser'};
const CODE = {id: 'code.desktop', name: 'Visual Studio Code'};
const SPOTIFY = {id: 'spotify.desktop', name: 'Spotify'};
const WALL = 1_700_000_000_000;

/** A monotonic and wall clock that advance together. */
function clock() {
    let now = 0;
    let wall = WALL;
    return {
        now: () => now,
        wall: () => wall,
        advance(ms) {
            now += ms;
            wall += ms;
        },
    };
}

test('time in front counts without input, and a new stretch starts after a gap', () => {
    const time = clock();
    const session = new AppUsageSession(time.now, time.wall);
    session.switchTo(BRAVE);
    // Five minutes with no input: the app in front still counts all of it.
    time.advance(5 * 60_000);
    assert.deepEqual(session.end(), {...BRAVE, start: WALL, ms: 5 * 60_000});
    // Away for ten more minutes: no app's time passes.
    time.advance(10 * 60_000);
    assert.equal(session.flush(), null);
    // The next input starts a new stretch.
    session.switchTo(BRAVE);
    time.advance(60_000);
    assert.deepEqual(session.flush(), {...BRAVE, start: WALL + 15 * 60_000, ms: 60_000});
});

test('a session held awake keeps counting past five idle minutes', () => {
    const time = clock();
    const session = new AppUsageSession(time.now, time.wall);
    session.switchTo(BRAVE);
    // A video is playing, so appUsage.js does not end the session at the idle watch.
    time.advance(20 * 60_000);
    assert.deepEqual(session.flush(), {...BRAVE, start: WALL, ms: 20 * 60_000});
});

test('focus moving to another app ends the old stretch and starts the new one', () => {
    const time = clock();
    const session = new AppUsageSession(time.now, time.wall);
    session.switchTo(BRAVE);
    time.advance(5 * 60_000);
    assert.deepEqual(session.switchTo(CODE, time.now(), time.wall()), {...BRAVE, start: WALL, ms: 5 * 60_000});
    time.advance(2 * 60_000);
    assert.deepEqual(session.flush(), {...CODE, start: WALL + 5 * 60_000, ms: 2 * 60_000});
});

test('another window of the same app keeps the session whole', () => {
    const time = clock();
    const session = new AppUsageSession(time.now, time.wall);
    session.switchTo(BRAVE);
    time.advance(3 * 60_000);
    // The focus moved to another Brave window: the same desktop id is not a new session.
    assert.equal(session.switchTo({...BRAVE}, time.now(), time.wall()), null);
    time.advance(4 * 60_000);
    assert.deepEqual(session.flush(), {...BRAVE, start: WALL, ms: 7 * 60_000});
});

test('no focused window stops the clock until an app is in front again', () => {
    const time = clock();
    const session = new AppUsageSession(time.now, time.wall);
    session.switchTo(BRAVE);
    time.advance(2 * 60_000);
    assert.deepEqual(session.switchTo(null, time.now(), time.wall()), {...BRAVE, start: WALL, ms: 2 * 60_000});
    // The desktop or the overview: no app is in front, so no app's time passes.
    time.advance(30 * 60_000);
    assert.equal(session.flush(), null);
    session.switchTo(SPOTIFY);
    time.advance(60_000);
    assert.deepEqual(session.flush(), {...SPOTIFY, start: WALL + 32 * 60_000, ms: 60_000});
});

test('ending the session reports its last stretch once', () => {
    const time = clock();
    const session = new AppUsageSession(time.now, time.wall);
    session.switchTo(BRAVE);
    time.advance(90_000);
    assert.deepEqual(session.end(), {...BRAVE, start: WALL, ms: 90_000});
    assert.equal(session.end(), null);
    time.advance(60_000);
    assert.equal(session.flush(), null);
    // A new focus after the session ended starts from zero.
    session.switchTo(BRAVE);
    time.advance(2_000);
    assert.deepEqual(session.flush(), {...BRAVE, start: WALL + 150_000, ms: 2_000});
});

test('reports merge a stretch that goes on with the same app, and drop flicks', () => {
    const reports = new AppUsageReports();
    reports.add({...BRAVE, start: WALL, ms: 30_000});
    reports.add({...BRAVE, start: WALL + 30_000, ms: 30_000});
    reports.add({...BRAVE, start: WALL + 60_000, ms: 500});
    assert.deepEqual(reports.take(), [{...BRAVE, start: WALL, ms: 60_000}]);
    assert.equal(reports.size, 0);
});

test('a stretch flushed every minute goes out as one report', () => {
    const time = clock();
    const session = new AppUsageSession(time.now, time.wall);
    const reports = new AppUsageReports();
    session.switchTo(BRAVE);
    // The tracker's minute timer flushes the running stretch into the buffer each time.
    for (let i = 0; i < 3; i++) {
        time.advance(60_000);
        reports.add(session.flush());
    }
    // A focus change and its flush join the same stretch: the app was in front the whole time.
    time.advance(30_000);
    reports.add(session.switchTo(CODE, time.now(), time.wall()));
    time.advance(60_000);
    reports.add(session.flush());
    assert.deepEqual(reports.take(), [
        {...BRAVE, start: WALL, ms: 3 * 60_000 + 30_000},
        {...CODE, start: WALL + 3 * 60_000 + 30_000, ms: 60_000},
    ]);
});

test('reports keep different apps, names and untouching stretches apart', () => {
    const reports = new AppUsageReports();
    reports.add({...BRAVE, start: WALL, ms: 1_000});
    reports.add({...CODE, start: WALL + 1_000, ms: 2_000});
    reports.add({...BRAVE, start: WALL + 3_000, ms: 3_000});
    reports.add({...BRAVE, name: 'Brave', start: WALL + 6_000, ms: 4_000});
    assert.deepEqual(reports.take(), [
        {...BRAVE, start: WALL, ms: 1_000},
        {...CODE, start: WALL + 1_000, ms: 2_000},
        {...BRAVE, start: WALL + 3_000, ms: 3_000},
        {...BRAVE, name: 'Brave', start: WALL + 6_000, ms: 4_000},
    ]);
});

test('a stretch merges up to an hour, then starts a new one', () => {
    const reports = new AppUsageReports();
    for (let i = 0; i < 90; i++)
        reports.add({...BRAVE, start: WALL + i * 60_000, ms: 60_000});
    assert.deepEqual(reports.take(), [
        {...BRAVE, start: WALL, ms: 60 * 60_000},
        {...BRAVE, start: WALL + 60 * 60_000, ms: 30 * 60_000},
    ]);
});

test('the buffer keeps the newest 500 stretches while keenud is away', () => {
    const reports = new AppUsageReports();
    for (let i = 0; i < 600; i++)
        reports.add({...(i % 2 ? CODE : BRAVE), start: WALL + i * 2_000, ms: 1_000});
    const kept = reports.take();
    assert.equal(kept.length, 500);
    assert.equal(kept[0].start, WALL + 100 * 2_000);
});

test('stretches a report could not deliver go back ahead of newer ones', () => {
    const reports = new AppUsageReports();
    reports.add({...BRAVE, start: WALL, ms: 2_000});
    const taken = reports.take();
    reports.add({...CODE, start: WALL + 5_000, ms: 3_000});
    reports.restore(taken);
    assert.deepEqual(reports.take(), [{...BRAVE, start: WALL, ms: 2_000}, {...CODE, start: WALL + 5_000, ms: 3_000}]);
});
