// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Square previews and colours for shelf files, decoded on a worker thread and
// cached as small PNGs in ~/.cache/keenu/shelf.

import GdkPixbuf from 'gi://GdkPixbuf';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GnomeDesktop from 'gi://GnomeDesktop';

import {typeColor, vividColor} from './colors.js';
import {forEachChild} from './widgets.js';

/** Previews are cut at this many pixels a side: twice the tile's, for HiDPI. */
const PIXELS = 104;
const ATTRIBUTES = 'standard::content-type,standard::icon,thumbnail::path,time::modified';

/** Runs an async call, `begin(callback)`, to the value of `finish(source, result)`. */
function call(begin, finish) {
    return new Promise((resolve, reject) => {
        begin((source, result) => {
            try {
                resolve(finish(source, result));
            } catch (e) {
                reject(e);
            }
        });
    });
}

/** The shelf's previews, each file loaded once and shared by every tile showing it. */
export class Thumbnails {
    /** Nothing is read until a tile asks. */
    constructor() {
        this._dir = GLib.build_filenamev([GLib.get_user_cache_dir(), 'keenu', 'shelf']);
        /** uri → Promise of what `get` returns. */
        this._loads = new Map();
        this._cancellable = new Gio.Cancellable();
    }

    /** `{gicon, image, whole, color, contentType}` for `uri` (`image` a preview path or null; `whole` when it is a
     * document's page, kept whole), or null if gone. */
    get(uri) {
        if (!this._loads.has(uri)) {
            this._loads.set(uri, this._load(uri).catch(e => {
                if (!e.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    console.warn(`Keenu: no preview for ${uri}: ${e.message}`);
                return null;
            }));
        }
        return this._loads.get(uri);
    }

    /** Forgets every file but those in `uris`, deleting their previews. */
    keepOnly(uris) {
        const kept = uris.join('\n');
        if (kept === this._kept)
            return;
        this._kept = kept;
        for (const uri of [...this._loads.keys()]) {
            if (!uris.includes(uri))
                this._loads.delete(uri);
        }
        const keep = new Set(uris.flatMap(uri => [this._previewName(uri), this._previewName(uri, true)]));
        forEachChild(this._dir, (child, info) => {
            if (!keep.has(info.get_name()))
                child.delete_async(GLib.PRIORITY_LOW, null, null);
        }, this._cancellable);
    }

    /** Cancels the loads still running. */
    destroy() {
        this._cancellable.cancel();
        this._loads.clear();
    }

    /** File name of a preview: the uri's MD5, as the desktop's own thumbnails do; a page kept whole apart. */
    _previewName(uri, whole = false) {
        return `${GLib.compute_checksum_for_string(GLib.ChecksumType.MD5, uri, -1)}${whole ? '-whole' : ''}.png`;
    }

    /** Looks the file up and makes its preview, from the cache when still fresh. */
    async _load(uri) {
        const file = Gio.File.new_for_uri(uri);
        const info = await call(
            callback => file.query_info_async(ATTRIBUTES, Gio.FileQueryInfoFlags.NONE, GLib.PRIORITY_LOW, this._cancellable, callback),
            (source, result) => source.query_info_finish(result));
        const contentType = info.get_content_type();
        const mime = (contentType && Gio.content_type_get_mime_type(contentType)) ?? '';
        // Pictures and videos fill the square; a document's page shows whole, as in Files.
        const whole = !mime.startsWith('image/') && !mime.startsWith('video/');
        const shown = {gicon: info.get_icon(), image: null, whole, color: typeColor(contentType), contentType};
        const modified = info.get_modification_date_time()?.to_unix() ?? 0;

        // The desktop's own thumbnail if Files made one, else the picture, else one made now as Files would.
        const source = info.get_attribute_byte_string('thumbnail::path') ??
            (mime.startsWith('image/') ? file.get_path() : null) ??
            await this._thumbnail(uri, mime, modified);
        if (!source)
            return shown;

        const preview = GLib.build_filenamev([this._dir, this._previewName(uri, whole)]);
        let square = await this._cached(preview, modified);
        if (!square) {
            try {
                square = await this._cut(source, whole);
                await this._save(square, preview);
            } catch (e) {
                // A picture GdkPixbuf cannot read (HEIC, RAW...) keeps its icon and colour.
                if (e.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                    throw e;
                return shown;
            }
        }
        shown.image = preview;
        shown.color = vividColor(square) ?? shown.color;
        return shown;
    }

    /** The preview made earlier, unless the file has changed since. */
    async _cached(path, modified) {
        const file = Gio.File.new_for_path(path);
        try {
            const info = await call(
                callback => file.query_info_async('time::modified', Gio.FileQueryInfoFlags.NONE, GLib.PRIORITY_LOW, this._cancellable, callback),
                (source, result) => source.query_info_finish(result));
            if ((info.get_modification_date_time()?.to_unix() ?? 0) < modified)
                return null;
            const stream = await call(
                callback => file.read_async(GLib.PRIORITY_LOW, this._cancellable, callback),
                (source, result) => source.read_finish(result));
            // Decoded on a worker thread.
            const pixbuf = await call(
                callback => GdkPixbuf.Pixbuf.new_from_stream_async(stream, this._cancellable, callback),
                (_source, result) => GdkPixbuf.Pixbuf.new_from_stream_finish(result));
            stream.close_async(GLib.PRIORITY_LOW, null, null);
            return pixbuf;
        } catch {
            return null;
        }
    }

    /** Writes `pixbuf` to `path` as a PNG, encoded on a worker thread. */
    async _save(pixbuf, path) {
        GLib.mkdir_with_parents(this._dir, 0o700);
        const stream = await call(
            callback => Gio.File.new_for_path(path).replace_async(null, false, Gio.FileCreateFlags.NONE,
                GLib.PRIORITY_LOW, this._cancellable, callback),
            (source, result) => source.replace_finish(result));
        await call(
            callback => pixbuf.save_to_streamv_async(stream, 'png', [], [], this._cancellable, callback),
            (_source, result) => GdkPixbuf.Pixbuf.save_to_stream_finish(result));
        // In place once closed.
        await call(
            callback => stream.close_async(GLib.PRIORITY_LOW, null, callback),
            (source, result) => source.close_finish(result));
    }

    /** A thumbnail of the file at `uri` made by the desktop's thumbnailers (a PDF's first page, a video's frame),
     * kept where Files keeps its own; its path, or null when there is none for its type or it fails. */
    _thumbnail(uri, mime, modified) {
        // One at a time, as Files makes them: many dropped at once never run that many thumbnailers.
        const made = (this._making ?? Promise.resolve()).then(() => this._makeThumbnail(uri, mime, modified));
        this._making = made.catch(() => null);
        return made;
    }

    async _makeThumbnail(uri, mime, modified) {
        this._factory ??= GnomeDesktop.DesktopThumbnailFactory.new(GnomeDesktop.DesktopThumbnailSize.LARGE);
        const factory = this._factory;
        if (!mime || !factory.can_thumbnail(uri, mime, modified) || factory.has_valid_failed_thumbnail(uri, modified))
            return null;
        try {
            const pixbuf = await call(
                callback => factory.generate_thumbnail_async(uri, mime, this._cancellable, callback),
                (source, result) => source.generate_thumbnail_finish(result));
            await call(
                callback => factory.save_thumbnail_async(pixbuf, uri, modified, this._cancellable, callback),
                (source, result) => source.save_thumbnail_finish(result));
        } catch (e) {
            if (e.matches?.(Gio.IOErrorEnum, Gio.IOErrorEnum.CANCELLED))
                throw e;
            // Not tried again until the file changes, as Files does.
            factory.create_failed_thumbnail_async(uri, modified, null, null);
            return null;
        }
        return factory.lookup(uri, modified);
    }

    /** The picture at `path`, `PIXELS` a side: its middle square, or `whole`, fitted on a clear square. */
    async _cut(path, whole = false) {
        const [, width, height] = await call(
            callback => GdkPixbuf.Pixbuf.get_file_info_async(path, this._cancellable, callback),
            (_source, result) => GdkPixbuf.Pixbuf.get_file_info_finish(result));
        if (!width || !height)
            throw new Error('not a picture');
        const scale = PIXELS / (whole ? Math.max(width, height) : Math.min(width, height));
        const scaledWidth = Math.max(whole ? 1 : PIXELS, Math.round(width * scale));
        const scaledHeight = Math.max(whole ? 1 : PIXELS, Math.round(height * scale));
        const stream = await call(
            callback => Gio.File.new_for_path(path).read_async(GLib.PRIORITY_LOW, this._cancellable, callback),
            (source, result) => source.read_finish(result));
        // Decoded and scaled on a worker thread.
        const pixbuf = await call(
            callback => GdkPixbuf.Pixbuf.new_from_stream_at_scale_async(stream, scaledWidth, scaledHeight, false, this._cancellable, callback),
            (_source, result) => GdkPixbuf.Pixbuf.new_from_stream_finish(result));
        stream.close_async(GLib.PRIORITY_LOW, null, null);
        if (whole) {
            const square = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, true, 8, PIXELS, PIXELS);
            square.fill(0);
            const x = Math.floor((PIXELS - scaledWidth) / 2), y = Math.floor((PIXELS - scaledHeight) / 2);
            pixbuf.add_alpha(false, 0, 0, 0).copy_area(0, 0, scaledWidth, scaledHeight, square, x, y);
            return square;
        }
        return pixbuf.new_subpixbuf(Math.floor((scaledWidth - PIXELS) / 2),
            Math.floor((scaledHeight - PIXELS) / 2), PIXELS, PIXELS).copy();
    }
}
