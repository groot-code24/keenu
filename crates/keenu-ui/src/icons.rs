// SPDX-License-Identifier: GPL-3.0-or-later

//! The island's icons as filled polygons on a 24 by 24 grid, so every host draws them the same with
//! nothing but a polygon fill and no icon files to ship.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Music,
    Calendar,
    Clipboard,
    Shelf,
    Mic,
    Agent,
    Usage,
    Settings,
    Play,
    Pause,
    Next,
    Previous,
    Pin,
    Trash,
    Open,
    Folder,
    Close,
    Check,
    Warning,
    Stop,
}

type Point = (f32, f32);

/// A rectangle as a polygon.
const fn r(x: f32, y: f32, w: f32, h: f32) -> [Point; 4] {
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
}

/// The polygons that make up `icon`.
pub fn polygons(icon: Icon) -> Vec<Vec<Point>> {
    fn rects(list: &[[Point; 4]]) -> Vec<Vec<Point>> {
        list.iter().map(|rect| rect.to_vec()).collect()
    }
    match icon {
        Icon::Play => vec![vec![(7.0, 4.0), (20.0, 12.0), (7.0, 20.0)]],
        Icon::Pause => rects(&[r(6.0, 4.0, 4.5, 16.0), r(13.5, 4.0, 4.5, 16.0)]),
        Icon::Stop => rects(&[r(6.0, 6.0, 12.0, 12.0)]),
        Icon::Next => vec![
            vec![(4.0, 5.0), (12.0, 12.0), (4.0, 19.0)],
            vec![(12.0, 5.0), (20.0, 12.0), (12.0, 19.0)],
        ],
        Icon::Previous => vec![
            vec![(20.0, 5.0), (12.0, 12.0), (20.0, 19.0)],
            vec![(12.0, 5.0), (4.0, 12.0), (12.0, 19.0)],
        ],
        Icon::Music => {
            let mut shapes = rects(&[
                r(9.0, 5.0, 2.0, 12.0),
                r(18.0, 3.0, 2.0, 12.0),
                r(9.0, 4.0, 11.0, 2.5),
            ]);
            shapes.push(vec![
                (5.0, 16.0),
                (9.0, 15.5),
                (11.0, 17.0),
                (9.0, 20.0),
                (5.0, 20.0),
                (4.0, 18.0),
            ]);
            shapes.push(vec![
                (14.0, 14.0),
                (18.0, 13.5),
                (20.0, 15.0),
                (18.0, 18.0),
                (14.0, 18.0),
                (13.0, 16.0),
            ]);
            shapes
        }
        Icon::Calendar => rects(&[
            r(4.0, 5.0, 16.0, 3.0),
            r(4.0, 5.0, 2.0, 15.0),
            r(18.0, 5.0, 2.0, 15.0),
            r(4.0, 18.0, 16.0, 2.0),
            r(7.0, 3.0, 2.0, 4.0),
            r(15.0, 3.0, 2.0, 4.0),
            r(8.0, 11.0, 3.0, 3.0),
            r(13.0, 11.0, 3.0, 3.0),
        ]),
        Icon::Clipboard => rects(&[
            r(5.0, 5.0, 2.0, 16.0),
            r(17.0, 5.0, 2.0, 16.0),
            r(5.0, 19.0, 14.0, 2.0),
            r(5.0, 5.0, 14.0, 2.0),
            r(9.0, 3.0, 6.0, 4.0),
        ]),
        Icon::Shelf => {
            let mut shapes = rects(&[
                r(4.0, 14.0, 16.0, 2.0),
                r(4.0, 14.0, 2.0, 6.0),
                r(18.0, 14.0, 2.0, 6.0),
                r(4.0, 18.0, 16.0, 2.0),
                r(11.0, 3.0, 2.0, 7.0),
            ]);
            shapes.push(vec![(7.0, 9.0), (17.0, 9.0), (12.0, 14.0)]);
            shapes
        }
        Icon::Mic => rects(&[
            r(9.0, 3.0, 6.0, 11.0),
            r(5.0, 10.0, 2.0, 6.0),
            r(17.0, 10.0, 2.0, 6.0),
            r(5.0, 15.0, 14.0, 2.0),
            r(11.0, 17.0, 2.0, 3.0),
            r(8.0, 19.0, 8.0, 2.0),
        ]),
        Icon::Agent => vec![vec![
            (12.0, 2.0),
            (14.5, 9.5),
            (22.0, 12.0),
            (14.5, 14.5),
            (12.0, 22.0),
            (9.5, 14.5),
            (2.0, 12.0),
            (9.5, 9.5),
        ]],
        Icon::Usage => rects(&[
            r(4.0, 13.0, 4.0, 7.0),
            r(10.0, 8.0, 4.0, 12.0),
            r(16.0, 4.0, 4.0, 16.0),
        ]),
        Icon::Settings => {
            let mut shapes = rects(&[
                r(11.0, 2.0, 2.0, 4.0),
                r(11.0, 18.0, 2.0, 4.0),
                r(2.0, 11.0, 4.0, 2.0),
                r(18.0, 11.0, 4.0, 2.0),
            ]);
            shapes.push(vec![
                (12.0, 5.0),
                (17.0, 7.0),
                (19.0, 12.0),
                (17.0, 17.0),
                (12.0, 19.0),
                (7.0, 17.0),
                (5.0, 12.0),
                (7.0, 7.0),
                (12.0, 5.0),
                (12.0, 9.0),
                (9.8, 9.8),
                (9.0, 12.0),
                (9.8, 14.2),
                (12.0, 15.0),
                (14.2, 14.2),
                (15.0, 12.0),
                (14.2, 9.8),
                (12.0, 9.0),
            ]);
            shapes
        }
        Icon::Pin => vec![
            vec![
                (8.0, 3.0),
                (16.0, 3.0),
                (15.0, 11.0),
                (18.0, 14.0),
                (6.0, 14.0),
                (9.0, 11.0),
            ],
            vec![(11.0, 14.0), (13.0, 14.0), (12.5, 21.0), (11.5, 21.0)],
        ],
        Icon::Trash => rects(&[
            r(5.0, 6.0, 14.0, 2.0),
            r(9.0, 3.0, 6.0, 2.0),
            r(7.0, 9.0, 2.0, 12.0),
            r(11.0, 9.0, 2.0, 12.0),
            r(15.0, 9.0, 2.0, 12.0),
            r(7.0, 19.0, 10.0, 2.0),
        ]),
        Icon::Open => vec![
            r(4.0, 4.0, 2.0, 16.0).to_vec(),
            r(4.0, 18.0, 16.0, 2.0).to_vec(),
            vec![
                (10.0, 4.0),
                (20.0, 4.0),
                (20.0, 14.0),
                (17.0, 11.0),
                (11.0, 17.0),
                (9.0, 15.0),
                (15.0, 9.0),
                (12.0, 6.0),
            ],
        ],
        Icon::Folder => vec![vec![
            (3.0, 6.0),
            (9.0, 6.0),
            (11.0, 8.0),
            (21.0, 8.0),
            (21.0, 19.0),
            (3.0, 19.0),
        ]],
        Icon::Close => vec![
            vec![(5.0, 7.0), (7.0, 5.0), (19.0, 17.0), (17.0, 19.0)],
            vec![(17.0, 5.0), (19.0, 7.0), (7.0, 19.0), (5.0, 17.0)],
        ],
        Icon::Check => vec![vec![
            (4.0, 12.0),
            (6.0, 10.0),
            (10.0, 14.0),
            (18.0, 6.0),
            (20.0, 8.0),
            (10.0, 18.0),
        ]],
        Icon::Warning => vec![vec![(12.0, 3.0), (22.0, 20.0), (2.0, 20.0)]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_stays_on_its_grid() {
        for icon in [
            Icon::Music,
            Icon::Calendar,
            Icon::Clipboard,
            Icon::Shelf,
            Icon::Mic,
            Icon::Agent,
            Icon::Usage,
            Icon::Settings,
            Icon::Play,
            Icon::Pause,
            Icon::Next,
            Icon::Previous,
            Icon::Pin,
            Icon::Trash,
            Icon::Open,
            Icon::Folder,
            Icon::Close,
            Icon::Check,
            Icon::Warning,
            Icon::Stop,
        ] {
            let shapes = polygons(icon);
            assert!(!shapes.is_empty(), "{icon:?}");
            for shape in shapes {
                assert!(shape.len() >= 3, "{icon:?}");
                for (x, y) in shape {
                    assert!(
                        (0.0..=24.0).contains(&x) && (0.0..=24.0).contains(&y),
                        "{icon:?}"
                    );
                }
            }
        }
    }
}
