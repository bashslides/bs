//! Source presentation types — the human-authored semantic format.
//!
//! These types define *what exists* and *how it behaves*, not how it is drawn.
//! The engine reads these and resolves them into concrete `DrawOp`s per frame.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

// Re-export object types so they remain accessible via `engine::source::*`.
pub use super::objects::{
    Animation, Arrow, Art, AutoAdvance, Circle, Command, Group, HLine, Header, Label, List, Loop,
    Morph, MorphMode, Rect, Table, TextAlign, VerticalAlign,
};

use crate::types::{
    AnimationRegion, AutoAdvanceRegion, CommandRegion, LoopRegion, TerminalContract,
};

/// Most frames a deck may declare (see `SourcePresentation::validate_contract`).
pub const MAX_FRAME_COUNT: usize = 100_000;
/// Most cells a deck may rasterize in total: `frame_count × width × height`.
pub const MAX_DECK_CELLS: u64 = 50_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePresentation {
    pub width: u16,
    pub height: u16,
    pub frame_count: usize,
    pub objects: Vec<SceneObject>,
    /// Groups of object indices that are **linked**: a non-placement property
    /// edit (text, colour, art, …) on any member propagates to the others, while
    /// position/size/range/z-order stay per-object. Created by a *linked* paste;
    /// maintained through object deletion like `Group.members`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Vec<usize>>,
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
    Morph(Morph),
    Animation(Animation),
    AutoAdvance(AutoAdvance),
    Circle(Circle),
}

