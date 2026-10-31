// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The conversation above the assistant's pill (what you said in bubbles, its answers, the commands it ran),
// and the card that asks before a command that changes the system runs.

import Cairo from 'cairo';
import Clutter from 'gi://Clutter';
import Cogl from 'gi://Cogl';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Pango from 'gi://Pango';
import St from 'gi://St';

import {BotIcon} from './botIcon.js';
import {VERTICAL, onUiScale, px} from './compat.js';
import * as Format from './format.js';
import {icon, iconButton, playMotion, slideBy, wrapLines} from './widgets.js';

/** The little face beside its answers, in CSS pixels (see `px`). */
const FACE_SIZE = 18;
/** Messages this far apart (ms) get the time between them. */
const TIME_GAP_MS = 5 * 60000;
/** A bubble of yours is at most this wide; longer ones wrap. */
const BUBBLE_MAX = 420;
/** What a risk badge says, and its icon (see `risksOf` in keenud). */
const RISKS = {
    removes: ['isle-trash', 'Removes files or apps'],
    internet: ['isle-link', 'Runs a script from the web'],
    root: ['isle-lock-symbolic', 'Needs your password'],
};
const STATUS = {
    done: ['isle-check-symbolic', 'Done', 'isle-card-done'],
    failed: ['isle-close-symbolic', 'Failed', 'isle-card-failed'],
    denied: ['isle-close-symbolic', 'Denied', 'isle-card-denied'],
    stopped: ['isle-stop-symbolic', 'Stopped', 'isle-card-denied'],
};

// A label in one call.
const label = (text, styleClass, params = {}) => new St.Label({text, style_class: styleClass, ...params});

/** How long (ms) "Copied" shows in place of the conversation's title. */
const COPIED_MS = 1200;
/** Run and Deny take no press this soon after the card shows. */
const ARMED_AFTER_MS = 500;
// A package manager stopped halfway can leave the system half changed, so its Stop asks for a second press
// within this long.
const STOP_ARMED_MS = 4000;
const INSTALLER = /(^|[\s;&|(])(apt|apt-get|aptitude|dpkg|dnf|yum|rpm|pacman|zypper|flatpak|snap)\s/;
/** A selection's colours: blue under white text. GNOME 47 moved colours from Clutter to Cogl. */
const Color = Clutter.Color ?? Cogl.Color;
const SELECTION = Color.from_string('#0a84ffb3')[1];
const SELECTED_TEXT = Color.from_string('#ffffff')[1];
/** A selection is copied once it has been still this long (ms) with the button up. */
const SELECT_SETTLE_MS = 150;

/** Puts `text` on the clipboard (Ctrl+V); not in the middle-click selection, so a stray middle click or
 * three-finger tap in the field never pastes it there. */
function copy(text) {
    St.Clipboard.get_default().set_text(St.ClipboardType.CLIPBOARD, text);
}

// Selectable text that copies a settled selection, as a terminal does; `settled` follows every press so the keys
// go back to the field. Watched through the selection, as GNOME 49+ gives text no button events of its own.
function copyable(actor, copied, settled) {
    const text = actor.clutter_text;
    text.set({selectable: true, reactive: true});
    text.set_selection_color(SELECTION);
    text.set_selected_text_color(SELECTED_TEXT);
    let timer = 0;
    let last = '';
    const settle = () => {
        timer = 0;
        const [,, mods] = global.get_pointer();
        if (mods & Clutter.ModifierType.BUTTON1_MASK) {
            timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, SELECT_SETTLE_MS, settle);
            return GLib.SOURCE_REMOVE;
        }
        const selected = text.get_selection();
        if (selected && selected !== last) {
            copy(selected);
            copied();
        }
        last = selected ?? '';
        settled();
        return GLib.SOURCE_REMOVE;
    };
    for (const property of ['notify::selection-bound', 'notify::cursor-position']) {
        text.connect(property, () => {
            if (!timer)
                timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, SELECT_SETTLE_MS, settle);
        });
    }
    text.connect('destroy', () => timer && GLib.source_remove(timer));
}

