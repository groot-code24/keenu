// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// App usage: times the application in front and sends its stretches to keenud, which keeps the days.
// Event driven: focus changes, the five-minute idle pause and its wake, and the Apps page asking move
// the session; stretches wait in the extension and go at most once a minute. See docs/product-spec.md.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';

import {AppUsageReports} from './appUsageReports.js';
import {AppUsageSession} from './appUsageSession.js';

/** A focused app counts until five minutes without input, unless the session is held awake (a video). */
const IDLE_MS = 5 * 60 * 1000;
/** How often buffered stretches go to keenud; the page asks too when it opens. */
const REPORT_MS = 60 * 1000;
/** The session manager's "idle" inhibit flag: a player holding the session awake. */
const INHIBIT_IDLE = 8;

/** Times the focused app, while `enabled`; keenud stores and sums it. */
export class AppUsage {
    constructor(daemon) {
        this._daemon = daemon;
        this._enabled = false;
        // Monotonic milliseconds for lengths; the epoch for the day a stretch began in.
        this._session = new AppUsageSession(
            () => Math.round(GLib.get_monotonic_time() / 1000),
            () => Math.round(GLib.get_real_time() / 1000));
        this._reports = new AppUsageReports();
        /** The last report in flight, so clearing can wait for it before forgetting. */
        this._pending = Promise.resolve();
        this._focusId = 0;
        this._idleWatch = 0;
        this._activeWatch = 0;
        this._monitor = null;
        this._idle = false;
        this._inhibited = false;
        this._reportTimer = 0;
        this._sessionProxy = null;
        this._sessionProxyId = 0;
        this._sessionCancel = null;
        /** Bumped by `clear`, so a report that failed before it is not put back after. */
        this._generation = 0;
    }

    get enabled() {
        return this._enabled;
    }

    /** Starts or stops timing; stopping sends what was not sent yet. */
    set enabled(enabled) {
        enabled = Boolean(enabled);
        if (enabled === this._enabled)
            return;
        this._enabled = enabled;
        if (enabled)
            this._start();
        else
            this._stop();
    }

