// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The island's look: Keenu's own black, or dark glass over what is behind it, blurred, as Blur my Shell draws
// GNOME's menus. What Cairo paints in the island's colour asks here; CSS does the rest.

const LOOKS = {
    black: {fill: [0, 0, 0, 1], solid: [0, 0, 0], edge: null, blur: null},
    // Blur my Shell's dark menus: what is behind blurred, dimmed to 40% (as its menus measure on screen; its own
    // setting says 60%), under rgba(30, 30, 33, 0.5). GNOME 46's blur skips its brightness for a copy, so the dim is
    // in the island's own colour: one layer, 80% opaque, giving the same (0.4 × 0.5 of what shows, plus the tint).
    glass: {fill: [30 / 255 * 0.625, 30 / 255 * 0.625, 33 / 255 * 0.625, 0.8], solid: [0.075, 0.085, 0.09],
        edge: [1, 1, 1, 0.1], blur: {radius: 30, brightness: 1}},
};

let current = 'black';

/** Sets the look by its Settings name; anything unknown is black. */
export function setTheme(name) {
    current = name in LOOKS ? name : 'black';
}

/** The look in use: 'black' or 'glass'. */
export function theme() {
    return current;
}

/** The island's colour as RGBA in 0..1; glass lets the blur behind show through. */
export function fill() {
    return LOOKS[current].fill;
}

/** An opaque colour as dark as the island looks, as RGB: for rings and corners painted over it. */
export function solid() {
    return LOOKS[current].solid;
}

/** The hairline round the island, as RGBA, or null for none. */
export function edge() {
    return LOOKS[current].edge;
}

/** How what is behind the island is blurred ({radius, brightness}), or null for not at all. */
export function blur() {
    return LOOKS[current].blur;
}
