// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// The glass theme's backdrop: copies of what lies under the island (wallpaper, windows, top bar), each redrawn
// only there, blurred by GNOME Shell's blur and cut to the island's shape by a shader.

import Clutter from 'gi://Clutter';
import Cogl from 'gi://Cogl';
import GObject from 'gi://GObject';
import Graphene from 'gi://Graphene';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

// GNOME 46 and 47 name their snippet hooks themselves; later ones use Cogl's.
const FRAGMENT = Shell.SnippetHook?.FRAGMENT ?? Cogl.SnippetHook.FRAGMENT;
const UNIFORMS = ['width', 'height', 'left', 'right', 'radius', 'shoulder'];

const DECLARATIONS = UNIFORMS.map(name => `uniform float ${name};`).join('\n');
// Covered: the body (its bottom corners rounded) and the shoulders flaring out above it, each edge softened
// over a pixel, as Cairo draws the island's own outline (island.js `_outline`).
const CODE = `
vec2 p = cogl_tex_coord_in[0].xy * vec2(width, height);
float body = 0.0;
if (p.x >= left - 0.5 && p.x <= right + 0.5) {
    body = clamp(min(p.x - left, right - p.x) + 0.5, 0.0, 1.0);
    if (p.y > height - radius) {
        vec2 c = vec2(clamp(p.x, left + radius, right - radius), height - radius);
        body = min(body, clamp(0.5 - (length(p - c) - radius), 0.0, 1.0));
    }
}
float flare = 0.0;
if (shoulder > 0.0 && p.y < shoulder) {
    if (p.x < left && p.x > left - shoulder)
        flare = clamp(length(p - vec2(left - shoulder, shoulder)) - shoulder + 0.5, 0.0, 1.0);
    else if (p.x > right && p.x < right + shoulder)
        flare = clamp(length(p - vec2(right + shoulder, shoulder)) - shoulder + 0.5, 0.0, 1.0);
}
cogl_color_out *= max(body, flare);
`;

/** Sets the shape's uniforms, by location (GNOME 46–50) or by name (51 on); only when they changed. */
function setShape(effect, values) {
    if (values.every((value, i) => value === effect._values?.[i]))
        return;
    effect._values = values;
    UNIFORMS.forEach((name, i) => effect.set_uniform_float(effect._locations?.[i] ?? name, 1, [values[i]]));
    effect.queue_repaint();
}

/** Cuts what it is on to the island's shape: Shell's GLSL effect up to GNOME 50, Clutter's shader from 51. */
const ShapeMask = Shell.GLSLEffect
    ? GObject.registerClass(class ShapeMask extends Shell.GLSLEffect {
        _init() {
            super._init();
            this._locations = UNIFORMS.map(name => this.get_uniform_location(name));
        }

        vfunc_build_pipeline() {
            this.add_glsl_snippet(FRAGMENT, DECLARATIONS, CODE, false);
        }

        setShape(values) {
            setShape(this, values);
        }
    })
    : GObject.registerClass(class ShapeMask extends Clutter.ShaderEffect {
        vfunc_get_static_snippet() {
            return Cogl.Snippet.new(Cogl.SnippetHook.FRAGMENT, DECLARATIONS, CODE);
        }

        setShape(values) {
            setShape(this, values);
        }
    });

// What a copy of a window follows of it: a clone draws its source without the source's own fade and motion.
const MIRRORED = ['opacity', 'scale-x', 'scale-y', 'pivot-point', 'translation-x', 'translation-y'];

/** A copy that tells Clutter it draws only within `paintArea` (its own pixels), so a change in its source redraws
 * just that part of the screen; with `mirror`, it fades, scales and moves as its source does. */
