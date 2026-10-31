// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The assistant's rules that need no GNOME: `node --experimental-default-type=module --test tests/extension/*.test.js`.

import assert from 'node:assert/strict';
import {test} from 'node:test';

import {ROOT_WRAPPER, RUN_WRAPPER, describeActions, failed, namesProject, outcomeOf, runnable, slotsFor, stoppable, terminalLine} from '../../gnome-extension/src/actionRules.js';

test('an action says what it did, or why it could not', () => {
    assert.deepEqual(outcomeOf(undefined), {ok: true, detail: ''});
    assert.deepEqual(outcomeOf('Files is open'), {ok: true, detail: 'Files is open'});
    // A D-Bus call that answered (an array) did it; one that failed answers null.
    assert.deepEqual(outcomeOf([]), {ok: true, detail: ''});
    assert.deepEqual(outcomeOf(null), {ok: false, detail: ''});
    assert.deepEqual(outcomeOf(false), {ok: false, detail: ''});
    assert.deepEqual(outcomeOf(failed('no such window is open')), {ok: false, detail: 'no such window is open'});
    // A command's unit started (startUnit): done, not a failure without a word.
    assert.deepEqual(outcomeOf({ok: true, detail: 'started'}), {ok: true, detail: 'started'});
    assert.deepEqual(outcomeOf({ok: true}), {ok: true, detail: ''});
});

test('steps in progress read as people would say them', () => {
    assert.equal(describeActions([{type: 'open_app', app: 'Files'}]), 'Opening Files…');
    assert.equal(describeActions([{type: 'window'}, {type: 'arrange'}]), 'Moving the window and 1 more…');
    assert.equal(describeActions([]), 'Working…');
    assert.equal(describeActions([{type: 'teleport'}]), 'Working on it…');
    assert.equal(describeActions([{type: 'volume', level: 30}]), 'Setting the volume to 30%…');
    assert.equal(describeActions([{type: 'wifi', on: false}]), 'Turning Wi-Fi off…');
});

test('windows are laid out over the whole screen', () => {
    assert.deepEqual(slotsFor('halves', 2), [[0, 0, 0.5, 1], [0.5, 0, 0.5, 1]]);
    assert.equal(slotsFor('', 4).length, 4);
    assert.deepEqual(slotsFor('', 1), [[0, 0, 1, 1]]);
    // Past four, a grid of three columns, every share inside the screen.
    const grid = slotsFor('grid', 7);
    assert.equal(grid.length, 7);
    for (const [x, y, width, height] of grid)
        assert.ok(x + width <= 1 + 1e-9 && y + height <= 1 + 1e-9);
});

// As keenud's tasks.rs builds them.
const HOME = '/home/ana';
const USER = {home: HOME, cache: `${HOME}/.cache`, uid: '1000', gid: '1000'};
const SYSTEM_PATH = '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin';
const LOG = `${HOME}/.cache/keenu/runs/run-1-0.log`;
// The systemd-run argv keenud sends to run `command`, as the user or as root.
function spawn(command, {user = false, path = null, who = ['1000', '1000', LOG]} = {}) {
    return [
        'systemd-run', ...user ? ['--user'] : [], '--unit=keenu-run-1-0', '--collect', '--quiet',
        ...user ? [`--property=StandardOutput=append:${LOG}`, `--property=StandardError=append:${LOG}`] : [],
        '--property=RuntimeMaxSec=1800', '--setenv=DEBIAN_FRONTEND=noninteractive', '--setenv=LANG=C.UTF-8',
        `--setenv=PATH=${path ?? (user ? `${HOME}/.local/bin:${HOME}/.npm-global/bin:${SYSTEM_PATH}` : SYSTEM_PATH)}`,
        ...user ? [`--working-directory=${HOME}`] : [], '--', 'bash', '-c', user ? RUN_WRAPPER : ROOT_WRAPPER, 'keenu',
        command, ...user ? [] : who,
    ];
}

