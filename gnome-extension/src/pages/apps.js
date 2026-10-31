// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// App usage: time in each application today, or over the last seven or 30 days, most used first.
// The extension times focus changes and keenud keeps the days (see appUsage.js and AppUsage1).

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import Pango from 'gi://Pango';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {VERTICAL} from '../compat.js';
import * as Format from '../format.js';
import {icon, label} from '../widgets.js';
import {Page} from './page.js';

const PERIODS = [
    {name: 'today', label: 'Today', total: 'today'},
    {name: 'week', label: '7 days', total: 'in 7 days'},
    {name: 'month', label: '30 days', total: 'in 30 days'},
];

// A row's bar: the fill is `share` (0..1) of the width.
const Bar = GObject.registerClass(class Bar extends St.Widget {
    _init(share) {
        super._init({style_class: 'isle-apps-track', y_align: Clutter.ActorAlign.CENTER});
        this._share = share;
        this._fill = new St.Widget({style_class: 'isle-apps-fill'});
        this.add_child(this._fill);
    }

    set share(share) {
        if (share === this._share)
            return;
        this._share = share;
        this.queue_relayout();
    }

    vfunc_allocate(box) {
        this.set_allocation(box);
        const [width, height] = box.get_size();
        // A sliver shows even for a minute beside hours.
        const filled = this._share > 0 ? Math.max(height, Math.round(width * this._share)) : 0;
        const fill = new Clutter.ActorBox();
        fill.set_origin(0, 0);
        fill.set_size(filled, height);
        this._fill.allocate(fill);
    }
});

