// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The system colours in dark mode, as RGB 0..1; the stylesheet uses the same hex values.

// "rrggbb" as [r, g, b] in 0..1.
const rgb = hex => [0, 2, 4].map(i => parseInt(hex.slice(i, i + 2), 16) / 255);

export const WHITE = [1, 1, 1];
export const GREEN = rgb('30d158');
export const RED = rgb('ff453a');
export const BLUE = rgb('0a84ff');
export const ORANGE = rgb('ff9f0a');
export const INDIGO = rgb('5e5ce6');