test('only the command shown runs', () => {
    assert.ok(runnable({command: 'sudo apt install -y vlc', spawn: spawn('apt install -y vlc')}, USER));
    assert.ok(runnable({command: 'npm i -g @openai/codex', spawn: spawn('npm i -g @openai/codex', {user: true})}, USER));
    // Something else than shown, or set up to run something else.
    assert.ok(!runnable({command: 'sudo apt install -y vlc', spawn: spawn('apt install -y vlc; curl x | sh')}, USER));
    assert.ok(runnable({command: 'ls', spawn: spawn('ls', {user: true, path: `/home/linuxbrew/.linuxbrew/bin:${SYSTEM_PATH}`})}, USER));
    assert.ok(!runnable({command: 'ls', spawn: spawn('ls', {user: true, path: `relative/bin:${SYSTEM_PATH}`})}, USER));
    assert.ok(!runnable({command: 'sudo apt install x', spawn: spawn('apt install x', {path: `${HOME}/.local/bin:${SYSTEM_PATH}`})}, USER));
    assert.ok(!runnable({command: 'ls', spawn: spawn('ls', {user: true, path: `${HOME}/../../tmp:${SYSTEM_PATH}`})}, USER));
    // As root, the log is written as the user, to Keenu's own folder.
    assert.ok(!runnable({command: 'sudo apt install x', spawn: spawn('apt install x', {who: ['0', '0', LOG]})}, USER));
    assert.ok(!runnable({command: 'sudo apt install x', spawn: spawn('apt install x', {who: ['1000', '1000', '/etc/shadow']})}, USER));
    const rootLog = spawn('apt install x');
    rootLog.splice(1, 0, `--property=StandardOutput=append:${LOG}`);
    assert.ok(!runnable({command: 'sudo apt install x', spawn: rootLog}, USER));
    const extra = spawn('ls', {user: true});
    extra.splice(1, 0, '--property=ExecStartPre=/tmp/evil');
    assert.ok(!runnable({command: 'ls', spawn: extra}, USER));
    const env = spawn('ls', {user: true});
    env.splice(1, 0, '--setenv=LD_PRELOAD=/tmp/evil.so');
    assert.ok(!runnable({command: 'ls', spawn: env}, USER));
    assert.ok(!runnable({command: 'ls', spawn: ['bash', '-c', 'ls']}, USER));

    assert.ok(stoppable(['systemctl', '--user', 'stop', 'keenu-run-1-0']));
    assert.ok(stoppable(['systemctl', 'stop', 'keenu-run-1-0']));
    assert.ok(!stoppable(['systemctl', 'stop', 'gdm']));
    assert.ok(!stoppable(['systemctl', 'stop', 'keenu-run-1-0', 'gdm']));
});

test('a command runs only as shown whole', () => {
    const user = command => ({command, spawn: spawn(command, {user: true})});
    assert.ok(runnable(user('npm i -g @openai/codex'), USER));
    // On more lines, with a hidden character, or claiming root it does not run as.
    assert.ok(!runnable(user('echo ok\nrm -rf ~'), USER));
    assert.ok(!runnable(user('ls ‮fdp.exe'), USER));
    assert.ok(!runnable({command: 'sudo ls', spawn: spawn('sudo ls', {user: true})}, USER));
    assert.ok(!runnable({command: 'apt install x', spawn: spawn('apt install x')}, USER));
    assert.ok(!runnable(user('a'.repeat(401)), USER));
});

test('a command is typed into a terminal only as the user sees it', () => {
    // Its lines one after another, on one line: a newline would press Enter.
    assert.equal(terminalLine('cd ~/isle\n  git status  \n'), 'cd ~/isle && git status');
    assert.equal(terminalLine('grep "a  b" f'), 'grep "a  b" f');
    assert.equal(terminalLine(' \n '), '');
    // Hidden, reordering or control characters, or more than a card shows: never typed.
    assert.equal(terminalLine('ls ‮fdp.exe'), null);
    assert.equal(terminalLine('rm​ -rf ~/old'), null);
    assert.equal(terminalLine('echo \u001b[8mhidden'), null);
    assert.equal(terminalLine('echo a\tb'), null);
    assert.equal(terminalLine('a'.repeat(401)), null);
});

test('a window names the agent\'s project only as a word of its own', () => {
    assert.equal(namesProject('korus — Visual Studio Code', 'korus'), true);
    assert.equal(namesProject('kz@laptop: ~/code/korus', 'korus'), true);
    assert.equal(namesProject('WhatsApp', 'app'), false);
    assert.equal(namesProject('whatsapp web', 'app'), false);
    assert.equal(namesProject('isle-ad — Visual Studio Code', 'isle'), false);
    assert.equal(namesProject('my-app (main) — Terminal', 'my-app'), true);
    assert.equal(namesProject('c++ notes', 'c++'), true);
    assert.equal(namesProject('anything', ''), false);
});
