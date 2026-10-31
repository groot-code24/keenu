// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// Gio.DesktopAppInfo: GLib 2.86 (GNOME 49) moved it to GioUnix and warns on the
// old name, the only one GNOME 46 has. No shell imports: prefs share this file.

import Gio from 'gi://Gio';

let GioUnix = null;
try {
    ({default: GioUnix} = await import('gi://GioUnix?version=2.0'));
} catch {
    // Older GLib: the old name below is the only one.
}

export const DesktopAppInfo = GioUnix?.DesktopAppInfo ?? Gio.DesktopAppInfo;
