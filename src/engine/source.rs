//! Source presentation types — the human-authored semantic format.
//!
//! These types define *what exists* and *how it behaves*, not how it is drawn.
//! The engine reads these and resolves them into concrete `DrawOp`s per frame.

use serde::{Deserialize, Serialize};

// Re-export object types so they remain accessible via `engine::source::*`.
pub use super::objects::{
    Arrow, Art, Command, Group, HLine, Header, Label, List, Loop, Rect, Table,
};

use crate::types::{CommandRegion, LoopRegion};

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
    Art(Art),
    Command(Command),
    List(List),
    Loop(Loop),
}

impl SceneObject {
    /// The object's own declared frame range. A `Group` with an *auto* range
    /// (`frames = None`) has no declared range and returns `None`; every other
    /// object (and an explicit-range group) returns `Some`.
    pub fn declared_frame_range(&self) -> Option<FrameRange> {
        match self {
            SceneObject::Group(g) => g.frames.clone(),
            SceneObject::Label(l) => Some(l.frames.clone()),
            SceneObject::HLine(h) => Some(h.frames.clone()),
            SceneObject::Rect(r) => Some(r.frames.clone()),
            SceneObject::Header(h) => Some(h.frames.clone()),
            SceneObject::Arrow(a) => Some(a.frames.clone()),
            SceneObject::Table(t) => Some(t.frames.clone()),
            SceneObject::Art(a) => Some(a.frames.clone()),
            SceneObject::Command(c) => Some(c.frames.clone()),
            SceneObject::List(l) => Some(l.frames.clone()),
            SceneObject::Loop(l) => Some(l.frames.clone()),
        }
    }

    /// Overwrite the object's frame range. On a `Group` this sets an explicit
    /// range (`Some`); on every other type it replaces `frames`.
    pub fn set_frame_range(&mut self, r: FrameRange) {
        match self {
            SceneObject::Group(g) => g.frames = Some(r),
            SceneObject::Label(l) => l.frames = r,
            SceneObject::HLine(h) => h.frames = r,
            SceneObject::Rect(rc) => rc.frames = r,
            SceneObject::Header(h) => h.frames = r,
            SceneObject::Arrow(a) => a.frames = r,
            SceneObject::Table(t) => t.frames = r,
            SceneObject::Art(a) => a.frames = r,
            SceneObject::Command(c) => c.frames = r,
            SceneObject::List(l) => l.frames = r,
            SceneObject::Loop(l) => l.frames = r,
        }
    }
}

impl SourcePresentation {
    /// Effective frame range of object `i` — the range used to decide where it
    /// is visible. For most objects this is their declared range; for an *auto*
    /// group it is the union of its members' declared ranges. An empty/auto
    /// group with no usable member ranges yields an empty range (`0..0`).
    pub fn effective_frame_range(&self, i: usize) -> FrameRange {
        match self.objects.get(i) {
            Some(SceneObject::Group(g)) if g.frames.is_none() => self.group_derived_range(g),
            Some(o) => o
                .declared_frame_range()
                .unwrap_or(FrameRange { start: 0, end: 0 }),
            None => FrameRange { start: 0, end: 0 },
        }
    }

    /// Union of a group's members' declared ranges. Nested auto-groups (a group
    /// whose own range is itself auto) contribute nothing — kept non-recursive
    /// so a cyclic/self-referential member list can never loop.
    fn group_derived_range(&self, g: &Group) -> FrameRange {
        let mut start = usize::MAX;
        let mut end = 0usize;
        for &m in &g.members {
            if let Some(r) = self.objects.get(m).and_then(|o| o.declared_frame_range()) {
                start = start.min(r.start);
                end = end.max(r.end);
            }
        }
        if start == usize::MAX {
            FrameRange { start: 0, end: 0 }
        } else {
            FrameRange { start, end }
        }
    }

    /// Per-object frame-range override imposed by an explicit group range.
    /// `out[i] = Some(range)` means object `i` is a member of a group whose
    /// range is explicit, so it must render on `range` instead of its own.
    /// `None` means the object keeps its own range. Auto groups impose nothing.
    /// If an object belongs to several explicit groups, the last one wins.
    pub fn member_overrides(&self) -> Vec<Option<FrameRange>> {
        let mut out = vec![None; self.objects.len()];
        for obj in &self.objects {
            if let SceneObject::Group(g) = obj {
                if let Some(range) = &g.frames {
                    for &m in &g.members {
                        if m < out.len() {
                            out[m] = Some(range.clone());
                        }
                    }
                }
            }
        }
        out
    }

    /// Collect the runtime command specs from all `Command` objects, evaluated
    /// at each command's first active frame. These travel as a sidecar on the
    /// `PlayablePresentation` because they cannot be baked into static frames.
    pub fn command_regions(&self) -> Vec<CommandRegion> {
        self.objects
            .iter()
            .filter_map(|obj| match obj {
                SceneObject::Command(c) => Some(c.region(c.frames.start)),
                _ => None,
            })
            .collect()
    }

    /// Collect the runtime loop specs from all `Loop` objects. Like the command
    /// specs, these travel as a sidecar on the `PlayablePresentation` because a
    /// loop is a play-time navigation behavior, not something bakeable into the
    /// static frames.
    pub fn loop_regions(&self) -> Vec<LoopRegion> {
        self.objects
            .iter()
            .filter_map(|obj| match obj {
                SceneObject::Loop(l) => Some(l.region()),
                _ => None,
            })
            .collect()
    }

    /// Validate every `Loop` object's range: each must be non-empty, fit within
    /// the deck, and be **disjoint** from every other loop (loops may neither
    /// overlap nor nest). Returns an error describing the first problem found.
    ///
    /// Run both in the editor (to surface mistakes live) and at compile time (a
    /// hard gate before a playable is written).
    pub fn validate_loops(&self) -> Result<(), String> {
        let mut seen: Vec<(usize, usize)> = Vec::new();
        for obj in &self.objects {
            let SceneObject::Loop(l) = obj else { continue };
            let (s, e) = (l.frames.start, l.frames.end);
            if s >= e {
                return Err(format!("a loop has an empty range (frames {s}..{e})"));
            }
            if e > self.frame_count {
                return Err(format!(
                    "a loop range ({s}..{e}) extends past the {}-frame deck",
                    self.frame_count
                ));
            }
            // Disjoint check: two half-open ranges overlap iff each starts
            // before the other ends.
            for &(ps, pe) in &seen {
                if s < pe && ps < e {
                    return Err(format!(
                        "loops may not overlap or nest, but frames {ps}..{pe} and {s}..{e} do"
                    ));
                }
            }
            seen.push((s, e));
        }
        Ok(())
    }
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
