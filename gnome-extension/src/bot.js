// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The assistant's character, drawn with Cairo alone so Settings can draw it too: its body, its eyes, its glow.
// Shapes and expressions after bloub (MIT, © 2026 Jérémy Perret, github.com/jeremy-prt/bloub).

import Cairo from 'cairo';

/** Each shape is its radius at this many angles round the centre, so any two morph by blending radii. */
const SAMPLES = 64;
const ANGLES = Array.from({length: SAMPLES}, (_, i) => i / SAMPLES * 2 * Math.PI);
const COS = ANGLES.map(Math.cos);
const SIN = ANGLES.map(Math.sin);

/** Three calm colours, violet to blue to cyan: the island's light with the white bot. */
export const CLASSIC_GLOW = [[0.56, 0.4, 1], [0.27, 0.56, 1], [0.22, 0.84, 0.96]];

/** Scales `radii` so the farthest reaches `peak`. */
const normalize = (radii, peak = 1) => {
    const most = Math.max(...radii);
    return Float64Array.from(radii, r => r * peak / most);
};

/** The farthest point along each angle that any of `circles` reaches: bumps without path booleans. */
function unionOfCircles(circles) {
    return ANGLES.map((_, i) => Math.max(0, ...circles.map(({x, y, r}) => {
        const b = COS[i] * x + SIN[i] * y;
        const disc = b * b - (x * x + y * y - r * r);
        return disc < 0 ? 0 : b + Math.sqrt(disc);
    })));
}

/** Where a ray from the centre at each angle leaves the closed polygon `points`. */
function fromPolygon(points) {
    return ANGLES.map((_, k) => {
        let best = 0;
        for (let i = 0; i < points.length; i++) {
            const [ax, ay] = points[i];
            const [bx, by] = points[(i + 1) % points.length];
            const ex = bx - ax, ey = by - ay;
            const den = COS[k] * ey - SIN[k] * ex;
            if (Math.abs(den) < 1e-9)
                continue;
            const t = (ax * ey - ay * ex) / den;
            const u = (ax * SIN[k] - ay * COS[k]) / den;
            if (t > best && u >= 0 && u <= 1)
                best = t;
        }
        return best;
    });
}

/** The outline round two circles: the big one's arc, then the small one's. */
function hullOfCircles(x1, y1, r1, x2, y2, r2) {
    const base = Math.atan2(y2 - y1, x2 - x1);
    const spread = Math.acos(Math.max(-1, Math.min(1, (r1 - r2) / (Math.hypot(x2 - x1, y2 - y1) || 1e-6))));
    const points = [];
    for (let i = 0; i <= 48; i++) {
        const a = base + spread + (2 * Math.PI - 2 * spread) * i / 48;
        points.push([x1 + Math.cos(a) * r1, y1 + Math.sin(a) * r1]);
    }
    for (let i = 0; i <= 48; i++) {
        const a = base - spread + 2 * spread * i / 48;
        points.push([x2 + Math.cos(a) * r2, y2 + Math.sin(a) * r2]);
    }
    return points;
}

/** A regular polygon `radius` across, its corners rounded by `round`: each edge pushed out by a disc. */
function roundedPolygon(sides, radius, round, turn = 0) {
    const points = [];
    for (let i = 0; i < sides; i++) {
        const a = turn + i / sides * 2 * Math.PI;
        const [cx, cy] = [Math.cos(a) * (radius - round), Math.sin(a) * (radius - round)];
        const from = a - Math.PI / sides, to = a + Math.PI / sides;
        for (let k = 0; k <= 10; k++) {
            const b = from + (to - from) * k / 10;
            points.push([cx + Math.cos(b) * round, cy + Math.sin(b) * round]);
        }
    }
    return points;
}

/** A ghost: round on top, straight sides, three soft scallops along the bottom. */
function ghost() {
    const points = [];
    for (let i = 0; i <= 32; i++) {
        const a = Math.PI + Math.PI * i / 32;
        points.push([Math.cos(a), -0.08 + Math.sin(a)]);
    }
    for (let i = 0; i <= 48; i++) {
        const u = i / 48;
        points.push([1 - 2 * u, 0.74 + 0.17 * Math.abs(Math.sin(3 * Math.PI * u))]);
    }
    return points;
}