impl SceneObject {
    /// Stable, human-facing name for editor UI and diagnostics.
    ///
    /// Keep this mapping next to the enum rather than in an editor module: the
    /// source schema owns the set of object variants, while several consumers
    /// need a display name for the same variant.
    pub fn type_name(&self) -> &'static str {
        match self {
            SceneObject::Label(_) => "Label",
            SceneObject::HLine(_) => "HLine",
            SceneObject::Rect(_) => "Rect",
            SceneObject::Header(_) => "Header",
            SceneObject::Group(_) => "Group",
            SceneObject::Arrow(_) => "Arrow",
            SceneObject::Table(_) => "Table",
            SceneObject::Art(_) => "Art",
            SceneObject::Command(_) => "Command",
            SceneObject::List(_) => "List",
            SceneObject::Loop(_) => "Loop",
            SceneObject::Morph(_) => "Morph",
            SceneObject::Animation(_) => "Animation",
            SceneObject::AutoAdvance(_) => "AutoAdvance",
            SceneObject::Circle(_) => "Circle",
        }
    }

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
            SceneObject::Morph(m) => Some(m.frames.clone()),
            SceneObject::Animation(a) => Some(a.frames.clone()),
            SceneObject::AutoAdvance(a) => Some(a.frames.clone()),
            SceneObject::Circle(c) => Some(c.frames.clone()),
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
            SceneObject::Morph(m) => m.frames = r,
            SceneObject::Animation(a) => a.frames = r,
            SceneObject::AutoAdvance(a) => a.frames = r,
            SceneObject::Circle(c) => c.frames = r,
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
    /// An `Animation` member is never overridden: its own `frames` stay the
    /// single source of truth for its span ([`AnimSpans`]).
    pub fn member_overrides(&self) -> Vec<Option<FrameRange>> {
        let mut out = vec![None; self.objects.len()];
        for obj in &self.objects {
            if let SceneObject::Group(g) = obj {
                if let Some(range) = &g.frames {
                    for &m in &g.members {
                        if m < out.len() && !matches!(self.objects[m], SceneObject::Animation(_)) {
                            out[m] = Some(range.clone());
                        }
                    }
                }
            }
        }
        out
    }

    /// Every object as compiled: a member of an explicit-range group is a clone
    /// carrying the group's range ([`member_overrides`](Self::member_overrides)),
    /// everything else is borrowed as-is. The engine and every play-time sidecar
    /// read objects through this, so a member's drawn frames and its runtime
    /// region can never disagree.
    pub fn effective_objects(&self) -> Vec<Cow<'_, SceneObject>> {
        self.objects
            .iter()
            .zip(self.member_overrides())
            .map(|(obj, ov)| match ov {
                Some(range) => {
                    let mut member = obj.clone();
                    // A morph's range is also its blend timing: the group
                    // only gates it, so keep its own range as the timing.
                    if let SceneObject::Morph(m) = &mut member {
                        if m.timing.is_none() {
                            m.timing = Some(m.frames.clone());
                        }
                    }
                    member.set_frame_range(range);
                    Cow::Owned(member)
                }
                None => Cow::Borrowed(obj),
            })
            .collect()
    }

    /// Collect the runtime command specs from all `Command` objects. These
    /// travel as a sidecar on the `PlayablePresentation` because they cannot be
    /// baked into static frames. The geometry is evaluated per frame, and a
    /// command whose box moves or resizes (animated coordinates) yields one
    /// region per run of frames with the same geometry, so play-time output
    /// always lands in the box drawn on that frame. A static command is a
    /// single region over its whole range.
    pub fn command_regions(&self) -> Vec<CommandRegion> {
        let anims = AnimSpans::of(self);
        let geometry = |r: &CommandRegion| (r.x, r.y, r.w, r.h, r.status_x, r.status_y);
        let mut out = Vec::new();
        for obj in self.effective_objects() {
            let SceneObject::Command(c) = obj.as_ref() else { continue };
            // Frames past the deck never play; the last run keeps the range end.
            let mut cur = c.region(c.frames.start, &anims);
            for f in c.frames.start.saturating_add(1)..c.frames.end.min(self.frame_count) {
                let next = c.region(f, &anims);
                if geometry(&next) != geometry(&cur) {
                    cur.end_frame = f;
                    out.push(std::mem::replace(&mut cur, CommandRegion { start_frame: f, ..next }));
                }
            }
            out.push(cur);
        }
        out
    }

    /// Collect the runtime loop specs from all `Loop` objects. Like the command
    /// specs, these travel as a sidecar on the `PlayablePresentation` because a
    /// loop is a play-time navigation behavior, not something bakeable into the
    /// static frames.
    pub fn loop_regions(&self) -> Vec<LoopRegion> {
        self.effective_objects()
            .iter()
            .filter_map(|obj| match obj.as_ref() {
                SceneObject::Loop(l) => Some(l.region()),
                _ => None,
            })
            .collect()
    }

    /// Other objects linked to object `index` (its link family minus itself).
    /// Empty when the object is in no link group. Only valid object indices,
    /// each once: `links` comes from the file unchecked.
    pub fn link_siblings(&self, index: usize) -> Vec<usize> {
        let mut seen = std::collections::HashSet::new();
        self.links
            .iter()
            .find(|fam| fam.contains(&index))
            .map(|fam| {
                fam.iter()
                    .copied()
                    .filter(|&i| i != index && i < self.objects.len() && seen.insert(i))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Drop `Group.members` entries that name no object (or repeat within a
    /// group). Returns whether anything was dropped. Like [`Self::sanitize_links`],
    /// the editor runs it on open, so a stale member is never used or saved back.
    pub fn sanitize_group_members(&mut self) -> bool {
        let n = self.objects.len();
        let mut changed = false;
        for obj in &mut self.objects {
            if let SceneObject::Group(g) = obj {
                let mut seen = std::collections::HashSet::new();
                let before = g.members.len();
                g.members.retain(|&m| m < n && seen.insert(m));
                changed |= g.members.len() != before;
            }
        }
        changed
    }

    /// Drop `links` entries that name no object (or repeat within a family),
    /// then families left with fewer than two members. Returns whether anything
    /// was dropped. The editor runs it on open, so a hand-edited file's stale
    /// entry is never used or saved back.
    pub fn sanitize_links(&mut self) -> bool {
        let n = self.objects.len();
        let before = self.links.clone();
        for fam in &mut self.links {
            let mut seen = std::collections::HashSet::new();
            fam.retain(|&i| i < n && seen.insert(i));
        }
        self.links.retain(|fam| fam.len() >= 2);
        self.links != before
    }

    /// Collect the runtime animation specs from all `Animation` objects. Like
    /// loops, these travel as a sidecar on the `PlayablePresentation` (the span +
    /// auto-play config is a play-time behavior; the motion itself is already
    /// baked into the frames via the objects' animated coordinates).
    pub fn animation_regions(&self) -> Vec<AnimationRegion> {
        self.objects
            .iter()
            .filter_map(|obj| match obj {
                SceneObject::Animation(a) => Some(a.region()),
                _ => None,
            })
            .collect()
    }

    /// Collect the runtime auto-advance specs from all `AutoAdvance` objects.
    /// Like loops and animations, these travel as a sidecar on the
    /// `PlayablePresentation` — auto-advance is a play-time navigation behavior
    /// that draws nothing into the static frames.
    pub fn auto_advance_regions(&self) -> Vec<AutoAdvanceRegion> {
        self.effective_objects()
            .iter()
            .filter_map(|obj| match obj.as_ref() {
                SceneObject::AutoAdvance(a) => Some(a.region()),
                _ => None,
            })
            .collect()
    }

    /// Check the deck-level contract before anything is materialised: at least
    /// one frame, at most [`MAX_FRAME_COUNT`] frames, a canvas within
    /// [`MAX_CANVAS_CELLS`](crate::types::MAX_CANVAS_CELLS), and
    /// `frame_count × width × height` within [`MAX_DECK_CELLS`] (compile
    /// rasterizes every cell of every frame). The
    /// one place these limits live: `compile` rejects a deck that fails, the
    /// editor refuses such a frame size and skips the preview of such a deck.
    pub fn validate_contract(&self) -> Result<(), String> {
        if self.frame_count == 0 {
            return Err("deck has no frames (frame_count is 0)".into());
        }
        if self.frame_count > MAX_FRAME_COUNT {
            return Err(format!(
                "frame_count {} exceeds the limit of {MAX_FRAME_COUNT}",
                self.frame_count
            ));
        }
        TerminalContract { width: self.width, height: self.height }.validate()?;
        let total = u64::from(self.width) * u64::from(self.height) * self.frame_count as u64;
        if total > MAX_DECK_CELLS {
            return Err(format!(
                "{} frames of {}×{} is {total} cells; the limit is {MAX_DECK_CELLS}",
                self.frame_count, self.width, self.height
            ));
        }
        Ok(())
    }

    /// Validate every `Loop` object's range: each must be non-empty, fit within
    /// the deck, **disjoint** from every other loop (loops may neither overlap
    /// nor nest), and must not **bisect an animation** — a loop replays whole
    /// animations, so every `Animation` span must be either fully inside the
    /// loop or fully outside it (never half-in). It also rejects two `Animation`
    /// objects sharing an `id` (coordinates would follow one span while both
    /// auto-play). Returns an error describing the first problem found.
    ///
    /// Run both in the editor (to surface mistakes live) and at compile time (a
    /// hard gate before a playable is written).
    pub fn validate_loops(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for obj in &self.objects {
            if let SceneObject::Animation(a) = obj {
                if !ids.insert(a.id) {
                    return Err(format!(
                        "animation id {} is used by more than one animation",
                        a.id
                    ));
                }
            }
        }
        let objects = self.effective_objects();
        let animations: Vec<(usize, usize)> = objects
            .iter()
            .filter_map(|o| match o.as_ref() {
                SceneObject::Animation(a) => Some((a.frames.start, a.frames.end)),
                _ => None,
            })
            .collect();

        let mut seen: Vec<(usize, usize)> = Vec::new();
        for obj in &objects {
            let SceneObject::Loop(l) = obj.as_ref() else { continue };
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
            // A loop must not cut an animation in half: an animation either sits
            // entirely within the loop or entirely outside it. A partial overlap
            // (they intersect, yet the animation is not contained) is rejected.
            for &(as_, ae) in &animations {
                let intersects = s < ae && as_ < e;
                let contained = s <= as_ && ae <= e;
                if intersects && !contained {
                    return Err(format!(
                        "a loop ({s}..{e}) must not cut an animation ({as_}..{ae}) in half"
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

/// Stable identifier of an [`Animation`] object. An animated coordinate carries
/// this id to reference the animation that owns its span — the single source of
/// truth for *when* the motion plays. (`from`/`to` — *where* it moves — stay on
/// the coordinate, since they are per-object.)
pub type AnimId = u32;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coordinate {
    /// Fixed value stored as f64 so group-scaling can use fractional precision.
    /// Rendered by flooring to the nearest integer.
    Fixed(f64),
    /// Linear motion from `from` to `to` over the span owned by the [`Animation`]
    /// with id `anim`. The span lives only on that animation — never here — so a
    /// coordinate can never disagree with its animation about timing.
    Animated { from: u16, to: u16, anim: AnimId },
}

/// A lookup from [`AnimId`] to the animation's frame span, built once from a
/// presentation's [`Animation`] objects. Threaded into [`Coordinate::evaluate`]
/// so an animated coordinate can resolve its timing from the single source of
/// truth (the `Animation` object) rather than storing a copy of the span.
#[derive(Debug, Default, Clone)]
pub struct AnimSpans {
    spans: std::collections::HashMap<AnimId, FrameRange>,
}

impl AnimSpans {
    /// Build the table from every `Animation` object in `source`.
    pub fn of(source: &SourcePresentation) -> Self {
        let spans = source
            .objects
            .iter()
            .filter_map(|o| match o {
                SceneObject::Animation(a) => Some((a.id, a.frames.clone())),
                _ => None,
            })
            .collect();
        AnimSpans { spans }
    }

    /// Build a table directly from `(id, span)` pairs — for callers (and tests)
    /// that have spans in hand without a full presentation.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (AnimId, FrameRange)>) -> Self {
        AnimSpans { spans: pairs.into_iter().collect() }
    }

    /// The span of animation `id`, if it exists.
    pub fn span(&self, id: AnimId) -> Option<&FrameRange> {
        self.spans.get(&id)
    }
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
    /// animated coordinates look up their span in `anims` and are linearly
    /// interpolated and rounded. An animated coordinate whose animation is
    /// missing (a dangling reference) holds at `from` — it renders as if static.
    pub fn evaluate(&self, frame: usize, anims: &AnimSpans) -> u16 {
        match self {
            Coordinate::Fixed(v) => v.max(0.0).floor() as u16,
            Coordinate::Animated { from, to, anim } => {
                let Some(span) = anims.span(*anim) else {
                    return *from;
                };
                // `FrameRange::end` is exclusive; the motion reaches `to` on the
                // last frame it covers, i.e. `end - 1`.
                let start = span.start;
                let end_frame = span.end.saturating_sub(1);
                if frame <= start {
                    return *from;
                }
                if frame >= end_frame {
                    return *to;
                }
                let progress = (frame - start) as f64 / (end_frame - start) as f64;
                (*from as f64 + (*to as f64 - *from as f64) * progress).round() as u16
            }
        }
    }

    /// The coordinate's value independent of animation timing — `Fixed` floored,
    /// `Animated` at its `from` (its position at the span start). For display and
    /// summaries that have no [`AnimSpans`] handy and only need a stable label.
    pub fn start_value(&self) -> u16 {
        match self {
            Coordinate::Fixed(v) => v.max(0.0).floor() as u16,
            Coordinate::Animated { from, .. } => *from,
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
