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
//!    `pub use super::objects::{…}` re-export, and (only if the object runs a
//!    binary at play time, like `Command`) handle it in `command_regions()`.
//! 4. **`src/editor/properties.rs`** — `impl Editable for <New>`, plus an arm
//!    in both `as_editable()` and `as_editable_mut()`.
//! 5. **`src/editor/object_defaults.rs`** — add the display name to
//!    `OBJECT_TYPES` and a construction arm in `create_default()`.
//! 6. **`src/editor/state.rs`** — add an arm to `object_type_name()`.
//! 7. **`src/editor/input.rs`** — only if the type needs special-case editing
//!    behaviour (e.g. the `Group`/`Table`/`Art` `matches!` checks). Plain
//!    types that edit through the `Editable` trait need nothing here.
//!
//! `panel.rs`, `menubar.rs`, and `preview.rs` are driven by `OBJECT_TYPES` and
//! the generic `Editable` dispatch, so they usually need no changes.

pub mod font;
mod arrow;
mod art;
mod command;
mod group;
mod header;
mod hline;
mod label;
mod list;
mod rect;
pub mod table;
mod wrap;

pub use arrow::Arrow;
pub use art::Art;
pub use command::Command;
pub use group::Group;
pub use header::Header;
pub use hline::HLine;
pub use label::Label;
pub use list::List;
pub use rect::Rect;
pub use table::Table;

use crate::types::DrawOp;

use super::source::SceneObject;

/// Resolve an object for a given frame into concrete `DrawOp`s.
pub trait Resolve {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>);
}

impl Resolve for SceneObject {
    fn resolve(&self, frame: usize, ops: &mut Vec<DrawOp>) {
        match self {
            SceneObject::Label(o) => o.resolve(frame, ops),
            SceneObject::HLine(o) => o.resolve(frame, ops),
            SceneObject::Rect(o) => o.resolve(frame, ops),
            SceneObject::Header(o) => o.resolve(frame, ops),
            SceneObject::Group(o) => o.resolve(frame, ops),
            SceneObject::Arrow(o) => o.resolve(frame, ops),
            SceneObject::Table(o) => o.resolve(frame, ops),
            SceneObject::Art(o) => o.resolve(frame, ops),
            SceneObject::Command(o) => o.resolve(frame, ops),
            SceneObject::List(o) => o.resolve(frame, ops),
        }
    }
}
