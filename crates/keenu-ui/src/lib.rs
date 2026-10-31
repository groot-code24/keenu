// SPDX-License-Identifier: GPL-3.0-or-later

//! The island on hosts that draw it themselves (Windows and macOS; on Linux it is the GNOME extension).
//!
//! [`view::build`] turns what should be shown into a [`scene::Scene`]: shapes, text and clickable regions,
//! in logical pixels. A host only paints the scene and reports clicks, so every decision about what is
//! on screen is here and can be tested without a display. The look follows `docs/product-spec.md`.

pub mod icons;
pub mod scene;
pub mod view;
