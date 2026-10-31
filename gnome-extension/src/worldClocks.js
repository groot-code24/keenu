// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

// World clocks: the cities of GNOME's World Clocks (the calendar menu's, kept in step with the Clocks app).

import GLib from 'gi://GLib';
import Gio from 'gi://Gio';
import GWeather from 'gi://GWeather?version=4.0';

const SHELL_SCHEMA = 'org.gnome.shell.world-clocks';
const CLOCKS_SCHEMA = 'org.gnome.clocks';

/** GNOME Shell's list of cities, or null where the schema is missing. */
export function shellSettings() {
    const source = Gio.SettingsSchemaSource.get_default();
    return source?.lookup(SHELL_SCHEMA, true) ? new Gio.Settings({schema_id: SHELL_SCHEMA}) : null;
}

/** `{location, name, timezone}` per city with a time zone, west to east, as GNOME's calendar menu lists them. */
export function readCities(settings) {
    if (!settings)
        return [];
    const world = GWeather.Location.get_world();
    const now = GLib.DateTime.new_now_utc();
    const cities = [];
    for (const serialized of settings.get_value('locations').deepUnpack()) {
        const location = world?.deserialize(serialized);
        const timezone = location?.get_timezone();
        if (timezone)
            cities.push({location, name: location.get_city_name() || location.get_name(), timezone});
    }
    // Each city's UTC offset right now, summer time included.
    const offset = city => city.timezone.get_offset(city.timezone.find_interval(GLib.TimeType.UNIVERSAL, now.to_unix()));
    return cities.sort((a, b) => offset(a) - offset(b));
}

/** Every city GNOME knows, `{location, name, place, search}`, by name; `place` is "State, Country". */
export function allCities() {
    const cities = [];
    // Down through continents, countries and regions to their cities.
    const walk = parent => {
        for (let child = parent.next_child(null); child; child = parent.next_child(child)) {
            if (child.get_level() === GWeather.LocationLevel.CITY) {
                const name = child.get_city_name() || child.get_name();
                const region = parent.get_level() === GWeather.LocationLevel.ADM1 ? parent.get_name() : '';
                const place = [region, child.get_country_name() ?? ''].filter(Boolean).join(', ');
                cities.push({location: child, name, place, search: `${name} ${place}`.toLowerCase()});
            } else if (child.get_level() < GWeather.LocationLevel.CITY) {
                walk(child);
            }
        }
    };
    const world = GWeather.Location.get_world();
    if (world)
        walk(world);
    return cities.sort((a, b) => a.name.localeCompare(b.name));
}

/** Writes the cities where GNOME keeps them: the shell's list, and Clocks' own so the app shows the same. */
export function writeCities(settings, locations) {
    settings.set_value('locations', new GLib.Variant('av', locations.map(l => l.serialize())));
    const source = Gio.SettingsSchemaSource.get_default();
    if (source?.lookup(CLOCKS_SCHEMA, true)?.has_key('world-clocks')) {
        new Gio.Settings({schema_id: CLOCKS_SCHEMA}).set_value('world-clocks', new GLib.Variant('aa{sv}',
            locations.map(l => ({location: l.serialize()}))));
    }
}

/** "+2h", "−6h", "+5:30h": how far `timezone` is ahead of the local time now; "" when the same. */
export function offsetText(timezone, now = GLib.DateTime.new_now_local()) {
    const minutes = Math.round((now.to_timezone(timezone).get_utc_offset() - now.get_utc_offset()) / 60e6);
    if (minutes === 0)
        return '';
    const abs = Math.abs(minutes);
    const sign = minutes > 0 ? '+' : '−';
    return abs % 60 ? `${sign}${Math.floor(abs / 60)}:${String(abs % 60).padStart(2, '0')}h` : `${sign}${abs / 60}h`;
}

/** "Tomorrow" or "Yesterday" when the city's date is not the local one, else "". */
export function dayText(timezone, now = GLib.DateTime.new_now_local()) {
    const there = now.to_timezone(timezone);
    // A day number that orders dates across years.
    const key = d => d.get_year() * 400 + d.get_day_of_year();
    const diff = key(there) - key(now);
    return diff > 0 ? 'Tomorrow' : diff < 0 ? 'Yesterday' : '';
}
