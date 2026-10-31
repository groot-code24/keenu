// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Keenu: a notch-style island for the GNOME top bar. The extension only draws;
// keenud (the Rust daemon) tracks media, timers and system state over D-Bus.

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {Controller} from './src/controller.js';
import {releaseIconTheme, setIconDir} from './src/widgets.js';

/** The entry point GNOME Shell loads; everything else hangs off the controller. */
export default class KeenuExtension extends Extension {
    /** Icons first: the widgets the controller builds look them up. */
    enable() {
        setIconDir(this.dir.get_child('icons'));
        this._controller = new Controller(this);
    }

    /** Takes the island down; GNOME also calls this when the screen locks. */
    disable() {
        this._controller?.destroy();
        this._controller = null;
        releaseIconTheme();
    }
}