// A command as one press: a click copies all of it, as a terminal's copy button would.
function copyButton(child, command, copied) {
    const button = new St.Button({child, style_class: 'isle-copy-button', x_expand: true, can_focus: true,
        accessible_name: 'Copy the command'});
    button.connect('clicked', () => {
        copy(command());
        copied();
    });
    return button;
}

/** m:ss for an elapsed time in seconds. */
export const elapsed = seconds => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`;

// A box no wider than `max` (CSS px), as tall as its text wrapped at that width (St would measure one line).
const Bubble = GObject.registerClass(
class Bubble extends St.BoxLayout {
    /** `max` in CSS pixels. */
    _init(params, max) {
        super._init(params);
        this._max = max;
    }

    /** Never wider than `max`. */
    vfunc_get_preferred_width(forHeight) {
        const [min, natural] = super.vfunc_get_preferred_width(forHeight);
        const max = px(this._max);
        return [Math.min(min, max), Math.min(natural, max)];
    }

    /** Measured at the width it will really get, so wrapped lines count. */
    vfunc_get_preferred_height(forWidth) {
        const max = px(this._max);
        const width = forWidth < 0 ? max : Math.min(forWidth, max);
        const [, natural] = super.vfunc_get_preferred_height(width);
        return [natural, natural];
    }
});

// A thin line with light sweeping along it: a command runs, for however long it takes. Moved by the
// assistant's own frame clock (`tick`), so it costs nothing of its own.
const Sweep = GObject.registerClass(
class Sweep extends St.DrawingArea {
    /** Three pixels tall at any scale. */
    _init() {
        super._init({style_class: 'isle-card-sweep', x_expand: true});
        onUiScale(this, scale => (this.height = 3 * scale));
        this._time = 0;
        this.connect('repaint', area => this._paint(area));
    }

    /** `time` in seconds; repaints only while on screen. */
    tick(time) {
        this._time = time;
        if (this.mapped)
            this.queue_repaint();
    }

    /** A faint rounded track with a soft highlight moving left to right. */
    _paint(area) {
        const cr = area.get_context();
        const [width, height] = area.get_surface_size();
        const r = height / 2;
        cr.newSubPath();
        cr.arc(r, r, r, Math.PI / 2, 1.5 * Math.PI);
        cr.arc(width - r, r, r, -Math.PI / 2, Math.PI / 2);
        cr.closePath();
        cr.setSourceRGBA(1, 1, 1, 0.1);
        cr.fillPreserve();
        const at = (this._time * 0.55) % 1.4 - 0.2;
        const light = new Cairo.LinearGradient(0, 0, width, 0);
        for (const [offset, alpha] of [[at - 0.22, 0], [at, 0.85], [at + 0.22, 0]])
            light.addColorStopRGBA(Math.max(0, Math.min(1, offset)), 1, 1, 1, alpha);
        cr.setSource(light);
        cr.fill();
        cr.$dispose();
    }
});

/** An answer's light Markdown as Pango markup: **bold**, `code`, "- " lists, a [link](url) as its words.
 * Escaped first, so nothing in the text itself can be read as markup. */
export function answerMarkup(text) {
    return text.replace(/\[([^\]\n]+)\]\((?:https?:\/\/|www\.)[^)\s]+\)/g, '$1').split('\n').map(line => {
        const bullet = /^\s*[-*•]\s+/;
        const listed = bullet.test(line);
        // Code spans first, as written; bold only in the words between them, so the markup always nests.
        const body = (listed ? line.replace(bullet, '') : line).split(/(`[^`]+`)/).map((part, i) => i % 2
            ? `<span font_family="monospace" background="#ffffff" bgalpha="12%">${GLib.markup_escape_text(part.slice(1, -1), -1)}</span>`
            : GLib.markup_escape_text(part, -1).replace(/\*\*(.+?)\*\*/g, '<b>$1</b>')).join('');
        return listed ? `  •  ${body}` : body;
    }).join('\n');
}

/** How a command's words are coloured: a little like a shell's own highlighting. */
const SHELL = {
    prompt: ['#ffffff', 45],
    root: ['#ffb340', 100],
    program: ['#ffffff', 100],
    flag: ['#ffffff', 52],
    quoted: ['#64d2ff', 100],
    joiner: ['#ff6482', 100],
    word: ['#ffffff', 86],
};
/** A span of `text` in one of SHELL's colours (Pango wants its alpha apart). */
const span = ([color, alpha], text, bold = false) =>
    `<span foreground="${color}" fgalpha="${alpha}%"${bold ? ' weight="bold"' : ''}>${text}</span>`;
