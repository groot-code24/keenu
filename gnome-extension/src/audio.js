// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The default sound output: its volume, mute state and the outputs to choose from.
// Uses the same mixer as GNOME's Quick Settings, so both always agree.

import * as Signals from 'resource:///org/gnome/shell/misc/signals.js';
import {getMixerControl} from 'resource:///org/gnome/shell/ui/status/volume.js';

// Output ids are small integers. GNOME learns them from `output-added` at start-up, which is over
// by the time the extension runs, so existing ones are found by looking up this id range once.
const MAX_DEVICE_ID = 256;

/** Signals: `changed` (): volume, mute, outputs or the active output changed; `outputs-changed` (): only
 * the outputs or the active one, not volume or mute (many times a second while a slider moves). */
export class AudioOutputs extends Signals.EventEmitter {
    /** Picks up the outputs GNOME already knows, then follows the mixer. */
    constructor() {
        super();
        this._mixer = getMixerControl();
        this._devices = new Map();
        this._sink = null;
        this._sinkIds = [];

        for (let id = 0; id < MAX_DEVICE_ID; id++) {
            const device = this._mixer.lookup_output_id(id);
            if (device)
                this._devices.set(id, device);
        }

        this._mixerIds = [
            this._mixer.connect('output-added', (_mixer, id) => {
                const device = this._mixer.lookup_output_id(id);
                if (device)
                    this._devices.set(id, device);
                this._outputsChanged();
            }),
            this._mixer.connect('output-removed', (_mixer, id) => {
                this._devices.delete(id);
                this._outputsChanged();
            }),
            this._mixer.connect('active-output-update', () => this._outputsChanged()),
            this._mixer.connect('default-sink-changed', () => this._watchSink()),
            this._mixer.connect('state-changed', () => this._watchSink()),
        ];
        this._watchSink();
    }

    /** Volume of the default output, 0..1 (1 is 100%, not the boost limit). */
    get volume() {
        if (!this._sink)
            return 0;
        return this._sink.volume / this._mixer.get_vol_max_norm();
    }

    /** Sets the level, unmuting when raised above 0. */
    set volume(fraction) {
        if (!this._sink)
            return;
        const clamped = Math.min(1, Math.max(0, fraction));
        this._sink.volume = clamped * this._mixer.get_vol_max_norm();
        if (this._sink.is_muted && clamped > 0)
            this._sink.change_is_muted(false);
        this._sink.push_volume();
    }

    /** Whether the default output is muted; false when there is none. */
    get muted() {
        return this._sink?.is_muted ?? false;
    }

    /** Whether there is a sound output to set at all. */
    get hasOutput() {
        return Boolean(this._sink);
    }

    /** Mutes or unmutes the default output. */
    toggleMuted() {
        this._sink?.change_is_muted(!this.muted);
    }

    /** Icon for the current level, the same set GNOME uses. */
    get iconName() {
        if (this.muted || this.volume <= 0)
            return 'audio-volume-muted-symbolic';
        if (this.volume < 1 / 3)
            return 'audio-volume-low-symbolic';
        if (this.volume < 2 / 3)
            return 'audio-volume-medium-symbolic';
        return 'audio-volume-high-symbolic';
    }

    /** Outputs as `{id, name, detail, gicon, active}`, active first. */
    get outputs() {
        const activeId = this._activeId();
        return [...this._devices.entries()]
            .map(([id, device]) => ({
                id,
                name: device.get_description(),
                detail: device.get_origin(),
                gicon: device.get_gicon(),
                active: id === activeId,
            }))
            .sort((a, b) => b.active - a.active || a.name.localeCompare(b.name));
    }

    /** Makes the output with `id` the default, as picked in the output list. */
    activate(id) {
        const device = this._devices.get(id);
        if (device)
            this._mixer.change_output(device);
    }

    /** Id of the output the default sink plays on, or null. */
    _activeId() {
        const sink = this._mixer.get_default_sink();
        return sink ? this._mixer.lookup_device_from_stream(sink)?.get_id() ?? null : null;
    }

    /** Moves the volume and mute listeners to the current default sink. */
    _watchSink() {
        this._sinkIds.forEach(id => this._sink.disconnect(id));
        this._sinkIds = [];
        this._sink = this._mixer.get_default_sink();
        if (this._sink) {
            this._sinkIds = ['notify::volume', 'notify::is-muted'].map(signal =>
                this._sink.connect(signal, () => this.emit('changed')));
        }
        this._outputsChanged();
    }

    /** An output change is a change of everything shown too. */
    _outputsChanged() {
        this.emit('changed');
        this.emit('outputs-changed');
    }

    /** Lets go of the mixer and the sink. */
    destroy() {
        // Tell no one about changes made while tearing down.
        this.disconnectAll();
        this._sinkIds.forEach(id => this._sink.disconnect(id));
        this._mixerIds.forEach(id => this._mixer.disconnect(id));
    }
}