/** The shapes to pick from, in order; a squircle or a capsule weighs as much as the circle at a larger peak. */
export const SHAPES = {
    circle: {label: 'Circle', radii: new Float64Array(SAMPLES).fill(1)},
    pebble: {label: 'Pebble', radii: normalize(ANGLES.map(a => 1 + 0.075 * Math.cos(2 * a + 0.5) + 0.035 * Math.cos(3 * a + 2.1)), 1.02)},
    squircle: {label: 'Squircle', radii: normalize(ANGLES.map((_, i) => (Math.abs(COS[i]) ** 4.2 + Math.abs(SIN[i]) ** 4.2) ** (-1 / 4.2)), 1.1)},
    capsule: {label: 'Capsule', radii: normalize(fromPolygon(hullOfCircles(-0.42, 0, 0.62, 0.42, 0, 0.62)), 1.08)},
    cloud: {label: 'Cloud', radii: normalize(unionOfCircles([{x: -0.44, y: 0.2, r: 0.54}, {x: 0.46, y: 0.2, r: 0.5},
        {x: 0.02, y: 0.3, r: 0.6}, {x: -0.24, y: -0.3, r: 0.48}, {x: 0.3, y: -0.24, r: 0.44}]), 1.06)},
    hexagon: {label: 'Hexagon', radii: normalize(fromPolygon(roundedPolygon(6, 1.04, 0.26)), 1.04)},
    drop: {label: 'Drop', radii: normalize(fromPolygon(hullOfCircles(0, 0.28, 0.66, 0, -0.96, 0.05)), 1.06)},
    ghost: {label: 'Ghost', radii: normalize(fromPolygon(ghost()), 1.04)},
};
/** Where the eyes sit on each shape (down, in body radii) and how big, so they stay well inside it. */
const EYE_FIT = {cloud: {y: 0.1, size: 1}, drop: {y: 0.2, size: 0.9}, hexagon: {y: 0, size: 0.95}, ghost: {y: -0.06, size: 1}};

/** '#rrggbb' to RGB in 0..1. */
const hex = value => [1, 3, 5].map(i => parseInt(value.slice(i, i + 2), 16) / 255);
/** `a` moved towards `b` by `t`. */
const mix = (a, b, t) => a.map((v, i) => v + (b[i] - v) * t);
/** Perceived brightness (Rec. 709 weights). */
const luminance = ([r, g, b]) => 0.2126 * r + 0.7152 * g + 0.0722 * b;

/** Colours to pick from: the body, and the three colours the island glows with beside it. */
export const COLORS = {
    cloud: {label: 'Cloud', body: '#f5f5f8', glow: CLASSIC_GLOW},
    ink: {label: 'Ink', body: '#2a2a33', glow: ['#7f8cff', '#b9c2ff', '#5fb4ff']},
    peach: {label: 'Peach', body: '#ffb48f', glow: ['#ff7a59', '#ffb36b', '#ff8fa3']},
    coral: {label: 'Coral', body: '#ff6f61', glow: ['#ff4d6d', '#ff7a45', '#ffb347']},
    amber: {label: 'Amber', body: '#ffc94a', glow: ['#ff9f0a', '#ffd60a', '#ff7a2f']},
    mint: {label: 'Mint', body: '#63e2ad', glow: ['#30d158', '#5fe0c0', '#b4f05a']},
    teal: {label: 'Teal', body: '#2fc6b8', glow: ['#30c8d8', '#30d1a0', '#4f9dff']},
    sky: {label: 'Sky', body: '#5eaaff', glow: ['#4f7dff', '#3fb6ff', '#6fe3ff']},
    lavender: {label: 'Lavender', body: '#ab8eff', glow: ['#8f66ff', '#c77dff', '#5e7bff']},
    rose: {label: 'Rose', body: '#ff80b8', glow: ['#ff5fa2', '#c86bff', '#ff8a7a']},
    graphite: {label: 'Graphite', body: '#8f909c', glow: ['#a0a8c0', '#d6dae6', '#7d8bb0']},
};

/** A glow colour as RGB, whether written in hex or already a triple. */
const rgb = color => typeof color === 'string' ? hex(color) : color;

/** Three glow colours around `base`: its hue and its two neighbours, bright enough to light the edge. */
function glowAround(base) {
    const [r, g, b] = base;
    const most = Math.max(r, g, b), least = Math.min(r, g, b);
    if (most - least < 0.08)
        return CLASSIC_GLOW;
    // A turn of the colour wheel, done on the RGB cube's grey axis, keeps it simple and smooth.
    const turn = degrees => {
        const a = degrees * Math.PI / 180, c = Math.cos(a), s = Math.sin(a), k = (1 - c) / 3, q = Math.sqrt(1 / 3) * s;
        const m = [[c + k, k - q, k + q], [k + q, c + k, k - q], [k - q, k + q, c + k]];
        const out = m.map(row => row[0] * r + row[1] * g + row[2] * b);
        const top = Math.max(...out, 1e-3);
        return out.map(v => Math.max(0, Math.min(1, 0.25 + 0.75 * v / top)));
    };
    return [turn(-28), turn(0), turn(30)];
}