const JOINERS = new Set(['|', '||', '&&', ';', '>', '>>', '<', '2>&1']);

/** `command` as Pango markup: the program bright, sudo amber, flags dim, links and quotes blue, pipes pink. */
export function shellMarkup(command) {
    let first = true;
    const words = command.split(/(\s+)/).map(word => {
        if (/^\s+$/.test(word) || !word)
            return word;
        const text = GLib.markup_escape_text(word, -1);
        let color = SHELL.word;
        if (JOINERS.has(word)) {
            color = SHELL.joiner;
            first = true;
        } else if (word === 'sudo' || word === 'pkexec') {
            color = SHELL.root;
        } else if (first) {
            color = SHELL.program;
            first = false;
        } else if (word.startsWith('-')) {
            color = SHELL.flag;
        } else if (/^["']|:\/\//.test(word)) {
            color = SHELL.quoted;
        }
        return span(color, text, color === SHELL.program);
    });
    return `${span(SHELL.prompt, '$')} ${words.join('')}`;
}

/** What to know about a command, in its colour: an icon and a few words. */
function riskRow(risk, iconName, text) {
    const row = new St.BoxLayout({style_class: `isle-card-risk isle-card-risk-${risk}`});
    row.add_child(new St.Icon({gicon: icon(iconName), icon_size: 11, y_align: Clutter.ActorAlign.CENTER}));
    row.add_child(label(text, 'isle-card-risk-text', {y_align: Clutter.ActorAlign.CENTER}));
    return row;
}

/** A label with a key's name in a small box after it: Run ⏎. */
function keyButton(text, key, styleClass, onClick) {
    const box = new St.BoxLayout({style_class: 'isle-card-button-box'});
    const name = label(text, 'isle-card-button-label', {y_align: Clutter.ActorAlign.CENTER});
    box.add_child(name);
    const cap = label(key ?? '', 'isle-card-key', {y_align: Clutter.ActorAlign.CENTER, visible: Boolean(key)});
    box.add_child(cap);
    const button = new St.Button({child: box, style_class: `isle-pill isle-pill-small isle-card-button ${styleClass}${key ? '' : ' isle-card-button-plain'}`,
        can_focus: true, y_align: Clutter.ActorAlign.CENTER});
    button.connect('clicked', () => onClick());
    button.cap = cap;
    button.label_actor = name;
    return button;
}

// The card for one command, set straight on the pill. Waiting: why, the coloured command, one risk, Deny and Run;
// running: how long, its last lines and Stop; ended: one line saying how it went.
export const CommandCard = GObject.registerClass({
    Signals: {
        // Run (true) or Deny (false), pressed.
        'decided': {param_types: [GObject.TYPE_BOOLEAN]},
        'stop': {},
    },
}, class CommandCard extends St.BoxLayout {
    /** Head, terminal and foot, all made once and shown or hidden per state. */
    _init() {
        super._init({...VERTICAL, style_class: 'isle-card', x_expand: true});
        this.set_pivot_point(0.5, 0.5);

        // The head: what it is for and a line under it; gone once it ended (the answer says it).
        const head = new St.BoxLayout({style_class: 'isle-card-head'});
        this._head = head;
        const titles = new St.BoxLayout({...VERTICAL, x_expand: true, y_align: Clutter.ActorAlign.CENTER});
        this._title = label('', 'isle-card-title');
        this._title.clutter_text.ellipsize = Pango.EllipsizeMode.END;
        this._subtitle = label('', 'isle-card-subtitle');
        this._subtitle.clutter_text.ellipsize = Pango.EllipsizeMode.END;
        titles.add_child(this._title);
        titles.add_child(this._subtitle);
        head.add_child(titles);
        this.add_child(head);

        // The terminal: the command, then what it prints.
        this._terminal = new St.BoxLayout({...VERTICAL, style_class: 'isle-card-terminal'});
        // Its colours are markup, which the hyphen fix of `wrapLines` would undo at the start. Once ended, how it
        // went sits at the line's end: the card is that one line.
        const line = new St.BoxLayout({style_class: 'isle-card-line'});
        this._command = label('', 'isle-card-command', {x_expand: true, y_align: Clutter.ActorAlign.CENTER});
        this._command.clutter_text.set({use_markup: true, line_wrap_mode: Pango.WrapMode.WORD_CHAR});
        line.add_child(copyButton(this._command, () => this._commandText, () => this._flashCopied()));
        this._status = new St.BoxLayout({style_class: 'isle-card-status', y_align: Clutter.ActorAlign.CENTER});
        this._statusIcon = new St.Icon({icon_size: 11, y_align: Clutter.ActorAlign.CENTER});
        this._statusText = label('', 'isle-card-status-text', {y_align: Clutter.ActorAlign.CENTER});
        this._status.add_child(this._statusIcon);
        this._status.add_child(this._statusText);
        line.add_child(this._status);
        this._terminal.add_child(line);
        this._output = label('', 'isle-card-output');
        this._output.clutter_text.ellipsize = Pango.EllipsizeMode.END;
        this._terminal.add_child(this._output);
        this._sweep = new Sweep();
        this._terminal.add_child(this._sweep);
        this.add_child(this._terminal);

        // The foot: what to know on the left, the buttons on the right.
        this._foot = new St.BoxLayout({style_class: 'isle-card-foot'});
        this._risks = new St.BoxLayout({style_class: 'isle-card-risks', x_expand: true, y_align: Clutter.ActorAlign.CENTER});
        this._foot.add_child(this._risks);
        this._stopButton = keyButton('Stop', null, 'isle-card-ghost', () => this._stop());
        this._deny = keyButton('Deny', 'esc', 'isle-card-ghost', () => this.decide(false));
        this._run = keyButton('Run', '↩', 'isle-card-run', () => this.decide(true));
        for (const button of [this._stopButton, this._deny, this._run])
            this._foot.add_child(button);
        this.add_child(this._foot);
        this._state = null;
        this.connect('destroy', () => {
            this._destroyed = true;
            if (this._copiedTimer)
                GLib.source_remove(this._copiedTimer);
            this._disarmStop();
        });
    }

    /** Shows `card`: {command, why, detail, risks, status, output, seconds, exit}; status is waiting, running,
     * done, failed, denied or stopped. */
    setCard(card) {
        // Another card (the last one hidden while it waited) starts its own wait.
        const previous = card.id === this._id ? this._state : null;
        this._id = card.id;
        this._state = card.status;
        if (card.status === 'waiting' && previous !== 'waiting')
            this._waitingSince = GLib.get_monotonic_time();
        const waiting = card.status === 'waiting';
        const running = card.status === 'running';
        const ended = !waiting && !running;
        this._title.text = card.why;
        // Waiting, the line under the title says what to know; running, how long it has been at it.
        this._subtitle.text = waiting ? card.detail ?? '' : running ? `Running · ${elapsed(card.seconds ?? 0)}` : '';
        this._subtitle.visible = Boolean(this._subtitle.text);
        this._command.clutter_text.set_markup(shellMarkup(card.command));
        this._commandText = card.command;
        // Waiting, the whole command shows; once on its way, a line of it is enough.
        this._command.clutter_text.line_wrap = waiting;
        this._command.clutter_text.ellipsize = waiting ? Pango.EllipsizeMode.NONE : Pango.EllipsizeMode.END;

        this._risks.destroy_all_children();
        if (!running)
            this._disarmStop();
        if (running && this._stopArmed) {
            this._risks.add_child(riskRow('removes', 'isle-warning-symbolic', 'May leave it half installed'));
        } else if (waiting) {
            // One is enough beside the buttons: the most telling (see RISKS), removing before the web before a password.
            const risk = Object.keys(RISKS).find(key => card.risks?.includes(key));
            if (risk)
                this._risks.add_child(riskRow(risk, ...RISKS[risk]));
        }

        // Its last two lines as they print; on failure, the line that says why, in red.
        const lines = (card.output ?? '').split('\n').map(line => line.trimEnd()).filter(Boolean);
        this._output.text = running ? lines.slice(-2).join('\n') : card.status === 'failed' ? lines.at(-1) ?? '' : '';
        this._output.visible = Boolean(this._output.text);
        this._output.style_class = card.status === 'failed' ? 'isle-card-output isle-card-output-failed' : 'isle-card-output';
        this._sweep.visible = running;

        this._head.visible = !ended;
        const [iconName, text, styleClass] = STATUS[card.status] ?? [null, '', ''];
        this._status.visible = ended;
        if (ended) {
            this._statusIcon.gicon = icon(iconName);
            // Short, so the command keeps its line: a tick and how long, or the word.
            const done = card.status === 'done' && card.seconds >= 1 ? elapsed(card.seconds) : null;
            this._statusText.text = done ?? text;
            this._status.style_class = `isle-card-status ${styleClass}`;
        }
        this._foot.visible = waiting || running;
        this._deny.visible = this._run.visible = waiting;
        this._stopButton.visible = running;
        // Ended, it steps back: the answer above is what matters now.
        this.style_class = ended ? 'isle-card isle-card-ended' : 'isle-card';
        this.opacity = card.status === 'denied' || card.status === 'stopped' ? 165 : 255;
        if (ended && previous && previous !== card.status && St.Settings.get().enable_animations) {
            this._statusIcon.set_pivot_point(0.5, 0.5);
            this._statusIcon.set({scale_x: 0.4, scale_y: 0.4, opacity: 0});
            this._statusIcon.ease({scale_x: 1, scale_y: 1, opacity: 255, duration: 320, mode: Clutter.AnimationMode.EASE_OUT_BACK});
        }
    }

    /** The card's state as last set. */
    get status() {
        return this._state;
    }

    // Stop; for a package manager, a second press within STOP_ARMED_MS, with a line on why.
    _stop() {
        if (this._stopArmed || !INSTALLER.test(` ${this._commandText ?? ''}`)) {
            this._disarmStop();
            this.emit('stop');
            return;
        }
        this._stopArmed = GLib.timeout_add(GLib.PRIORITY_DEFAULT, STOP_ARMED_MS, () => {
            this._stopArmed = 0;
            this._showStop(false);
            return GLib.SOURCE_REMOVE;
        });
        this._showStop(true);
    }

    /** Back to a plain Stop. */
    _disarmStop() {
        if (this._stopArmed)
            GLib.source_remove(this._stopArmed);
        this._stopArmed = 0;
        this._showStop(false);
    }

    /** "Stop anyway" and the warning while armed. */
    _showStop(armed) {
        if (this._destroyed)
            return;
        this._stopButton.label_actor.text = armed ? 'Stop anyway' : 'Stop';
        this._risks.destroy_all_children();
        if (armed)
            this._risks.add_child(riskRow('removes', 'isle-warning-symbolic', 'May leave it half installed'));
    }

    // "Copied" for a moment where the card says how it is going: under the title, or at the line's end.
    _flashCopied() {
        const label = this._head.visible ? this._subtitle : this._statusText;
        const shown = label.text;
        const wasVisible = label.visible;
        if (this._copiedTimer)
            GLib.source_remove(this._copiedTimer);
        else
            this._beforeCopied = {label, shown, wasVisible};
        label.text = 'Copied';
        label.visible = true;
        label.add_style_class_name('isle-card-copied');
        if (label === this._statusText)
            this._status.visible = true;
        this._copiedTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, COPIED_MS, () => {
            this._copiedTimer = 0;
            const before = this._beforeCopied;
            before.label.text = before.shown;
            before.label.visible = before.wasVisible;
            before.label.remove_style_class_name('isle-card-copied');
            if (before.label === this._statusText)
                this._status.visible = !['waiting', 'running'].includes(this._state);
            return GLib.SOURCE_REMOVE;
        });
    }

    /** Moves the running sweep on, with the assistant's clock. */
    tick(time) {
        if (this._sweep.visible)
            this._sweep.tick(time);
    }

    /** Run (true) or Deny (false): a small press, or a shake; then it goes on. Also from Enter and Esc. */
    decide(run) {
        // An Enter or a click meant for something else never answers a card that just showed.
        if (this._state !== 'waiting' || GLib.get_monotonic_time() - this._waitingSince < ARMED_AFTER_MS * 1000)
            return;
        const button = run ? this._run : this._deny;
        if (St.Settings.get().enable_animations) {
            button.set_pivot_point(0.5, 0.5);
            button.ease({scale_x: 0.92, scale_y: 0.92, duration: 80, mode: Clutter.AnimationMode.EASE_OUT_QUAD,
                onComplete: () => !this._destroyed && button.ease({scale_x: 1, scale_y: 1, duration: 240,
                    mode: Clutter.AnimationMode.EASE_OUT_BACK})});
            if (!run)
                playMotion(this, 'shake');
        }
        this.emit('decided', run);
    }
});