const AreaClone = GObject.registerClass(
class AreaClone extends Clutter.Clone {
    _init(source, {mirror = false} = {}) {
        super._init({source});
        this._area = null;
        this._bindings = mirror
            ? MIRRORED.map(name => source.bind_property(name, this, name, GObject.BindingFlags.SYNC_CREATE)) : [];
        this.connect('destroy', () => this._bindings.forEach(binding => binding.unbind()));
    }

    set paintArea(area) {
        const old = this._area;
        if (old && ['x', 'y', 'width', 'height'].every(key => old[key] === area[key]))
            return;
        this._area = area;
        // Clutter keeps the last volume until told: the redraws would stay in the old area.
        this.invalidate_paint_volume();
    }

    vfunc_get_paint_volume(volume) {
        if (!this._area)
            return false;
        volume.set_origin(new Graphene.Point3D({x: this._area.x, y: this._area.y, z: 0}));
        volume.set_width(this._area.width);
        volume.set_height(this._area.height);
        return true;
    }
});

/** The blurred backdrop, copying nothing but for glass. The blur is on a frame a radius larger all round (its edges
 * blur what is there); the cut on this actor, clipped to the island, so its shader's pixels are the island's. */
export const GlassBackdrop = GObject.registerClass(
class GlassBackdrop extends St.Widget {
    _init() {
        super._init({visible: false, clip_to_allocation: true});
        this._pad = 0;
        this._on = false;
        this._rect = null;
        this._frame = new St.Widget({clip_to_allocation: true});
        this.add_child(this._frame);
        // The copies, at their places on the screen: the wallpaper, the windows under the island, the top bar.
        this._layers = new Clutter.Actor();
        this._frame.add_child(this._layers);
        this._blur = new Shell.BlurEffect({mode: Shell.BlurMode.ACTOR});
        this._frame.add_effect(this._blur);
        this._mask = new ShapeMask();
        this.add_effect(this._mask);
        this._wallpaper = null;
        this._bar = null;
        this._windows = new Map();
        this._watched = new Set();
        this._later = 0;
        // Going with the island (Keenu off, the shell shutting down): its signals and copies go with it; only a
        // pending look is taken back, as the shell may already be gone.
        this.connect('destroy', () => {
            this._on = false;
            if (this._later)
                global.compositor?.get_laters().remove(this._later);
            this._later = 0;
            this._watched.clear();
            this._windows.clear();
        });
    }

    /** Shows it blurred as `blur` says ({radius, brightness}), or takes it down for null. */
    set look(blur) {
        if (blur) {
            this._blur.radius = blur.radius;
            this._blur.brightness = blur.brightness;
            this._pad = blur.radius;
            this._start();
        } else {
            this._stop();
        }
        this._syncVisible();
    }

    /** Copies and follows what is behind the island. */
    _start() {
        if (this._on)
            return;
        this._on = true;
        this._wallpaper = new AreaClone(Main.layoutManager._backgroundGroup);
        this._bar = new AreaClone(Main.layoutManager.panelBox);
        this._layers.add_child(this._wallpaper);
        this._layers.add_child(this._bar);
        // Each let go of when either side goes, so nothing is left behind as the shell shuts down.
        const later = () => this._queueSync();
        global.window_group.connectObject('child-added', later, 'child-removed', later, this);
        global.display.connectObject('restacked', later, this);
        // A bar that hides or slides away (another extension) shows here as it is.
        Main.layoutManager.panelBox.connectObject('notify::allocation', later, 'notify::mapped', later,
            'notify::translation-y', later, this);
        // The overview hides the windows; the island shows its colour alone there.
        Main.overview.connectObject('showing', () => this._syncVisible(), 'hidden', () => this._syncVisible(), this);
        this._queueSync();
    }

    /** Lets go of every copy and signal: glass turned off. */
    _stop() {
        if (!this._on)
            return;
        this._on = false;
        if (this._later)
            global.compositor.get_laters().remove(this._later);
        this._later = 0;
        for (const object of [global.window_group, global.display, Main.layoutManager.panelBox, Main.overview])
            object.disconnectObject(this);
        this._watched.forEach(actor => actor.disconnectObject(this));
        this._watched.clear();
        this._windows.forEach(clone => clone.destroy());
        this._windows.clear();
        this._wallpaper?.destroy();
        this._bar?.destroy();
        this._wallpaper = this._bar = null;
    }

    _syncVisible() {
        this.visible = this._on && !Main.overview.visible;
    }

    /** Looks again at the windows once before the next frame, however many changed. */
    _queueSync() {
        if (!this._on || this._later)
            return;
        this._later = global.compositor.get_laters().add(Meta.LaterType.BEFORE_REDRAW, () => {
            this._later = 0;
            this._syncWindows();
            return false;
        });
    }

    /** Copies the windows that reach under the island, in their stacking order, and follows each window. */
    _syncWindows() {
        if (!this._on)
            return;
        const actors = global.get_window_actors();
        // Every window is followed (a move can bring one under the island); only those under it are copied.
        for (const actor of actors) {
            if (this._watched.has(actor))
                continue;
            this._watched.add(actor);
            actor.connectObject(
                'notify::allocation', () => this._queueSync(),
                'notify::visible', () => this._queueSync(),
                // A closed window's actor goes with its handlers: only forgotten here.
                'destroy', () => {
                    this._watched.delete(actor);
                    this._queueSync();
                }, this);
        }
        const rect = this._frameRect();
        const under = rect ? actors.filter(actor => actor.visible && !actor.meta_window?.is_override_redirect() &&
            actor.x < rect.x + rect.width && actor.x + actor.width > rect.x &&
            actor.y < rect.y + rect.height && actor.y + actor.height > rect.y) : [];
        for (const [actor, clone] of this._windows) {
            if (!under.includes(actor)) {
                clone.destroy();
                this._windows.delete(actor);
            }
        }
        let below = this._wallpaper;
        for (const actor of under) {
            let clone = this._windows.get(actor);
            if (!clone) {
                clone = new AreaClone(actor, {mirror: true});
                this._windows.set(actor, clone);
                this._layers.add_child(clone);
            }
            // Only when out of order: restacking always lays the copies out again.
            if (clone.get_previous_sibling() !== below)
                this._layers.set_child_above_sibling(clone, below);
            below = clone;
        }
        if (this._bar.get_previous_sibling() !== below)
            this._layers.set_child_above_sibling(this._bar, below);
        this._placeCopies();
    }

    /** Where the frame lies on the screen, or null before it is placed. */
    _frameRect() {
        if (!this._rect)
            return null;
        const {stageX, stageY, width, height} = this._rect, pad = this._pad;
        return {x: stageX - pad, y: stageY - pad, width: width + 2 * pad, height: height + 2 * pad};
    }

    /** Each copy at its source's place, drawing only over the frame. */
    _placeCopies() {
        const rect = this._frameRect();
        if (!rect)
            return;
        this._layers.set_position(-rect.x, -rect.y);
        const place = (clone, x, y) => {
            clone.set_position(x, y);
            clone.paintArea = {x: rect.x - x, y: rect.y - y, width: rect.width, height: rect.height};
        };
        place(this._wallpaper, 0, 0);
        const bar = Main.layoutManager.panelBox;
        this._bar.visible = bar.mapped;
        place(this._bar, bar.x + bar.translation_x, bar.y + bar.translation_y);
        this._windows.forEach((clone, actor) => place(clone, actor.x, actor.y));
    }

    /**
     * Over `width` × `height` of the island, at `stageX`, `stageY` on the screen: the body from `left` to `right`,
     * its bottom corners rounded by `radius`, shoulders `shoulder` wide.
     */
    place({stageX, stageY, width, height, left, right, radius, shoulder}) {
        if (!this._on)
            return;
        const old = this._rect;
        this._rect = {stageX, stageY, width, height};
        const pad = this._pad;
        this.set_size(width, height);
        this._frame.set_position(-pad, -pad);
        this._frame.set_size(width + 2 * pad, height + 2 * pad);
        this._mask.setShape([width, height, left, right, Math.min(radius, (right - left) / 2, height), shoulder]);
        // A new size can bring other windows under it.
        if (!old || old.stageX !== stageX || old.stageY !== stageY || old.width !== width || old.height !== height)
            this._syncWindows();
    }
});
