// SPDX-License-Identifier: GPL-3.0-or-later

//! A frame as data: what to draw, and where a click means what.

use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }

    pub fn inset(&self, by: f32) -> Self {
        Self::new(
            self.x + by,
            self.y + by,
            (self.w - by * 2.0).max(0.0),
            (self.h - by * 2.0).max(0.0),
        )
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// Straight sRGB, with alpha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// The island's palette, from the GNOME edition.
pub mod palette {
    use super::Color;

    pub const ISLAND: Color = Color::rgb(0, 0, 0);
    pub const TEXT: Color = Color::rgb(255, 255, 255);
    pub const MUTED: Color = Color::rgb(142, 142, 147);
    pub const FAINT: Color = Color::rgb(99, 99, 102);
    pub const BUTTON: Color = Color::rgb(44, 44, 46);
    pub const BUTTON_HOVER: Color = Color::rgb(58, 58, 60);
    pub const RAIL: Color = Color::rgb(58, 58, 60);
    pub const FOCUS: Color = Color::rgb(10, 132, 255);
    pub const GOOD: Color = Color::rgb(48, 209, 88);
    pub const WARN: Color = Color::rgb(255, 159, 10);
    pub const DANGER: Color = Color::rgb(255, 69, 58);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// How text sits in its rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub bold: bool,
    pub color: Color,
    pub align: Align,
    /// Wraps across lines instead of being cut with an ellipsis.
    pub wrap: bool,
    /// Fixed-width, for commands and code.
    pub mono: bool,
}

/// One thing to draw. Drawn in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// A rounded rectangle; `radius` of half the height makes a pill.
    Fill {
        rect: Rect,
        radius: f32,
        color: Color,
    },
    /// The island's own shape: square at the top, rounded at the bottom.
    Island {
        rect: Rect,
        radius: f32,
        color: Color,
    },
    /// An outline, for keyboard focus.
    Outline {
        rect: Rect,
        radius: f32,
        color: Color,
        width: f32,
    },
    Text {
        rect: Rect,
        text: String,
        style: TextStyle,
    },
    Icon {
        rect: Rect,
        icon: Icon,
        color: Color,
    },
    /// An image the host holds under `key`: the album cover, a thumbnail.
    Image {
        rect: Rect,
        radius: f32,
        key: String,
    },
}

/// A region that does something, and what a screen reader or the keyboard calls it.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit<A> {
    pub rect: Rect,
    pub action: A,
    pub label: String,
}

/// A whole frame, in logical pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene<A> {
    pub width: f32,
    pub height: f32,
    pub ops: Vec<Op>,
    /// Later ones lie on top.
    pub hits: Vec<Hit<A>>,
}

impl<A> Scene<A> {
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            ops: Vec::new(),
            hits: Vec::new(),
        }
    }

    /// What is at this point, if anything.
    pub fn hit_at(&self, x: f32, y: f32) -> Option<&Hit<A>> {
        self.hits.iter().rev().find(|hit| hit.rect.contains(x, y))
    }

    /// The text of every text op: what tests look for, and what a screen reader could be given.
    pub fn texts(&self) -> impl Iterator<Item = &str> {
        self.ops.iter().filter_map(|op| match op {
            Op::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }
}

/// `1:23` or `1:02:03`.
pub fn format_clock(ms: i64) -> String {
    let total = ms.max(0) / 1000;
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `3 h 20 min`.
pub fn format_duration(ms: i64) -> String {
    let minutes = ms.max(0) / 60_000;
    match (minutes / 60, minutes % 60) {
        (0, 0) => "under a minute".to_owned(),
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours} h"),
        (hours, minutes) => format!("{hours} h {minutes} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_topmost_region_takes_a_click() {
        let mut scene = Scene::new(100.0, 100.0);
        scene.hits.push(Hit {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            action: 1,
            label: String::new(),
        });
        scene.hits.push(Hit {
            rect: Rect::new(10.0, 10.0, 10.0, 10.0),
            action: 2,
            label: String::new(),
        });
        assert_eq!(scene.hit_at(15.0, 15.0).unwrap().action, 2);
        assert_eq!(scene.hit_at(50.0, 50.0).unwrap().action, 1);
        assert!(scene.hit_at(100.0, 100.0).is_none());
    }

    #[test]
    fn times_read_as_people_expect() {
        assert_eq!(format_clock(73_000), "1:13");
        assert_eq!(format_clock(3_723_000), "1:02:03");
        assert_eq!(format_clock(-5), "0:00");
        assert_eq!(format_duration(30_000), "under a minute");
        assert_eq!(format_duration(90 * 60_000), "1 h 30 min");
        assert_eq!(format_duration(60 * 60_000), "1 h");
    }
}