// The conversation so far: your words right, in bubbles (a mic for spoken ones), its answers left, the
// commands it ran as short lines; + starts a new one. It scrolls, newest at the bottom.
export const ChatView = GObject.registerClass({
    // `settled`: a press on its text is over (a selection copied, or a plain click), so the keys go back.
    Signals: {'new-chat': {}, 'close': {}, 'settled': {}},
}, class ChatView extends St.BoxLayout {
    /** Header with New chat and Hide, then the scrolling list that keeps to its end. */
    _init() {
        super._init({...VERTICAL, style_class: 'isle-chat', x_expand: true});
        const header = new St.BoxLayout({style_class: 'isle-chat-header'});
        this._title = label('Chat', 'isle-chat-title', {x_expand: true, y_align: Clutter.ActorAlign.CENTER});
        header.add_child(this._title);
        header.add_child(iconButton('isle-plus-symbolic', 'isle-button-flat isle-chat-new', () => this.emit('new-chat'),
            {size: 13, accessibleName: 'New chat'}));
        header.add_child(iconButton('isle-chevron-down-symbolic', 'isle-button-flat isle-chat-new', () => this.emit('close'),
            {size: 13, accessibleName: 'Hide the conversation'}));
        this.add_child(header);
        this._list = new St.BoxLayout({...VERTICAL, style_class: 'isle-chat-list', x_expand: true});
        this._scroll = new St.ScrollView({
            style_class: 'isle-chat-scroll',
            hscrollbar_policy: St.PolicyType.NEVER,
            vscrollbar_policy: St.PolicyType.EXTERNAL,
            x_expand: true,
            y_expand: true,
        });
        this._scroll.set_child(this._list);
        // GNOME 49+ pans a scroll view by dragging, which would take every press from the text: dragging here
        // selects, to copy. The wheel and the touchpad still scroll it.
        for (const action of this._scroll.get_actions?.() ?? []) {
            if (Clutter.PanGesture && action instanceof Clutter.PanGesture)
                this._scroll.remove_action(action);
        }
        this.add_child(this._scroll);
        this._items = [];
        this.connect('destroy', () => this._copiedTimer && GLib.source_remove(this._copiedTimer));
        // New messages come in at the bottom, and it keeps there. The panel also shrinks as the command
        // card below it grows: read to its end then, the end stays in sight.
        const adjustment = this._scroll.vadjustment;
        let last = {value: 0, upper: 0, page: 0};
        const remember = () => (last = {value: adjustment.value, upper: adjustment.upper, page: adjustment.page_size});
        const toEnd = () => (adjustment.value = adjustment.upper - adjustment.page_size);
        adjustment.connect('notify::upper', () => {
            toEnd();
            remember();
        });
        adjustment.connect('notify::page-size', () => {
            if (last.value >= last.upper - last.page - px(2))
                toEnd();
            remember();
        });
        // A value clamped as the page changes is not the reader's: it waits for the page's own notice.
        adjustment.connect('notify::value', () => adjustment.page_size === last.page && remember());
        // Another scale: the rows are made again at its size.
        onUiScale(this, () => this.setItems(this._items), {now: false});
    }

    /** {shape, palette} for the little face beside its answers (see bot.js). */
    set look(look) {
        this._look = look;
        this.setItems(this._items);
    }

    /** The messages: [{role: user | assistant | command, text, spoken, command, status, at (ms)}]. A gap of a
     * while gets the time above it; its answers after yours get its face. */
    setItems(items) {
        // Those after the ones shown before are new: they rise into place.
        const known = this._items.length && items.length >= this._items.length && items[0] === this._items[0]
            ? this._items.length : items.length;
        this._items = items;
        this._list.destroy_all_children();
        let last = null;
        items.forEach((item, index) => {
            if (item.at && (!last?.at || item.at - last.at > TIME_GAP_MS))
                this._list.add_child(label(Format.timeOfDay(GLib.DateTime.new_from_unix_local(Math.floor(item.at / 1000))),
                    'isle-chat-time', {x_align: Clutter.ActorAlign.CENTER}));
            const row = this._row(item, last?.role !== 'assistant');
            this._list.add_child(row);
            if (index >= known && St.Settings.get().enable_animations) {
                const delay = 60 * (index - known);
                row.opacity = 0;
                slideBy(row, 'y', px(10), 0, {delay, duration: 260});
                row.ease({opacity: 255, duration: 260, delay, mode: Clutter.AnimationMode.EASE_OUT_CUBIC});
            }
            last = item;
        });
        this._title.text = 'Conversation';
    }

    /** Messages shown. */
    get count() {
        return this._items.length;
    }

    /** One message: your bubble, a command line, or an answer with or without the face. */
    _row(item, withFace) {
        if (item.role === 'user') {
            const bubble = new Bubble({style_class: 'isle-chat-user', x_align: Clutter.ActorAlign.END}, BUBBLE_MAX);
            const text = label(item.text ?? '', 'isle-chat-user-text', {x_expand: true});
            copyable(text, () => this._copied(), () => this.emit('settled'));
            wrapLines(text);
            bubble.add_child(text);
            return bubble;
        }
        if (item.role === 'command') {
            const row = new St.BoxLayout({style_class: 'isle-chat-command'});
            const [iconName, text, styleClass] = STATUS[item.status] ?? ['isle-terminal-symbolic', '', ''];
            row.add_child(new St.Widget({width: px(FACE_SIZE)}));
            const command = label('', 'isle-chat-command-text', {x_expand: true, y_align: Clutter.ActorAlign.CENTER});
            command.clutter_text.set_markup(shellMarkup(item.command));
            command.clutter_text.ellipsize = Pango.EllipsizeMode.END;
            row.add_child(copyButton(command, () => item.command, () => this._copied()));
            if (text) {
                const status = new St.BoxLayout({style_class: `isle-card-status isle-chat-command-status ${styleClass}`});
                status.add_child(new St.Icon({gicon: icon(iconName), icon_size: 10, y_align: Clutter.ActorAlign.CENTER}));
                status.add_child(label(text, 'isle-card-status-text', {y_align: Clutter.ActorAlign.CENTER}));
                row.add_child(status);
            }
            return row;
        }
        const row = new St.BoxLayout({style_class: 'isle-chat-reply-row'});
        // The face once per run of answers; a space as wide after it, so their text lines up.
        const face = withFace && this._look ? new BotIcon(FACE_SIZE, this._look) : new St.Widget({width: px(FACE_SIZE)});
        face.y_align = Clutter.ActorAlign.START;
        row.add_child(face);
        const text = label('', 'isle-chat-reply', {x_expand: true});
        copyable(text, () => this._copied(), () => this.emit('settled'));
        text.clutter_text.set({line_wrap: true, line_wrap_mode: Pango.WrapMode.WORD_CHAR, ellipsize: Pango.EllipsizeMode.NONE});
        text.clutter_text.set_markup(answerMarkup(item.text ?? ''));
        row.add_child(text);
        return row;
    }

    // "Copied", in the title's place for a moment.
    _copied() {
        this._title.text = 'Copied';
        this._title.add_style_class_name('isle-chat-copied');
        if (this._copiedTimer)
            GLib.source_remove(this._copiedTimer);
        this._copiedTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, COPIED_MS, () => {
            this._copiedTimer = 0;
            this._title.text = 'Conversation';
            this._title.remove_style_class_name('isle-chat-copied');
            return GLib.SOURCE_REMOVE;
        });
    }

    /** The height it would like at `width`, header and all, for the island to fit (never more than `max`). */
    naturalHeight(width, max) {
        const parts = [this.get_first_child(), this._list].map(part => part.get_preferred_height(width)[1]);
        const node = this.get_theme_node();
        return Math.min(max, Math.ceil(parts[0] + parts[1] + node.get_length('spacing') + node.get_vertical_padding()));
    }
});
