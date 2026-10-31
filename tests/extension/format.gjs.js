// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Formatting that needs GLib but not GNOME Shell: `gjs -m tests/extension/format.gjs.js`.

import {answerPlain, hoursMinutes, shortcutLabel, weekendDays} from '../../gnome-extension/src/format.js';

let failures = 0;
// Prints a failure and counts it; JSON compares arrays by value.
function check(what, actual, expected) {
    if (JSON.stringify(actual) === JSON.stringify(expected))
        return;
    failures++;
    printerr(`FAIL ${what}: ${JSON.stringify(actual)}, wanted ${JSON.stringify(expected)}`);
}

check('Super+I', shortcutLabel('<Super>i'), 'Super+I');
check('Ctrl+Alt+Space', shortcutLabel('<Primary><Alt>space'), 'Ctrl+Alt+Space');
check('no shortcut', shortcutLabel(''), '');
check('app time in seconds', hoursMinutes(29_999), '<1m');
check('app time at half a minute', hoursMinutes(30_000), '<1m');
check('app time just under a minute', hoursMinutes(59_999), '<1m');
check('app time of a minute', hoursMinutes(60_000), '1m');
check('app time in minutes', hoursMinutes(25 * 60_000), '25m');
check('app time in hours', hoursMinutes(75 * 60_000), '1h 15m');
check('no app time', hoursMinutes(0), '0m');
check('weekend in Indonesia', weekendDays('id_ID.UTF-8'), [0, 6]);
check('weekend in Saudi Arabia', weekendDays('ar_SA.UTF-8'), [5, 6]);
check('weekend with no region', weekendDays('C.UTF-8'), [0, 6]);
check('bold in an answer', answerPlain('Playing **Late Night Blue** by The Harbor Trio.'), 'Playing Late Night Blue by The Harbor Trio.');
check('code and a list', answerPlain('Run `sudo apt update`:\n- first\n- second'), 'Run sudo apt update: • first • second');
check('plain stays', answerPlain('Two things today.'), 'Two things today.');
check('say json', answerPlain('{"say":"Something."}'), 'Something.');
check('say json fenced', answerPlain('```json\n{"say":"Hi there."}\n```'), 'Hi there.');

if (failures)
    imports.system.exit(1);
print('format: all passed');
