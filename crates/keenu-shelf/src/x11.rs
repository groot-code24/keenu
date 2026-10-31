// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! Turning GTK's window into an override-redirect X11 window and placing it.

use std::cell::RefCell;

use gtk::prelude::*;
use gtk4 as gtk;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt as _, Window,
};
use x11rb::rust_connection::RustConnection;

type Error = Box<dyn std::error::Error>;

thread_local! {
    /// Our own connection to the X server, opened once; dropped after a failure, to open afresh.
    static CONNECTION: RefCell<Option<RustConnection>> = const { RefCell::new(None) };
}

/// Runs `request` on the cached connection, which a failure drops.
fn with_connection(
    request: impl FnOnce(&RustConnection) -> Result<(), Error>,
) -> Result<(), Error> {
    CONNECTION.with_borrow_mut(|cached| {
        let conn = match cached.take() {
            Some(conn) => conn,
            None => x11rb::connect(None)?.0,
        };
        request(&conn)?;
        *cached = Some(conn);
        Ok(())
    })
}

/// Makes the realized, unmapped window override-redirect: undecorated, unplaced
/// by the window manager, and stacked by GNOME Shell above its own chrome.
pub fn make_override_redirect(window: &gtk::Window) -> Result<(), Error> {
    let xid = xid(window)?;
    sync_gdk(window);
    let attributes = ChangeWindowAttributesAux::new().override_redirect(1);
    with_connection(|conn| Ok(conn.change_window_attributes(xid, &attributes)?.check()?))
}

/// Moves and resizes the window. Coordinates are in logical pixels.
pub fn place(window: &gtk::Window, x: i32, y: i32, width: i32, height: i32) -> Result<(), Error> {
    let scale = window.scale_factor();
    let xid = xid(window)?;
    sync_gdk(window);
    let geometry = ConfigureWindowAux::new()
        .x(x * scale)
        .y(y * scale)
        .width((width * scale) as u32)
        .height((height * scale) as u32);
    with_connection(|conn| Ok(conn.configure_window(xid, &geometry)?.check()?))
}

/// Flushes GDK's queued requests and waits for the X server: ours use another
/// connection and could beat the window's creation, or GDK's placement undoing ours.
fn sync_gdk(window: &gtk::Window) {
    WidgetExt::display(window).sync();
}

/// The X11 window id behind GTK's window; an error before it is realized or off X11.
fn xid(window: &gtk::Window) -> Result<Window, Error> {
    let surface = window
        .surface()
        .ok_or("the window has no surface")?
        .downcast::<gdk4_x11::X11Surface>()
        .map_err(|_| "the window is not an X11 window")?;
    Ok(Window::try_from(surface.xid())?)
}
