// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The shelf's light while a file is dragged in: dark until the file is over the drop zone, it rises from
// the island's bottom edge, flares in the file's own colour as it lands, then sinks away.

import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';

import * as Signals from 'resource:///org/gnome/shell/misc/signals.js';

import {BLUE, INDIGO, ORANGE, RED, WHITE} from './palette.js';
import {Spring} from './spring.js';

const CYAN = [0.39, 0.82, 1];
const SHELF = [BLUE, CYAN, INDIGO];
const REFUSED = [RED, ORANGE, RED];
/** A little bounce on the swell, so the light answers the file like something alive. */
const SWELL = {stiffness: 90, damping: 11};
/** Per state: how bright it rests, how far and how fast (s) it breathes, and how fast it flows. */
const LOOK = {
    over: {level: 0.62, breathe: 0.06, period: 1.6, speed: 1.6},
    full: {level: 0.2, breathe: 0.04, period: 2.6, speed: 0.6},
    landing: {level: 0.85, breathe: 0, period: 1, speed: 2.2},
    landed: {level: 1, breathe: 0, period: 1, speed: 2.6},
};
/** The flare stays this long before it sinks (the island's 'aura' fade-out follows). */
const FLARE_MS = 450;
/** A drop let go over the shelf waits this long for the file to show up in keenud's list. */
const LANDING_MS = 1500;
/** Once its colour is gone, the light keeps moving this long while it fades. */
const FADE_MS = 700;
/** New colours (a file's, once its preview is read) flow in over about three times this (s). */
const COLOR_EASE = 0.12;

/** Blends colour `a` towards `b` by `t` (0..1). */
const mix = (a, b, t) => a.map((value, i) => value + (b[i] - value) * t);

// Signals: `changed` (on or off; see `colors`), `frame` (time in s, level 0..1). While on, `colors`
// stays the same arrays, eased in place towards new colours, so nothing jumps.
export class DropLight extends Signals.EventEmitter {
    /** Runs on `actor`'s frame clock, so it stops with the actor. */
    constructor(actor) {
        super();
        this._state = null;
        /** The colours shown, and those they ease towards. */
        this._colors = null;
        this._target = null;
        this._fileColors = null;
        this._time = 0;
        this._level = new Spring(0, SWELL);
        this._timers = new Map();
        this._timeline = new Clutter.Timeline({actor, duration: 1000, repeat_count: -1});
        this._timeline.connect('new-frame', () => this._tick());
        this._last = 0;
    }

    /** The colours it flows in, or null while off. */
    get colors() {
        return this._state ? this._colors : null;
    }

    /** 'over', 'full', 'landing' or 'landed'; null while off. */
    get state() {
        return this._state;
    }

    /** The dragged files' colours (RGB 0..1), or none: the light flows in them, else in the shelf's blues. */
    set fileColors(colors) {
        const [first, second, third] = colors ?? [];
        this._fileColors = !first ? null
            : second ? [first, second, third ?? mix(first, WHITE, 0.5)]
            : [first, mix(first, WHITE, 0.5), mix(first, BLUE, 0.3)];
        if (this._state === 'over')
            this._set('over', this._fileColors ?? SHELF);
    }

    /** A file is dragged near: lit only while `over` the drop zone; `full` turns it red. */
    approach(over, full) {
        if (['landing', 'landed'].includes(this._state))
            return;
        if (over)
            this._set(full ? 'full' : 'over', full ? REFUSED : this._fileColors ?? SHELF);
        else
            this.off();
    }

    /** Let go over the drop zone: bright until the file lands, or it gives up. */
    release() {
        this._set('landing', this._fileColors ?? SHELF);
        this._after('landing', LANDING_MS, () => this.off());
    }

    /** The file landed: a flare in its colour (RGB 0..1), then the light sinks. */
    land(color) {
        const rgb = color ?? CYAN;
        this._set('landed', [rgb, mix(rgb, WHITE, 0.55), mix(rgb, BLUE, 0.35)]);
        // A kick, so it overshoots and settles like a drop hitting water.
        this._level.velocity += 5;
        this._after('flare', FLARE_MS, () => this.off());
    }

    /** Lets the light sink; it keeps moving while it fades. */
    off() {
        if (!this._state)
            return;
        this._state = null;
        this._clearTimers();
        this.emit('changed');
        this._after('stop', FADE_MS, () => this._timeline.stop());
    }

    /** Stops timers and the frame clock, drops every listener. */
    destroy() {
        this._clearTimers();
        this._timeline.stop();
        this.disconnectAll();
    }

    /** Lit afresh or already on, now in `state`. */
    _set(state, colors) {
        this._clearTimers();
        const wasOn = Boolean(this._state);
        this._state = state;
        this._target = colors;
        // Lit afresh, it starts in these colours; already lit, it eases to them.
        if (!wasOn) {
            this._colors = colors.map(rgb => [...rgb]);
            this.emit('changed');
        }
        if (!this._timeline.is_playing()) {
            this._last = 0;
            this._timeline.start();
        }
    }

    /** Per frame: the swell's spring and the colour ease. */
    _tick() {
        const now = this._timeline.get_elapsed_time() + this._timeline.get_current_repeat() * 1000;
        const dt = this._last ? Math.min((now - this._last) / 1000, 0.1) : 1 / 60;
        this._last = now;
        const look = LOOK[this._state] ?? LOOK.over;
        const breathing = look.breathe * Math.sin(2 * Math.PI * this._time / look.period / look.speed);
        this._level.setTarget(this._state ? look.level + breathing : 0);
        this._level.step(dt);
        this._time += dt * look.speed;
        const k = 1 - Math.exp(-dt / COLOR_EASE);
        this._colors?.forEach((rgb, i) => rgb.forEach((value, j) => (rgb[j] = value + (this._target[i][j] - value) * k)));
        this.emit('frame', this._time, Math.min(1, Math.max(0, this._level.value)));
    }

    /** Named one-shot timers, so a newer one replaces the old. */
    _after(name, ms, callback) {
        this._timers.get(name)?.();
        const id = GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => {
            this._timers.delete(name);
            callback();
            return GLib.SOURCE_REMOVE;
        });
        this._timers.set(name, () => GLib.source_remove(id));
    }

    /** Cancels them all. */
    _clearTimers() {
        for (const clear of this._timers.values())
            clear();
        this._timers.clear();
    }
}
