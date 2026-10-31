// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Colours to wash the island in: picked from a picture (a song's cover, a
// file on the shelf) or, for a file without one, from its type.

import GdkPixbuf from 'gi://GdkPixbuf';
import Gio from 'gi://Gio';

import {memoByFile} from './widgets.js';

/** Pictures are averaged at this size, which costs next to nothing. */
const SAMPLE_SIZE = 12;

/** Hues are grouped into this many slices to find the picture's main one. */
const HUE_SLICES = 12;
/** Less colour than about this many vivid pixels counts as a grey picture. */
const MIN_COLOR_WEIGHT = 1.5;

// The picture's main colour (RGB 0..1) toned for a wash, or null: the heaviest hue
// group, so a blue-and-orange sunset gives one of them, not grey.
export function vividColor(pixbuf) {
    const sample = pixbuf.get_width() === SAMPLE_SIZE && pixbuf.get_height() === SAMPLE_SIZE
        ? pixbuf
        : pixbuf.scale_simple(SAMPLE_SIZE, SAMPLE_SIZE, GdkPixbuf.InterpType.BILINEAR);
    if (!sample)
        return null;
    const pixels = sample.get_pixels();
    const channels = sample.get_n_channels();
    const stride = sample.get_rowstride();
    // Per hue slice, then one more for greys: weighted sums of r, g, b, weight.
    const groups = Array.from({length: HUE_SLICES + 1}, () => [0, 0, 0, 0]);
    for (let y = 0; y < sample.get_height(); y++) {
        for (let x = 0; x < sample.get_width(); x++) {
            const i = y * stride + x * channels;
            // See-through pixels say nothing about the colour.
            if (channels === 4 && pixels[i + 3] < 128)
                continue;
            const [r, g, b] = [pixels[i], pixels[i + 1], pixels[i + 2]];
            const max = Math.max(r, g, b);
            const chroma = max - Math.min(r, g, b);
            let group = HUE_SLICES, weight = 0.05;
            if (chroma > 24 && max > 40) {
                group = Math.floor(hue(r, g, b, max, chroma) / 360 * HUE_SLICES) % HUE_SLICES;
                weight = (chroma / 255) * (0.4 + 0.6 * max / 255);
            }
            const sums = groups[group];
            sums[0] += r * weight;
            sums[1] += g * weight;
            sums[2] += b * weight;
            sums[3] += weight;
        }
    }
    // A slice and its neighbours together, so a hue split across two counts whole.
    const around = index => [-1, 0, 1].reduce((total, step) =>
        total + groups[(index + step + HUE_SLICES) % HUE_SLICES][3], 0);
    let best = HUE_SLICES;
    for (let index = 0; index < HUE_SLICES; index++) {
        if (groups[index][3] > 0 && (best === HUE_SLICES || around(index) > around(best)))
            best = index;
    }
    // Mostly grey: a grey wash then, kept dark.
    if (best !== HUE_SLICES && groups[best][3] < MIN_COLOR_WEIGHT)
        best = HUE_SLICES;
    const [r, g, b, total] = groups[best];
    if (!total)
        return null;
    const color = [r / total, g / total, b / total];
    const peak = Math.max(...color, 1);
    // Vivid pictures wash bright; grey ones stay dark, so white text reads.
    const saturation = (peak - Math.min(...color)) / peak;
    const brightness = 0.35 + 0.45 * Math.min(1, saturation * 2);
    return color.map(channel => (channel / peak) * brightness);
}

/** Hue in degrees of an RGB colour with the given max and chroma. */
function hue(r, g, b, max, chroma) {
    let h;
    if (max === r)
        h = ((g - b) / chroma) % 6;
    else if (max === g)
        h = (b - r) / chroma + 2;
    else
        h = (r - g) / chroma + 4;
    return (h * 60 + 360) % 360;
}

/** `vividColor` of the picture file at `path`, read small; null if unreadable. */
export const vividColorOfFile = memoByFile(path => {
    if (!path)
        return null;
    try {
        return vividColor(GdkPixbuf.Pixbuf.new_from_file_at_scale(path, SAMPLE_SIZE, SAMPLE_SIZE, false));
    } catch {
        return null;
    }
});

// A clear system palette, toned like `vividColor`'s.
const TYPE_COLORS = [
    [['application/pdf'], [0.8, 0.22, 0.2]],
    [['audio/'], [0.8, 0.22, 0.38]],
    [['video/'], [0.55, 0.3, 0.8]],
    [['image/'], [0.2, 0.55, 0.8]],
    [['spreadsheet', 'excel', 'text/csv'], [0.2, 0.62, 0.36]],
    [['presentation', 'powerpoint'], [0.8, 0.5, 0.15]],
    [['zip', 'compressed', 'tar', 'archive', 'x-7z', 'x-rar'], [0.62, 0.48, 0.3]],
    [['word', 'opendocument.text', 'rtf', 'text/'], [0.18, 0.45, 0.8]],
];
const OTHER_COLOR = [0.4, 0.38, 0.8];

/** A colour for a file of `contentType` that has no picture to take one from. */
export function typeColor(contentType) {
    const type = (contentType && Gio.content_type_get_mime_type(contentType)) ?? '';
    return TYPE_COLORS.find(([parts]) => parts.some(part => type.includes(part)))?.[1] ?? OTHER_COLOR;
}