    _start() {
        const tracker = Shell.WindowTracker.get_default();
        this._focusId = tracker.connect('notify::focus-app', () => this._onFocus());
        this._monitor = global.backend.get_core_idle_monitor();
        this._idle = this._monitor.get_idletime() >= IDLE_MS;
        this._idleWatch = this._monitor.add_idle_watch(IDLE_MS, () => this._onIdle());
        this._watchInhibitor();
        this._syncIdle();
        this._reportTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, REPORT_MS, () => {
            // The stretch so far goes too, so each report carries the whole last minute.
            this._reports.add(this._session.flush());
            this._flush();
            return GLib.SOURCE_CONTINUE;
        });
    }

    _stop() {
        this._reports.add(this._session.end());
        this._flush();
        this._enabled = false;
        if (this._focusId) {
            Shell.WindowTracker.get_default().disconnect(this._focusId);
            this._focusId = 0;
        }
        this._removeIdleWatches();
        this._unwatchInhibitor();
        if (this._reportTimer) {
            GLib.source_remove(this._reportTimer);
            this._reportTimer = 0;
        }
        this._reports.clear();
    }

    /** The app in front changed: time the new one, unless the user is away and nothing holds the session. */
    _onFocus() {
        if (!this._enabled)
            return;
        if (!this._canTime()) {
            this._reports.add(this._session.end());
            return;
        }
        this._reports.add(this._session.switchTo(this._focusedApp()));
    }

    /** Five minutes without input: stop timing, unless a video or another inhibitor keeps the session awake. */
    _onIdle() {
        if (!this._enabled)
            return;
        this._idle = true;
        this._syncIdle();
        this._watchActive();
    }

    /** Input returned: time the app in front again. */
    _onActive() {
        this._activeWatch = 0;
        this._idle = false;
        if (!this._enabled)
            return;
        this._reports.add(this._session.switchTo(this._focusedApp()));
    }

    /** Whether the app in front should be timed now. */
    _canTime() {
        return !this._idle || this._inhibited;
    }

    /** Starts or ends the session as the idle state and its inhibitor say. */
    _syncIdle() {
        if (!this._enabled)
            return;
        if (this._canTime()) {
            this._reports.add(this._session.switchTo(this._focusedApp()));
            return;
        }
        this._reports.add(this._session.end());
        this._watchActive();
    }

    /** Watches the session's inhibitors: a video player holds the session's idle flag. Asked without
     * waiting, so a busy session manager never holds up the shell. */
    _watchInhibitor() {
        const cancel = this._sessionCancel = new Gio.Cancellable();
        Gio.DBusProxy.new(Gio.DBus.session, Gio.DBusProxyFlags.NONE, null, 'org.gnome.SessionManager',
            '/org/gnome/SessionManager', 'org.gnome.SessionManager', cancel, (_source, res) => {
                let proxy;
                try {
                    proxy = Gio.DBusProxy.new_finish(res);
                } catch (e) {
                    // No session manager (a test shell), or timing stopped first: idle pauses as usual.
                    return;
                }
                if (cancel.is_cancelled()) {
                    proxy.run_dispose();
                    return;
                }
                this._sessionProxy = proxy;
                this._sessionProxyId = proxy.connect('g-properties-changed', (_proxy, changed) => {
                    if ('InhibitedActions' in changed.unpack())
                        this._syncInhibited();
                });
                this._syncInhibited();
            });
    }

    _unwatchInhibitor() {
        this._sessionCancel?.cancel();
        this._sessionCancel = null;
        if (this._sessionProxy && this._sessionProxyId)
            this._sessionProxy.disconnect(this._sessionProxyId);
        this._sessionProxy?.run_dispose();
        this._sessionProxy = null;
        this._sessionProxyId = 0;
        this._inhibited = false;
    }

    /** Updates whether an idle inhibitor holds the session, and times or pauses accordingly. */
    _syncInhibited() {
        const actions = this._sessionProxy?.get_cached_property('InhibitedActions')?.unpack() ?? 0;
        const inhibited = Boolean(actions & INHIBIT_IDLE);
        if (inhibited === this._inhibited)
            return;
        this._inhibited = inhibited;
        if (this._enabled && this._idle)
            this._syncIdle();
    }

    /** Watches for the input that ends an idle pause; only one at a time. */
    _watchActive() {
        if (this._activeWatch || !this._monitor)
            return;
        this._activeWatch = this._monitor.add_user_active_watch(() => this._onActive());
    }

    _removeIdleWatches() {
        if (this._monitor) {
            if (this._idleWatch)
                this._monitor.remove_watch(this._idleWatch);
            if (this._activeWatch)
                this._monitor.remove_watch(this._activeWatch);
        }
        this._idleWatch = 0;
        this._activeWatch = 0;
        this._monitor = null;
        this._idle = false;
    }

    /** The Apps page asks: send what is timed so far, then keep timing. */
    async flush() {
        if (!this._enabled)
            return;
        this._reports.add(this._session.flush());
        await this._flush();
    }

    /** Sends the buffered stretches in one call, if keenud is there to take them. */
    _flush() {
        if (!this._daemon.running || this._reports.size === 0)
            return Promise.resolve();
        const taken = this._reports.take();
        const generation = this._generation;
        const stretches = taken.map(({id, name, start, ms}) => [id, name, start, ms]);
        return (this._pending = this._daemon.attemptQuietly('appUsage', 'Report', stretches).then(({result, error,
            unknownMethod}) => {
            if (!result && error)
                console.warn(`Keenu: app usage report refused: ${error}`);
            // keenud went away mid-call, or one from before App usage runs until the next login: kept for the
            // keenud that takes them, unless cleared meanwhile.
            if (!result && (unknownMethod || !this._daemon.running) && generation === this._generation)
                this._reports.restore(taken);
        }));
    }

    /** Forgets what is recorded, then goes on timing the app in front; whether keenud forgot it. */
    async clear() {
        this._generation++;
        this._session.end();
        this._reports.clear();
        // A report already on its way would come back after the clear: wait for it to land first.
        await this._pending;
        const cleared = await this._daemon.call('appUsage', 'Clear');
        if (this._enabled && this._canTime())
            this._reports.add(this._session.switchTo(this._focusedApp()));
        return Boolean(cleared);
    }

    /** The app in front now as {id, name}, or null when no window has focus. */
    _focusedApp() {
        const app = Shell.WindowTracker.get_default().focus_app;
        if (!app)
            return null;
        const id = app.get_id() ?? '';
        if (id && !/^window:\d+$/.test(id))
            return {id, name: app.get_name() ?? ''};
        // No desktop file: GNOME's "window:N" changes each run and its name is the window's title, so the
        // WM class names it instead, steady between runs and with no document's title kept on disk.
        const wmClass = global.display.focus_window?.get_wm_class() ?? '';
        return wmClass ? {id: `window:${wmClass}`, name: wmClass.split('.').at(-1) || wmClass} : null;
    }

    /** Stops timing and sends the last stretch. */
    destroy() {
        this.enabled = false;
    }
}
