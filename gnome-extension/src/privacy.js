// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Mic, camera (from keenud), screen sharing and recording; all event driven.
// Mic streams that only meter input (sound settings) are ignored, as GNOME does.

import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as Signals from 'resource:///org/gnome/shell/misc/signals.js';
import {getMixerControl} from 'resource:///org/gnome/shell/ui/status/volume.js';

import {isWayland} from './compat.js';

/** Apps that open the microphone only to show its level. */
export const LEVEL_METERS = ['org.gnome.VolumeControl', 'org.PulseAudio.pavucontrol'];

/** Signals: `changed` (any state), `recording-started` (GNOME's screen recorder). */
export class PrivacyMonitor extends Signals.EventEmitter {
    /** Starts from what is in use now and follows each source's own signals. */
    constructor(daemon) {
        super();
        this._daemon = daemon;
        /** Screen-sharing handles and their `stopped` handler ids. */
        this._shares = new Map();
        this.microphone = false;
        this.camera = false;
        this.screenShare = false;
        this.recording = false;
        /** GLib monotonic time (µs) when the current recording started. */
        this.recordingSince = 0;

        this._mixer = getMixerControl();
        this._mixerIds = ['stream-added', 'stream-removed'].map(signal =>
            this._mixer.connect(signal, () => this._syncMicrophone()));

        this._daemonId = daemon.connect('changed', (_daemon, key) => {
            if (key === 'system')
                this._syncCamera();
        });

        this._screenshotUI = Main.screenshotUI;
        this._recordingId = this._screenshotUI.connect('notify::screencast-in-progress',
            () => this._syncRecording());

        // Screen sharing is only reliably known on Wayland, where Mutter
        // does the sharing itself; GNOME makes the same choice.
        this._remote = isWayland() ? global.backend.get_remote_access_controller() : null;
        this._remoteId = this._remote?.connect('new-handle', (_controller, handle) => this._onHandle(handle));
        this._seedShares();

        this._syncMicrophone();
        this._syncCamera();
        this._syncRecording();
    }

    /** Whether anything at all is in use, for showing indicators. */
    get any() {
        return this.microphone || this.camera || this.screenShare;
    }

    /** Stops GNOME's screen recording, from the island's stop button. */
    stopRecording() {
        this._screenshotUI.stopScreencast();
    }

    /** Any app recording sound, level meters aside. */
    _syncMicrophone() {
        const inUse = this._mixer.get_source_outputs()
            .some(stream => !LEVEL_METERS.includes(stream.get_application_id()));
        this._set('microphone', inUse);
    }

    /** The camera as keenud sees it. */
    _syncCamera() {
        this._set('camera', Boolean(this._daemon.state.system?.camera_in_use));
    }

    /** Notes when a screen recording starts or stops, and when it began. */
    _syncRecording() {
        const recording = this._screenshotUI.screencast_in_progress;
        if (recording === this.recording)
            return;
        this.recording = recording;
        this.recordingSince = recording ? GLib.get_monotonic_time() : 0;
        this.emit('changed');
        if (recording)
            this.emit('recording-started');
    }

    // A share started before Keenu (turned on again after unlocking, or updated mid-call) came as no
    // `new-handle`: GNOME's sharing indicator knows it (private: its `_handles`, else its being shown).
    _seedShares() {
        const indicator = Main.panel.statusArea.screenSharing;
        if (!this._remote || !indicator)
            return;
        if (indicator._handles instanceof Set) {
            indicator._handles.forEach(handle => this._onHandle(handle));
        } else if (indicator.visible) {
            this._indicator = indicator;
            this._indicatorId = indicator.connect('notify::visible', () => this._syncShares());
            this._syncShares();
        }
    }

    /** Shares known by handle, plus one only GNOME's indicator knows about, until it hides. */
    _syncShares() {
        const unknown = Boolean(this._indicator?.visible);
        if (this._indicator && !unknown) {
            this._indicator.disconnect(this._indicatorId);
            this._indicator = null;
        }
        this._set('screenShare', this._shares.size > 0 || unknown);
    }

    /** Counts a new screen share until it stops. */
    _onHandle(handle) {
        // Recordings are covered by the screen recorder state above.
        if (handle.is_recording)
            return;
        this._shares.set(handle, handle.connect('stopped', () => {
            handle.disconnect(this._shares.get(handle));
            this._shares.delete(handle);
            this._syncShares();
        }));
        this._set('screenShare', true);
    }

    /** Sets one state, emitting `changed` only on a real change. */
    _set(key, value) {
        if (this[key] === value)
            return;
        this[key] = value;
        this.emit('changed');
    }

    /** Disconnects from every source. */
    destroy() {
        // Tell no one about changes made while tearing down.
        this.disconnectAll();
        this._mixerIds.forEach(id => this._mixer.disconnect(id));
        this._daemon.disconnect(this._daemonId);
        this._screenshotUI.disconnect(this._recordingId);
        if (this._remoteId)
            this._remote.disconnect(this._remoteId);
        this._shares.forEach((id, handle) => handle.disconnect(id));
        this._indicator?.disconnect(this._indicatorId);
    }
}
