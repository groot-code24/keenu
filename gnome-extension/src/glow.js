// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The assistant's light, with Cairo only so Settings can show it too: light rising from a shape's edge,
// brightest along the bottom and gone by the top of the sides, its colours flowing round.

import Cairo from 'cairo';

import * as Theme from './theme.js';

const STOPS = 24;

// Light from the edge of a box `height` tall: `time` (s) moves it, `level` (0..1, the voice) swells it,
// `strength` fades it; `cache` keeps the rising mask while the height stays, `scale` is the shell's `uiScale`.
export function paintRim(cr, {left, right, top, radius}, height, colors, {time, level, work = 0}, strength, cache = {}, scale = 1) {
    const r = Math.max(0, radius);
    const sides = () => traceSides(cr, left, right, top, height, r);
    // On glass the light keeps to the edge, as behind it: a thinner haze over the glass, a brighter rim, finer
    // steps inwards, and each colour lit by its own lightness so stacked light never burns to white.
    const glass = Theme.theme() === 'glass';
    const mist = glass ? 0.85 : 1, rim = glass ? 1.3 : 1;
    cr.pushGroup();
    cr.setOperator(Cairo.Operator.ADD);
    // Faint strokes, each narrower, so the edge glows softly inwards.
    const STEPS = glass ? 6 : 3;
    cr.setSource(flow(colors, left, right, time, (0.1 + 0.08 * level) * strength * 3 / STEPS, null, 0, glass));
    for (let i = 0; i < STEPS; i++) {
        cr.setLineWidth((22 + 16 * level) * (1 - i / STEPS) * scale);
        sides();
        cr.stroke();
    }
    // A haze along the bottom, deeper as it swells, with two brighter streaks passing each other.
    const rise = Math.min(52 * scale, Math.max(0, height - top) * 0.5) * (0.75 + 0.5 * level);
    const haze = new Cairo.LinearGradient(0, height - rise, 0, height);
    haze.addColorStopRGBA(0, 0, 0, 0, 0);
    haze.addColorStopRGBA(1, 0, 0, 0, 1);
    cr.setSource(flow(colors, left, right, time * 0.8 + 2, (0.32 + 0.24 * level) * strength * mist, time, work, glass));
    cr.mask(haze);
    // A crisp line of light at the very edge.
    cr.setSource(flow(colors, left, right, time * 1.4, Math.min(1, (0.46 + 0.3 * level + 0.14 * work) * strength * rim),
        time + 3, work, glass));
    cr.setLineWidth(2.5 * scale);
    sides();
    cr.stroke();
    cr.popGroupToSource();
    // Rising from the bottom: gone by the top of the sides.
    if (cache.top !== top || cache.height !== height) {
        const pattern = new Cairo.LinearGradient(0, top, 0, height);
        pattern.addColorStopRGBA(0, 0, 0, 0, 0);
        pattern.addColorStopRGBA(0.55, 0, 0, 0, 0.35);
        pattern.addColorStopRGBA(1, 0, 0, 0, 1);
        Object.assign(cache, {top, height, pattern});
    }
    cr.setOperator(Cairo.Operator.OVER);
    cr.mask(cache.pattern);
}

/** The glow's path: up both sides and along the bottom, round its corners (`r`). */
function traceSides(cr, left, right, top, height, r) {
    cr.moveTo(left, top);
    cr.lineTo(left, height - r);
    cr.arcNegative(left + r, height - r, r, Math.PI, Math.PI / 2);
    cr.lineTo(right - r, height);
    cr.arcNegative(right - r, height - r, r, Math.PI / 2, 0);
    cr.lineTo(right, top);
}

/** A soft bump of brightness `width` wide (0..1) centred on `at`, wrapping round. */
const bump = (x, at, width) => {
    const d = Math.abs(((x - at) % 1 + 1.5) % 1 - 0.5);
    return Math.exp(-(d * d) / (2 * width * width));
};

// A left-to-right gradient of `colors`, flowing with `time`; `streaks` (a time) sends two brighter streaks
// along it in opposite directions, and `work` (0..1, the assistant at work) makes them brighter. `balanced`
// dims the lighter colours by their lightness.
function flow(colors, left, right, time, alpha, streaks = null, work = 0, balanced = false) {
    const gradient = new Cairo.LinearGradient(left, 0, right, 0);
    const weights = colors.map(color => balanced ? weight(color) : 1);
    const a = streaks === null ? 0 : (streaks * 0.11) % 1;
    const b = streaks === null ? 0 : 1 - (streaks * 0.075 + 0.4) % 1;
    for (let i = 0; i <= STOPS; i++) {
        const x = i / STOPS;
        const at = (x + time * 0.12) * colors.length;
        const index = Math.floor(at);
        const mix = at - index;
        const from = colors[index % colors.length];
        const to = colors[(index + 1) % colors.length];
        const balance = weights[index % colors.length] +
            (weights[(index + 1) % colors.length] - weights[index % colors.length]) * mix;
        const swell = streaks === null ? 1
            : 0.62 + (0.55 + 0.75 * work) * bump(x, a, 0.09) + (0.4 + 0.55 * work) * bump(x, b, 0.13);
        gradient.addColorStopRGBA(x, from[0] + (to[0] - from[0]) * mix,
            from[1] + (to[1] - from[1]) * mix, from[2] + (to[2] - from[2]) * mix, Math.min(1, alpha * swell * balance));
    }
    return gradient;
}

/** How strongly a colour lights: Classic's blues and cyan as they are, lighter ones (yellow, green) less. */
function weight([r, g, b]) {
    const linear = c => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    const lightness = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    return Math.min(1, 0.56 / Math.max(lightness, 0.05));
}
