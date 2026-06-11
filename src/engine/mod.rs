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
use source::{FrameRange, SourcePresentation};

pub struct Engine;

impl Engine {
    /// Compile a source presentation into resolved scenes, one per frame.
    pub fn compile(source: &SourcePresentation) -> Vec<ResolvedScene> {
        // A group with an explicit range overrides its members' frame ranges;
        // compute that mapping once and reuse it for every frame.
        let overrides = source.member_overrides();
        (0..source.frame_count)
            .map(|frame| Self::resolve_frame(source, frame, &overrides))
            .collect()
    }

    fn resolve_frame(
        source: &SourcePresentation,
        frame: usize,
        overrides: &[Option<FrameRange>],
    ) -> ResolvedScene {
        let mut ops = Vec::new();

        for (i, obj) in source.objects.iter().enumerate() {
            match overrides.get(i).and_then(|o| o.as_ref()) {
                // Member of an explicit-range group: render on the group's range
                // (a clone carries the substituted range through the object's own
                // self-gating) instead of the member's own range.
                Some(range) => {
                    if range.contains(frame) {
                        let mut member = obj.clone();
                        member.set_frame_range(range.clone());
                        member.resolve(frame, source.width, &mut ops);
                    }
                }
                None => obj.resolve(frame, source.width, &mut ops),
            }
        }

        ResolvedScene {
            width: source.width,
            height: source.height,
            ops,
        }
    }
}
