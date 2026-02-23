//! Engine — the semantic compiler.
//!
//! Turns a `SourcePresentation` (intent) into a sequence of `ResolvedScene`s
//! (concrete draw instructions), one per frame.
//!
//! The engine understands time, animation, layout, and relationships.
//! It never deals with terminals, ANSI codes, or grids.

pub mod objects;
pub mod source;

use crate::types::ResolvedScene;
use objects::Resolve;
use source::SourcePresentation;

pub struct Engine;

impl Engine {
    /// Compile a source presentation into resolved scenes, one per frame.
    pub fn compile(source: &SourcePresentation) -> Vec<ResolvedScene> {
        (0..source.frame_count)
            .map(|frame| Self::resolve_frame(source, frame))
            .collect()
    }

    fn resolve_frame(source: &SourcePresentation, frame: usize) -> ResolvedScene {
        let mut ops = Vec::new();

        for obj in &source.objects {
            obj.resolve(frame, &mut ops);
        }

        ResolvedScene {
            width: source.width,
            height: source.height,
            ops,
        }
    }
}
