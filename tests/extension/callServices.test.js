// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Telling a web call's window by its title: `node --experimental-default-type=module --test tests/extension/*.test.js`.

import assert from 'node:assert/strict';
import {test} from 'node:test';

import {callStreamOf, callWindowIndex, isEngineName, serviceOf, streamOwner} from '../../gnome-extension/src/callServices.js';

const chrome = (title, focused = false) => ({title: `${title} - Google Chrome`, focused});

test('a title names its service', () => {
    assert.equal(serviceOf('Meet - kgm-mwyq-mge - Google Chrome'), 'Google Meet');
    assert.equal(serviceOf('(4) WhatsApp - Google Chrome'), 'WhatsApp');
    assert.equal(serviceOf('Tulis Lab - Google Chrome'), null);
});

test('a Meet call is not taken for WhatsApp Web open in another window', () => {
    // WhatsApp's window used last, the Meet call's before it.
    assert.equal(callWindowIndex([chrome('(4) WhatsApp'), chrome('Meet - kgm-mwyq-mge')]), 1);
});

test('the window in front wins between two that only name a service', () => {
    assert.equal(callWindowIndex([chrome('(4) WhatsApp'), chrome('Discord | #general', true)]), 1);
});

test('a call under way wins over the window in front', () => {
    assert.equal(callWindowIndex([chrome('(2) WhatsApp', true), chrome('Zoom Meeting')]), 1);
    assert.equal(callWindowIndex([chrome('Google Meet', true), chrome('Weekly sync | Microsoft Teams meeting')]), 1);
});

test('else the most recently used, and none without a service', () => {
    assert.equal(callWindowIndex([chrome('(4) WhatsApp'), chrome('Telegram Web')]), 0);
    assert.equal(callWindowIndex([chrome('Tulis Lab'), chrome('YouTube')]), -1);
    assert.equal(callWindowIndex([]), -1);
});

test("a voice engine's name is never taken for its app's", () => {
    assert.ok(isEngineName('WEBRTC VoiceEngine'));
    assert.ok(isEngineName('ZOOM VoiceEngine'));
    assert.ok(!isEngineName('Google Chrome'));
    assert.ok(!isEngineName(null));
});

test('the process holding a microphone stream, as pactl lists it', () => {
    const list = [
        'Source Output #41',
        '\tDriver: PipeWire',
        '\tProperties:',
        '\t\tapplication.name = "Google Chrome"',
        '\t\tapplication.process.id = "2210"',
        '\t\tapplication.process.binary = "chrome"',
        '',
        'Source Output #57',
        '\tProperties:',
        '\t\tapplication.name = "WEBRTC VoiceEngine"',
        '\t\tapplication.process.id = "3141"',
        '\t\tapplication.process.binary = "Discord"',
    ].join('\n');
    assert.deepEqual(streamOwner(list, 57), {pid: 3141, binary: 'Discord'});
    assert.deepEqual(streamOwner(list, 41), {pid: 2210, binary: 'chrome'});
    assert.equal(streamOwner(list, 99), null);
    assert.equal(streamOwner(null, 57), null);
    assert.deepEqual(streamOwner('Source Output #5\n\tProperties:\n', 5), {pid: 0, binary: null});
});

test('a call keeps its own microphone stream while another app opens the microphone', () => {
    const discord = {name: 'WEBRTC VoiceEngine'}, firefox = {name: 'Firefox'};
    // Firefox's listed first: the Discord call stays Discord's.
    assert.equal(callStreamOf([firefox, discord], discord), discord);
    // Discord's gone: the call goes on in what is left.
    assert.equal(callStreamOf([firefox], discord), firefox);
    // A new call: the first.
    assert.equal(callStreamOf([firefox, discord], null), firefox);
    assert.equal(callStreamOf([], discord), null);
});
