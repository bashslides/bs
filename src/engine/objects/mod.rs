//! Object types and their resolve implementations.
//!
//! Each object lives in its own module with its struct definition and
//! `Resolve` implementation side by side.

pub mod font;
mod arrow;
mod group;
mod header;
mod hline;
mod label;
mod rect;

pub use arrow::Arrow;
pub use group::Group;
pub use header::Header;
pub use hline::HLine;
pub use label::Label;
pub use rect::Rect;

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
        }
    }
}
