//! Object types and their resolve implementations.
//!
//! Each object lives in its own module with its struct definition and
//! `Resolve` implementation side by side.
//!
//! # Adding a new object type — checklist
//!
//! A `SceneObject` variant is referenced from several files, and the compiler
//! only catches *some* of the omissions (the `match` arms; not the lookup
//! tables or the `matches!` behaviour checks). Touch every site below:
//!
//! 1. **`src/engine/objects/<new>.rs`** — define the struct
//!    (`#[derive(Debug, Clone, Serialize, Deserialize)]`) and `impl Resolve`.
//! 2. **`src/engine/objects/mod.rs`** (this file) — add `mod <new>;`, a
//!    `pub use <new>::<New>;`, and an arm to `impl Resolve for SceneObject`.
//! 3. **`src/engine/source.rs`** — add the `SceneObject` variant, extend the
//!    `pub use super::objects::{…}` re-export, and (only if the object emits a
//!    play-time sidecar, like `Command`'s `command_regions()` or `Loop`'s
//!    `loop_regions()`) collect it there.
//! 4. **`src/editor/properties.rs`** — `impl Editable for <New>`, plus an arm
//!    in both `as_editable()` and `as_editable_mut()`.
//! 5. **`src/editor/object_defaults.rs`** — add an `AddableObjectType { name,
//!    shortcut }` entry to `OBJECT_TYPES` (a unique quick-add letter, not the
//!    global fullscreen `f`) and a construction arm in `create_default()`.
//!    Skip this only for a type created through a dedicated editor sub-menu
//!    rather than the Add-Object menu (like `Animation`/`AutoAdvance`).
//! 6. **`src/engine/source.rs`** — add the display name to
//!    `SceneObject::type_name()`.
//! 7. **`src/editor/input.rs`** — only if the type needs special-case editing
//!    behaviour (e.g. the `Group`/`Table`/`Art` `matches!` checks). Plain
//!    types that edit through the `Editable` trait need nothing here.
//! 8. **Docs + tests** — add the type to `PRESENTATION_FORMAT.md` (the §4
//!    catalog row, its own field-table section, and the §10 tag list), the
//!    object-type list in `AGENTS.md`, the `src/engine/objects/` row of
//!    `CLAUDE.md`'s module map, and `README.md`'s catalog if the user-facing
//!    list changes; write a `tests/<type>.rs` behaviour test and list it in
//!    `TESTS.md`. `tests/docs.rs` fails the build on a missing catalog row, a
//!    stale type count, or an unlisted test — but it cannot judge prose, so
//!    still describe the type properly.
//!
//! `panel.rs`, `menubar.rs`, and `preview.rs` are driven by `OBJECT_TYPES` and
//! the generic `Editable` dispatch, so they usually need no changes.
//!
//! **What's enforced.** The compiler catches the missing `match` arms (steps 2,
//! 3, 6, and `as_editable`). The lookup *tables* it can't see are guarded by
//! tests instead: `object_defaults::tests::object_type_registry_is_complete`
//! constructs one of every `SceneObject` variant (an exhaustive `match` there
//! makes a new variant fail to compile until you list it) and asserts the enum,
//! `OBJECT_TYPES`, and the documented sub-menu-only set name exactly the same
//! types. `create_default_covers_every_object_type` and
//! `properties::tests::*_properties_roundtrip` cover steps 4–5, and `tests/docs.rs`
//! covers step 8. So: add the variant, run `cargo test`, and let the failures
//! walk you through the rest.

pub mod font;
mod animation;
mod arrow;
mod autoadvance;
mod circle;
mod art;
mod command;
mod group;
mod header;
mod hline;
mod label;
mod list;
mod looping;
mod morph;
mod rect;
pub mod table;
mod wrap;

pub use animation::Animation;
pub use arrow::Arrow;
pub use autoadvance::AutoAdvance;
pub use circle::Circle;
pub use art::Art;
pub use command::Command;
pub use group::Group;
pub use header::Header;
pub use hline::HLine;
pub use label::{Label, TextAlign, VerticalAlign};
pub use list::List;
pub use looping::Loop;
pub use morph::{Morph, MorphMode};
pub use rect::Rect;
pub use table::Table;

use crate::types::DrawOp;

use super::source::{AnimSpans, SceneObject};

/// Everything an object needs to resolve itself for one frame.
///
/// `frame` is the frame being rendered; `canvas_width` is the width (in cells)
/// of the output frame (most objects ignore it — `Header` uses it to word-wrap
/// its large glyphs); `anims` maps each animation id to its span, so an
/// animated `Coordinate` can look up its timing (the span lives on the
/// `Animation` object, not on the coordinate).
pub struct ResolveCtx<'a> {
    pub frame: usize,
    pub canvas_width: u16,
    pub anims: &'a AnimSpans,
}

/// Resolve an object for a given frame into concrete `DrawOp`s.
pub trait Resolve {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>);
}

impl Resolve for SceneObject {
    fn resolve(&self, ctx: &ResolveCtx, ops: &mut Vec<DrawOp>) {
        match self {
            SceneObject::Label(o) => o.resolve(ctx, ops),
            SceneObject::HLine(o) => o.resolve(ctx, ops),
            SceneObject::Rect(o) => o.resolve(ctx, ops),
            SceneObject::Header(o) => o.resolve(ctx, ops),
            SceneObject::Group(o) => o.resolve(ctx, ops),
            SceneObject::Arrow(o) => o.resolve(ctx, ops),
            SceneObject::Table(o) => o.resolve(ctx, ops),
            SceneObject::Art(o) => o.resolve(ctx, ops),
            SceneObject::Command(o) => o.resolve(ctx, ops),
            SceneObject::List(o) => o.resolve(ctx, ops),
            SceneObject::Loop(o) => o.resolve(ctx, ops),
            SceneObject::Morph(o) => o.resolve(ctx, ops),
            SceneObject::Animation(o) => o.resolve(ctx, ops),
            SceneObject::AutoAdvance(o) => o.resolve(ctx, ops),
            SceneObject::Circle(o) => o.resolve(ctx, ops),
        }
    }
}