// The Apps page: a heading with the period's total and a Today / 7 days / 30 days switch over the list.
export const AppsPage = GObject.registerClass(
class AppsPage extends Page {
    /** `height` is the expanded island's page height; `tracker` sends what is timed so far when opened. */
    _init(daemon, tracker, {height}) {
        super._init(daemon, {
            style_class: 'isle-page isle-apps',
            layout_manager: new Clutter.BoxLayout({orientation: Clutter.Orientation.VERTICAL}),
            height,
        });
        this.name = 'apps';
        this._tracker = tracker;
        this._period = 'today';
        this._key = null;
        this._rows = [];
        this._stats = null;

        this.add_child(this._buildBar());
        this.add_child(this._buildList());

        this._update();
    }

    /** The heading, the period's total, and the switch between today, 7 days and 30 days. */
    _buildBar() {
        const bar = new St.BoxLayout({style_class: 'isle-apps-bar'});
        bar.add_child(label('App Usage', 'isle-apps-heading', {x_expand: true}));
        this._total = label('', 'isle-apps-total');
        bar.add_child(this._total);

        const toggle = new St.BoxLayout({style_class: 'isle-segmented isle-apps-toggle'});
        this._buttons = new Map();
        for (const {name, label: text} of PERIODS) {
            const button = new St.Button({style_class: 'isle-segment', label: text, can_focus: true});
            button.connect('clicked', () => this._show(name));
            this._buttons.set(name, button);
            toggle.add_child(button);
        }
        bar.add_child(toggle);
        this._syncButtons();
        return bar;
    }

    /** The scrolling rows, with an empty state in their place when nothing is timed yet. */
    _buildList() {
        this._list = new St.BoxLayout({...VERTICAL, style_class: 'isle-apps-list'});
        this._scroll = new St.ScrollView({
            style_class: 'isle-apps-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            // Scrolls with the wheel or touchpad; no scrollbar over the rows.
            vscrollbar_policy: St.PolicyType.EXTERNAL,
            x_expand: true,
            y_expand: true,
        });
        this._scroll.set_child(this._list);

        this._empty = new St.BoxLayout({...VERTICAL, style_class: 'isle-empty',
            x_align: Clutter.ActorAlign.CENTER, y_align: Clutter.ActorAlign.CENTER, x_expand: true});
        this._empty.add_child(new St.Icon({
            gicon: icon('isle-apps'),
            icon_size: 24,
            style_class: 'isle-empty-icon',
            x_align: Clutter.ActorAlign.CENTER,
        }));
        this._empty.add_child(label('No usage yet', 'isle-empty-title', {x_align: Clutter.ActorAlign.CENTER}));
        this._empty.add_child(label('Time in the app in front shows up here',
            'isle-empty-hint', {x_align: Clutter.ActorAlign.CENTER}));

        const column = new St.Widget({layout_manager: new Clutter.BinLayout(), x_expand: true, y_expand: true});
        column.add_child(this._scroll);
        column.add_child(this._empty);
        return column;
    }

    /** The apps of the period shown, as keenud last summed them. */
    _apps() {
        return this._stats?.[this._period] ?? [];
    }

    /** Switches the period; the rows and heading follow. */
    _show(period) {
        if (period === this._period)
            return;
        this._period = period;
        this._syncButtons();
        this._update();
    }

    /** Marks the chosen period, as the shelf's tabs do. */
    _syncButtons() {
        for (const [name, button] of this._buttons) {
            if (name === this._period)
                button.add_style_pseudo_class('checked');
            else
                button.remove_style_pseudo_class('checked');
        }
    }

    /** Rebuilds the rows when what they show changed, and brings the total up to date. */
    _update() {
        const apps = this._apps();
        const total = apps.reduce((sum, app) => sum + Math.max(0, app.ms), 0);
        const period = PERIODS.find(({name}) => name === this._period);
        this._total.text = total > 0 ? `${Format.hoursMinutes(total)} ${period.total}` : '';
        this._empty.visible = apps.length === 0;
        this._scroll.visible = apps.length > 0;

        // The same apps in the same order (most minutes): their times and bars change in place.
        const most = Math.max(1, ...apps.map(app => app.ms));
        const key = JSON.stringify([this._period, apps.map(app => [app.id, app.name])]);
        if (key === this._key) {
            apps.forEach((app, i) => this._rows[i].update(app, most));
            return;
        }
        this._key = key;
        this._list.destroy_all_children();
        this._rows = apps.map(app => this._row(app, most));
    }

    /** Adds one app's row: its icon, name, a bar against the most used, and its time; returns its updater. */
    _row(app, most) {
        const row = new St.BoxLayout({style_class: 'isle-apps-row'});
        const shellApp = Shell.AppSystem.get_default().lookup_app(app.id);
        row.add_child(new St.Icon({
            gicon: shellApp?.get_icon() ?? icon('application-x-executable-symbolic'),
            icon_size: 18,
            style_class: 'isle-apps-icon',
            y_align: Clutter.ActorAlign.CENTER,
        }));

        const name = label(shellApp?.get_name() ?? app.name ?? app.id, 'isle-apps-name', {x_expand: true});
        name.clutter_text.ellipsize = Pango.EllipsizeMode.END;
        row.add_child(name);
        const bar = new Bar(Math.max(0, app.ms) / most);
        row.add_child(bar);
        const time = label(Format.hoursMinutes(app.ms), 'isle-apps-time');
        row.add_child(time);
        this._list.add_child(row);
        return {
            update: (next, top) => {
                bar.share = Math.max(0, next.ms) / top;
                time.text = Format.hoursMinutes(next.ms);
            },
        };
    }

    /** The page is on screen: send what is timed so far, ask for the totals, then ask once a minute. */
    onActivate() {
        // Opening sends the stretch being timed; the minutes after only ask. The tracker's own timer
        // is the one sender then, so a report never goes twice in a minute because the page is open.
        this._refresh({flush: true});
        this.later('refresh', 60000, () => this._tick());
    }

    /** While shown, asks again once a minute; the tracker sends its own report on its timer. */
    _tick() {
        this._refresh();
        this.later('refresh', 60000, () => this._tick());
    }

    /** Nothing is asked or rebuilt while it is closed. */
    onDeactivate() {
        this.cancel('refresh');
    }

    /** The history changed elsewhere (Settings cleared it): ask again while shown. */
    refresh() {
        if (this.active)
            this._refresh();
    }

    async _refresh({flush = false} = {}) {
        if (flush)
            await this._tracker?.flush();
        if (!this.active || this._destroyed)
            return;
        const result = await this.daemon.call('appUsage', 'GetStats');
        if (!this.active || this._destroyed)
            return;
        const stats = result?.[0];
        if (stats) {
            try {
                this._stats = JSON.parse(stats);
            } catch (e) {
                console.warn(`Keenu: invalid app usage: ${e.message}`);
            }
        }
        this._update();
    }
});
