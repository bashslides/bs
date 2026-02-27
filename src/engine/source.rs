//! Source presentation types — the human-authored semantic format.
//!
//! These types define *what exists* and *how it behaves*, not how it is drawn.
//! The engine reads these and resolves them into concrete `DrawOp`s per frame.

use serde::{Deserialize, Serialize};

// Re-export object types so they remain accessible via `engine::source::*`.
pub use super::objects::{Arrow, Group, HLine, Header, Label, Rect, Table};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePresentation {
    pub width: u16,
    pub height: u16,
    pub frame_count: usize,
    pub objects: Vec<SceneObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SceneObject {
    Label(Label),
    HLine(HLine),
    Rect(Rect),
    Header(Header),
    Group(Group),
    Arrow(Arrow),
    Table(Table),
}

// ---------------------------------------------------------------------------
// Shared geometry primitives
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub x: Coordinate,
    pub y: Coordinate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coordinate {
    /// Fixed value stored as f64 so group-scaling can use fractional precision.
    /// Rendered by flooring to the nearest integer.
    Fixed(f64),
    Animated {
        from: u16,
        to: u16,
        start_frame: usize,
        end_frame: usize,
    },
}

/// Serde deserializer that accepts either a plain number (`5` or `5.5`) or a full
/// `Coordinate` object (`{"fixed":5}` / `{"animated":{…}}`).
/// Used on fields that were previously plain `u16`.
pub fn deserialize_coord_compat<'de, D>(d: D) -> Result<Coordinate, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, MapAccess, Visitor};
    use std::fmt;

    struct CoordVisitor;

    impl<'de> Visitor<'de> for CoordVisitor {
        type Value = Coordinate;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a number or a coordinate object")
        }

        fn visit_u64<E: Error>(self, v: u64) -> Result<Coordinate, E> {
            Ok(Coordinate::Fixed(v as f64))
        }

        fn visit_i64<E: Error>(self, v: i64) -> Result<Coordinate, E> {
            Ok(Coordinate::Fixed(v.max(0) as f64))
        }

        fn visit_f64<E: Error>(self, v: f64) -> Result<Coordinate, E> {
            Ok(Coordinate::Fixed(v.max(0.0)))
        }

        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Coordinate, A::Error> {
            Coordinate::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }

    d.deserialize_any(CoordVisitor)
}

impl Coordinate {
    /// Evaluate to a terminal column/row value. Fixed coordinates are floored;
    /// animated coordinates are linearly interpolated and rounded.
    pub fn evaluate(&self, frame: usize) -> u16 {
        match self {
            Coordinate::Fixed(v) => v.max(0.0).floor() as u16,
            Coordinate::Animated {
                from,
                to,
                start_frame,
                end_frame,
            } => {
                if frame <= *start_frame {
                    return *from;
                }
                if frame >= *end_frame {
                    return *to;
                }
                let progress =
                    (frame - start_frame) as f64 / (end_frame - start_frame) as f64;
                (*from as f64 + (*to as f64 - *from as f64) * progress).round() as u16
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameRange {
    pub start: usize,
    pub end: usize,
}

impl FrameRange {
    pub fn contains(&self, frame: usize) -> bool {
        frame >= self.start && frame < self.end
    }
}