/** How the bot looks from Settings' `bot-color` and `bot-glow`: its body's shades, its eyes, its glow. */
export function paletteFor(color = 'cloud', glow = 'match') {
    const preset = COLORS[color];
    const base = preset ? hex(preset.body) : /^#[0-9a-f]{6}$/i.test(color) ? hex(color) : hex(COLORS.cloud.body);
    const dark = luminance(base) < 0.42;
    const light = base.every(v => v > 0.9);
    let glowColors;
    if (glow === 'classic')
        glowColors = CLASSIC_GLOW;
    else if (/^#[0-9a-f]{6}$/i.test(glow))
        glowColors = glowAround(hex(glow));
    else
        glowColors = preset ? preset.glow.map(rgb) : glowAround(base);
    return {
        base,
        // Lit from the top left: a lighter side, and a shade that keeps the colour (cool grey for white).
        light: mix(base, [1, 1, 1], dark ? 0.22 : 0.55),
        shade: light ? [0.74, 0.76, 0.84] : mix(base, [0, 0, 0.04], dark ? 0.35 : 0.3),
        eyes: dark ? [0.96, 0.96, 0.98] : [0.07, 0.07, 0.09],
        glow: glowColors,
        dark,
    };
}

/** Two matching eyes, the second's tilt mirrored. */
const pair = (w, h, tilt = 0, open = 1, curve = 0) => [{w, h, tilt, open, curve}, {w, h, tilt: -tilt, open, curve}];
// The head's turn and how far apart the eyes sit (degrees); each eye's width and height (eye units),
// tilt, how open, and `curve`: 0 a capsule, 1 a smiling arc.
const EXPRESSIONS = {
    attentive: {yaw: 3, pitch: 4, roll: 0, split: 16, eyes: pair(0.22, 0.44)},
    surprised: {yaw: 0, pitch: -2, roll: 0, split: 17.5, eyes: pair(0.32, 0.5)},
    curious: {yaw: 8, pitch: -5, roll: -5, split: 16, eyes: pair(0.22, 0.42)},
    happy: {yaw: 2, pitch: 7, roll: 0, split: 17, eyes: pair(0.3, 0.27, 0, 1, 1)},
    joy: {yaw: 0, pitch: 8, roll: 0, split: 17.5, eyes: pair(0.34, 0.3, 0, 1, 1)},
    focused: {yaw: 0, pitch: -4, roll: 0, split: 15.5, eyes: pair(0.23, 0.42, 0, 0.64)},
    confused: {yaw: -6, pitch: 2, roll: 7, split: 16, eyes: [{w: 0.22, h: 0.44, tilt: 0, open: 1, curve: 0}, {w: 0.22, h: 0.44, tilt: 0, open: 0.66, curve: 0}]},
    sad: {yaw: 2, pitch: -9, roll: 0, split: 15.5, eyes: pair(0.22, 0.42, 0, 0.6)},
    sleepy: {yaw: 3, pitch: -7, roll: -2, split: 16, eyes: pair(0.22, 0.42, 0, 0.4)},
};
/** A deep copy, so morphing never changes the presets. */
const copyExpression = ({eyes, ...turn}) => ({...turn, eyes: eyes.map(eye => ({...eye}))});
const TURN_KEYS = ['yaw', 'pitch', 'roll', 'split'];
const EYE_KEYS = ['w', 'h', 'tilt', 'open', 'curve'];

/** Moves made on cue, each lasting `length` seconds: a yes, a no, a little hop. */
const GESTURES = {
    nod: {length: 0.7, move: (t, out) => (out.pitch -= 11 * Math.sin(t * 2 * Math.PI / 0.34) * (1 - t / 0.7))},
    shake: {length: 0.62, move: (t, out) => (out.yaw += 14 * Math.sin(t * 2 * Math.PI / 0.24) * (1 - t / 0.62))},
    hop: {length: 0.6, move: (t, out) => {
        const up = Math.max(0, Math.sin(Math.PI * t / 0.36));
        out.lift += t < 0.36 ? up : 0;
        // Landing: squashed a moment, then back.
        out.squash += t >= 0.36 ? 0.09 * Math.sin(Math.PI * (t - 0.36) / 0.24) : -0.05 * up;
    }},
};

/** The head's radius and the eyes' ball and unit, as parts of the drawing's size. */
const HEAD = 15 / 36;
const EYE_BALL = 17.3 / 15;
const EYE_UNIT = 20.3 / 15;
const LOOK_TURN = {yaw: 12, pitch: 10};
const MAX_TURN = {yaw: 20, pitch: 16};
/** How much the eyes narrow and tilt as the head turns (0 flat, 1 a ball's): a hint, so they never look bent. */
const FORESHORTEN = 0.3;
const EYE_ROLL = 0.4;
/** How fast (per second) a new expression and a new shape take over: about a third of a second. */
const MORPH_RATE = 10;
/** A blink: the eye squashes shut in the first part of it and opens in the rest (s). */
const BLINK_S = 0.18;
const BLINK_CLOSING = 0.45;
const SMILE_S = 1.6;
const SURPRISE_S = 0.9;
const SLEEPY_S = 8;
/** How the eyes move to a new spot: quick, a touch past it, then settled, as eyes do. */
const SACCADE = {stiffness: 420, damping: 25};
/** After its smile it reads its answer along, this long (s). */
const READ_S = 2.4;
/** A pause this long (s) after it heard you speak earns a small nod: it got that. */
const HEARD_PAUSE_S = 0.5;
const RAD = Math.PI / 180;
/** Forms whose eyes go where the form says, not to the pointer. */
const OWN_GAZE = new Set(['thinking', 'searching', 'running', 'asking', 'error', 'confused']);
/** Keeps `value` within ±`limit`. */
const clamp = (value, limit = 1) => Math.max(-limit, Math.min(limit, value));
/** 0 to 1, a little past 1 before it settles. */
const easeOutBack = x => 1 + 2.2 * (x - 1) ** 3 + 1.2 * (x - 1) ** 2;

/** Turns `u` towards `v` by `angle` (radians), and `v` along: one rotation of two axes, in place. */
function rotate(u, v, angle) {
    const c = Math.cos(angle), s = Math.sin(angle);
    for (let i = 0; i < 3; i++) {
        const x = u[i], y = v[i];
        u[i] = x * c + y * s;
        v[i] = y * c - x * s;
    }
}

// The character's life: where it looks, its expression, blinks, gestures and shape, moved on by `step`
// and drawn by `paint` into any Cairo context. It allocates nothing per frame.
export class Bot {
    /** Starts in `shape`, in a `paletteFor` palette. */
    constructor({shape = 'circle', palette = paletteFor()} = {}) {
        this.palette = palette;
        this._radii = Float64Array.from(SHAPES[shape]?.radii ?? SHAPES.circle.radii);
        this._goalRadii = SHAPES[shape]?.radii ?? SHAPES.circle.radii;
        this._fit = {y: 0, size: 1, ...EYE_FIT[shape]};
        this._goalFit = this._fit;
        this._points = new Float64Array(2 * SAMPLES);
        this._head = {forward: new Float64Array(3), across: new Float64Array(3), up: new Float64Array(3)};
        this._places = [0, 1].map(() => ({x: 0, y: 0, at: new Float64Array(3), across: new Float64Array(3)}));
        this._turn = {yaw: 0, pitch: 0, roll: 0, split: 0, lift: 0, squash: 0};
        this.restart();
    }

    /** Starts the clock over, as the view does when it opens again. */
    restart() {
        this.time = 0;
        this.level = 0;
        this._form = 'listening';
        this._formAt = 0;
        this._last = 0;
        this._gaze = {x: 0, y: 0, vx: 0, vy: 0};
        this._glance = {x: 0, y: 0};
        this._jitter = {x: 0, y: 0, next: 0};
        this._nextGlance = 0;
        this._lookAt = -10;
        this._blinkAt = -1;
        this._nextBlink = 1.2;
        this._surpriseAt = -SURPRISE_S;
        this._heard = false;
        this._voiceAt = 0;
        this._spokeAt = -10;
        this._speaking = 0;
        this._gesture = null;
        this._gestureAt = 0;
        this._expression = copyExpression(EXPRESSIONS.attentive);
    }

    /** What it is doing now (see `setForm`). */
    get form() {
        return this._form;
    }

    /** Whether the form decides where it looks (thinking, reading, …) rather than the pointer. */
    get looksOnItsOwn() {
        return OWN_GAZE.has(this._form) || this._reading(this.time);
    }

    /** Reading its answer along, once it has smiled at you. */
    _reading(time) {
        const since = time - this._formAt;
        return this._form === 'reply' && since > SMILE_S && since < SMILE_S + READ_S;
    }

    /** A new shape, morphed to; a blink hides the change. */
    setShape(shape) {
        const goal = SHAPES[shape]?.radii ?? SHAPES.circle.radii;
        if (goal === this._goalRadii)
            return;
        this._goalRadii = goal;
        this._goalFit = {y: 0, size: 1, ...EYE_FIT[shape]};
        this.blink();
    }

    /** listening, typing, thinking, searching, asking, running, reply, success, confused or error;
     * each new form pops in behind a blink. */
    setForm(form) {
        if (form === this._form)
            return;
        this._form = form;
        this._formAt = this.time;
        this._heard = false;
        this.blink();
        if (form === 'success')
            this.gesture('hop');
    }

    /** nod, shake or hop (see GESTURES). */
    gesture(name) {
        this._gesture = GESTURES[name] ?? null;
        this._gestureAt = this.time;
    }

    /** Never blinks on its own: for a still copy, drawn once. */
    holdEyes() {
        this._nextBlink = Infinity;
    }

    /** Blinks now, unless a blink is already under way. */
    blink() {
        if (this.time - this._blinkAt > BLINK_S)
            this._blinkAt = this.time;
    }

    /** Moves it on to `time` (s). `look` is where to look, x and y in -1..1, or null to look about on its own;
     * `level` (0..1) is the voice heard. */
    step(time, look, level = 0) {
        const dt = Math.min(0.1, Math.max(0, time - this._last));
        this._last = this.time = time;
        this.level = level;

        if (look)
            this._lookAt = time;
        const target = look ?? this._target(time);
        // Tiny flicks now and then, as eyes do even when they rest on something.
        if (time >= this._jitter.next) {
            this._jitter.x = (Math.random() * 2 - 1) * 0.05;
            this._jitter.y = (Math.random() * 2 - 1) * 0.04;
            this._jitter.next = time + 0.35 + Math.random() * 0.6;
        }
        // Quick to turn, a touch past, then settled: a saccade, on a spring.
        if (Number.isFinite(target.x) && Number.isFinite(target.y)) {
            const gaze = this._gaze;
            const {stiffness, damping} = SACCADE;
            gaze.vx += ((target.x + this._jitter.x - gaze.x) * stiffness - gaze.vx * damping) * dt;
            gaze.vy += ((target.y + this._jitter.y - gaze.y) * stiffness - gaze.vy * damping) * dt;
            gaze.x += gaze.vx * dt;
            gaze.y += gaze.vy * dt;
        }

        this._hear(time, dt, level);

        const goal = EXPRESSIONS[this._expressionAt(time)];
        const morph = 1 - Math.exp(-dt * MORPH_RATE);
        const shape = this._expression;
        for (const key of TURN_KEYS)
            shape[key] += (goal[key] - shape[key]) * morph;
        for (let index = 0; index < 2; index++) {
            for (const key of EYE_KEYS)
                shape.eyes[index][key] += (goal.eyes[index][key] - shape.eyes[index][key]) * morph;
        }
        for (let i = 0; i < SAMPLES; i++)
            this._radii[i] += (this._goalRadii[i] - this._radii[i]) * morph;
        if (this._fit !== this._goalFit) {
            this._fit = {y: this._fit.y + (this._goalFit.y - this._fit.y) * morph,
                size: this._fit.size + (this._goalFit.size - this._fit.size) * morph};
            if (Math.abs(this._fit.y - this._goalFit.y) < 1e-3)
                this._fit = this._goalFit;
        }

        if (time >= this._nextBlink) {
            this._blinkAt = time;
            // Now and then a double blink; slow and far apart when sleepy.
            const sleepy = shape.eyes[0].open < 0.7;
            this._nextBlink = time + (sleepy ? 3.5 + Math.random() * 3 : Math.random() < 0.18 ? 0.26 : 1.9 + Math.random() * 2.7);
        }
        if (this._gesture && time - this._gestureAt > this._gesture.length)
            this._gesture = null;
    }

    // Listening: perks up at the first word, bounces with each syllable, leans in while you speak,
    // and nods once when you pause.
    _hear(time, dt, level) {
        if (this._form !== 'listening') {
            this._speaking = 0;
            return;
        }
        if (level > 0.12)
            this._voiceAt = time;
        if (level > 0.3) {
            if (!this._heard)
                this._surpriseAt = time;
            this._heard = true;
            this._spokeAt = time;
        }
        // How much it has been spoken to lately: rises quickly, settles slowly.
        const rate = level > 0.2 ? 6 : 1.5;
        this._speaking += ((level > 0.2 ? 1 : 0) - this._speaking) * (1 - Math.exp(-dt * rate));
        const paused = time - this._spokeAt;
        if (this._heard && paused > HEARD_PAUSE_S && paused - dt <= HEARD_PAUSE_S && !this._gesture)
            this.gesture('nod');
    }

    /** The expression for the form at `time` (see EXPRESSIONS). */
    _expressionAt(time) {
        const since = time - this._formAt;
        // Nothing going on for a while: no looking at things, no voice, no new form.
        const idle = time - Math.max(this._formAt, this._lookAt, this._voiceAt) > SLEEPY_S;
        switch (this._form) {
        case 'listening':
            return time - this._surpriseAt < SURPRISE_S ? 'surprised' : idle ? 'sleepy' : 'attentive';
        case 'thinking':
            return 'curious';
        case 'running':
            return 'focused';
        case 'success':
            return since < SMILE_S ? 'joy' : 'attentive';
        case 'reply':
            return since < SMILE_S ? 'happy' : idle ? 'sleepy' : 'attentive';
        case 'confused':
            return 'confused';
        case 'error':
            return 'sad';
        default:
            return 'attentive';
        }
    }

    /** Where the eyes look on their own, for the form at `time` (one object, reused). */
    _target(time) {
        const out = this._glance;
        switch (this._form) {
        // Thinking, sad and confused look where their expression turns the head, swaying a little.
        case 'thinking':
            return this._set(0.3 * Math.sin(time * 1.3), -0.15);
        // Reading along, line after line: a sweep left to right, then back.
        case 'searching':
            return this._set(0.9 * Math.sin(time * 2.2), 0.15 + 0.1 * Math.sin(time * 0.7));
        // Waiting on you: at the buttons below, glancing back now and then.
        case 'asking':
            return this._set(Math.sin(time * 0.5) > 0.85 ? 0 : 0.55, Math.sin(time * 0.5) > 0.85 ? 0 : 0.55);
        // Following the work: slowly along, as if watching lines go by.
        case 'running':
            return this._set(0.25 * Math.sin(time * 0.9), 0.35);
        case 'error':
            return this._set(0.15 * Math.sin(time * 0.8), 0.1);
        case 'confused':
            return this._set(0.12 * Math.sin(time * 1.6), 0);
        }
        // Its answer, line by line: along the first, back, along the second.
        if (this._reading(time)) {
            const at = (time - this._formAt - SMILE_S) / READ_S * 2;
            const line = Math.min(1, Math.floor(at));
            const along = at - Math.floor(at);
            return this._set(-0.2 + 0.85 * Math.min(1, along / 0.8), 0.18 + 0.22 * line);
        }
        if (time >= this._nextGlance) {
            out.x = (Math.random() * 2 - 1) * 0.8;
            out.y = (Math.random() * 2 - 1) * 0.5;
            this._nextGlance = time + 1 + Math.random() * 2.2;
        }
        return out;
    }

    /** One reused target object, so looking about allocates nothing. */
    _set(x, y) {
        this._targetOut ??= {x: 0, y: 0};
        this._targetOut.x = x;
        this._targetOut.y = y;
        return this._targetOut;
    }

    /** Draws it `size` pixels square at the context's origin (its top left). */
    paint(cr, size) {
        const radius = size * HEAD;
        const since = this.time - this._formAt;
        const pop = easeOutBack(Math.min(1, since / 0.3));
        const {x: gx, y: gy} = this._gaze;
        const turn = this._turn;
        const shape = this._expression;
        const t = this.time * 2 * Math.PI;
        turn.lift = 0;
        turn.squash = 0;
        // The expression's turn, plus where it looks, leaning in as you speak, and a slow sway as if it breathed.
        turn.yaw = shape.yaw + gx * LOOK_TURN.yaw + 2.2 * Math.sin(t / 11.3 + 0.4) + 0.7 * Math.sin(t / 3.7 + 2.1);
        turn.pitch = shape.pitch - gy * LOOK_TURN.pitch + 1.7 * Math.sin(t / 9.1 + 1.3) + 0.5 * Math.sin(t / 4.3 + 0.7) - 4 * this._speaking;
        turn.roll = shape.roll + 0.9 * Math.sin(t / 13.7 + 3.2);
        turn.split = shape.split;
        if (this._gesture)
            this._gesture.move(this.time - this._gestureAt, turn);
        turn.yaw = clamp(turn.yaw, MAX_TURN.yaw);
        turn.pitch = clamp(turn.pitch, MAX_TURN.pitch);

        // Each syllable a little bounce: taller as the voice rises, a breath's sway otherwise.
        const voice = this._form === 'listening' ? this.level : 0;
        const breath = 0.006 * Math.sin(t / 3.4);
        const stretch = 1 + breath + 0.05 * voice - turn.squash;
        const widen = 1 - 0.5 * (stretch - 1);
        const scale = (0.88 + 0.12 * pop) * (1 + 0.03 * voice);

        cr.save();
        cr.translate(size / 2 + gx * radius * 0.087, size / 2 + gy * radius * 0.067 - turn.lift * radius * 0.16);
        cr.scale(scale * widen, scale * stretch);
        this._paintBody(cr, radius);
        this._paintEyes(cr, radius, turn);
        cr.restore();
    }

    /** The box its body takes, at rest in the shape it is turning to, when painted `size` big:
     * {x, y, width, height}. */
    bounds(size) {
        const radius = size * HEAD;
        let left = 0, right = 0, top = 0, bottom = 0;
        for (let i = 0; i < SAMPLES; i++) {
            const x = radius * this._goalRadii[i] * COS[i], y = radius * this._goalRadii[i] * SIN[i];
            left = Math.min(left, x);
            right = Math.max(right, x);
            top = Math.min(top, y);
            bottom = Math.max(bottom, y);
        }
        return {x: size / 2 + left, y: size / 2 + top, width: right - left, height: bottom - top};
    }

    /** Traces the body's outline, `radius` big, round the origin: a smooth curve through its radii. */
    _trace(cr, radius) {
        const p = this._points;
        for (let i = 0; i < SAMPLES; i++) {
            p[2 * i] = radius * this._radii[i] * COS[i];
            p[2 * i + 1] = radius * this._radii[i] * SIN[i];
        }
        cr.moveTo(p[0], p[1]);
        for (let i = 0; i < SAMPLES; i++) {
            const a = 2 * ((i + SAMPLES - 1) % SAMPLES), b = 2 * i, c = 2 * ((i + 1) % SAMPLES), d = 2 * ((i + 2) % SAMPLES);
            cr.curveTo(p[b] + (p[c] - p[a]) / 6, p[b + 1] + (p[c + 1] - p[a + 1]) / 6,
                p[c] - (p[d] - p[b]) / 6, p[c + 1] - (p[d + 1] - p[b + 1]) / 6, p[c], p[c + 1]);
        }
        cr.closePath();
    }

    // A soft, lit body: light from the top left, a shade round the far side, a glint, and the island's
    // glow caught along its bottom edge.
    _paintBody(cr, radius) {
        const {light, base, shade, glow, dark} = this.palette;
        this._trace(cr, radius);
        const body = new Cairo.RadialGradient(-0.38 * radius, -0.42 * radius, 0, -0.1 * radius, -0.1 * radius, 1.35 * radius);
        body.addColorStopRGBA(0, ...light, 1);
        body.addColorStopRGBA(0.45, ...base, 1);
        body.addColorStopRGBA(1, ...shade, 1);
        cr.setSource(body);
        cr.fillPreserve();
        cr.save();
        cr.clip();
        // The glow's light on its lower edge, and a rim of light round a dark body so it reads on black.
        const caught = new Cairo.LinearGradient(0, 0.35 * radius, 0, 1.15 * radius);
        caught.addColorStopRGBA(0, ...glow[1], 0);
        caught.addColorStopRGBA(1, ...glow[1], dark ? 0.55 : 0.32);
        cr.setSource(caught);
        cr.paint();
        const glint = new Cairo.RadialGradient(-0.42 * radius, -0.5 * radius, 0, -0.42 * radius, -0.5 * radius, 0.42 * radius);
        glint.addColorStopRGBA(0, 1, 1, 1, dark ? 0.32 : 0.6);
        glint.addColorStopRGBA(1, 1, 1, 1, 0);
        cr.setSource(glint);
        cr.paint();
        cr.restore();
        this._trace(cr, radius);
        cr.setLineWidth(Math.max(0.8, radius / 15));
        const rim = new Cairo.LinearGradient(0, -radius, 0, radius);
        rim.addColorStopRGBA(0, 1, 1, 1, dark ? 0.35 : 0.5);
        rim.addColorStopRGBA(0.5, 1, 1, 1, dark ? 0.08 : 0);
        rim.addColorStopRGBA(1, ...glow[1], dark ? 0.6 : 0.25);
        cr.setSource(rim);
        cr.stroke();
    }

    /** The eyes, placed on a ball turned with the head, narrowing as they turn away. */
    _paintEyes(cr, radius, turn) {
        const {forward, across, up} = this._head;
        forward.fill(0);
        across.fill(0);
        up.fill(0);
        forward[2] = across[0] = up[1] = 1;
        rotate(forward, across, turn.yaw * RAD);
        rotate(up, forward, turn.pitch * RAD);
        rotate(across, up, turn.roll * RAD);
        const ball = radius * EYE_BALL * this._fit.size;
        const shift = radius * this._fit.y;
        const [r, g, b] = this.palette.eyes;
        for (let index = 0; index < 2; index++) {
            const place = this._places[index];
            place.at.set(forward);
            place.across.set(across);
            rotate(place.at, place.across, turn.split * (index ? 1 : -1) * RAD);
            cr.setSourceRGBA(r, g, b, 1);
            this._eye(cr, radius, place.at[0] * ball, place.at[1] * ball + shift, place.across, up, this._expression.eyes[index]);
        }
    }

    // One eye of `eye`'s shape lying on the head (turned, foreshortened); it squashes shut to blink, and a
    // smiling eye is an arc.
    _eye(cr, radius, x, y, across, up, eye) {
        const unit = radius * EYE_UNIT * this._fit.size;
        const since = this.time - this._blinkAt;
        const length = eye.open < 0.7 ? 2 * BLINK_S : BLINK_S;
        let lid = 1;
        if (since >= 0 && since < length) {
            const at = since / length;
            lid = at < BLINK_CLOSING ? 1 - at / BLINK_CLOSING : (at - BLINK_CLOSING) / (1 - BLINK_CLOSING);
        }
        const listening = this._form === 'listening' ? 1 + 0.28 * this.level : 1;
        const w = unit * eye.w;
        const h = unit * eye.h * listening * (0.08 + 0.92 * lid);
        const angle = Math.atan2(across[1], across[0]);
        cr.save();
        cr.translate(x, y);
        // Upright, but for a little of the head's roll; a touch narrower as it turns, never bent.
        cr.rotate(angle * EYE_ROLL);
        const across1 = Math.hypot(across[0], across[1]);
        const up1 = Math.abs(-Math.sin(angle) * up[0] + Math.cos(angle) * up[1]);
        cr.scale(1 - FORESHORTEN * (1 - across1), 1 - FORESHORTEN * (1 - up1));
        cr.rotate(eye.tilt * RAD);
        // Sleepy or set on something: an upper lid down over the eye, its bottom still round.
        const open = Math.max(0.12, Math.min(1, eye.open));
        if (open < 0.99) {
            cr.rectangle(-w, h / 2 - h * open, 2 * w, h * open + 1);
            cr.clip();
        }
        if (eye.curve < 0.98) {
            const rr = Math.min(w, h) / 2;
            const left = -w / 2 + rr, top = -h / 2 + rr, right = w / 2 - rr, bottom = h / 2 - rr;
            cr.newSubPath();
            cr.arc(right, top, rr, -Math.PI / 2, 0);
            cr.arc(right, bottom, rr, 0, Math.PI / 2);
            cr.arc(left, bottom, rr, Math.PI / 2, Math.PI);
            cr.arc(left, top, rr, Math.PI, 1.5 * Math.PI);
            cr.closePath();
            if (eye.curve > 0.02) {
                cr.pushGroup();
                cr.fill();
                cr.popGroupToSource();
                cr.paintWithAlpha(1 - eye.curve);
            } else {
                cr.fill();
            }
        }
        if (eye.curve > 0.02) {
            // ^ : a thick arc, its ends rounded, rising from where the eye's bottom was.
            const arc = w / 2;
            cr.setSourceRGBA(...this.palette.eyes, 1);
            cr.setLineWidth(Math.max(1, w * 0.42));
            cr.setLineCap(Cairo.LineCap.ROUND);
            cr.newSubPath();
            cr.arc(0, arc * 0.55 + (1 - lid) * arc * 0.3, arc, Math.PI * 1.18, Math.PI * 1.82);
            if (eye.curve < 0.98) {
                cr.pushGroup();
                cr.stroke();
                cr.popGroupToSource();
                cr.paintWithAlpha(eye.curve);
            } else {
                cr.stroke();
            }
        }
        cr.restore();
    }
}
