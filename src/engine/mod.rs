//! Engine — the semantic compiler.
//!
//! Turns a `SourcePresentation` (intent) into a sequence of `ResolvedScene`s
//! (concrete draw instructions), one per frame.
//!
//! The engine understands time, animation, layout, and relationships.
//! It never deals with terminals, ANSI codes, or grids.

pub mod objects;
pub mod source;

use crate::types::{ResolvedScene, MAX_FRAME_OPS};
use objects::{Resolve, ResolveCtx};
use std::borrow::Cow;

use source::{AnimSpans, SceneObject, SourcePresentation};

pub struct Engine;

impl Engine {
    /// Compile a source presentation into resolved scenes, one per frame, with
    /// no per-frame op budget. For small, trusted decks (tests, examples);
    /// `compile::compile` and the editor go through the bounded [`scenes`].
    ///
    /// [`scenes`]: Self::scenes
    pub fn compile(source: &SourcePresentation) -> Vec<ResolvedScene> {
        Self::scenes_within(source, u64::MAX).map_while(Result::ok).collect()
    }

    /// The resolved scenes one frame at a time, so a caller can stop early
    /// without holding every frame's draw ops at once. A frame whose objects
    /// draw more than [`MAX_FRAME_OPS`] cells (counting overlaps) yields an
    /// error instead, checked object by object so its ops stay bounded.
    pub fn scenes(
        source: &SourcePresentation,
    ) -> impl Iterator<Item = Result<ResolvedScene, String>> + '_ {
        Self::scenes_within(source, MAX_FRAME_OPS)
    }

    fn scenes_within(
        source: &SourcePresentation,
        max_ops: u64,
    ) -> impl Iterator<Item = Result<ResolvedScene, String>> + '_ {
        // A group with an explicit range overrides its members' frame ranges;
        // apply that once (the same view the play-time sidecars read).
        let objects = source.effective_objects();
        // The animation-span table is the single source of truth for timing;
        // build it once and thread it into every coordinate evaluation.
        let anims = AnimSpans::of(source);
        (0..source.frame_count)
            .map(move |frame| Self::resolve_frame(source, &objects, frame, &anims, max_ops))
    }

    /// The error for a frame whose ops passed `MAX_FRAME_OPS`, if `ops` did.
    /// Shared with the editor preview, which resolves frames itself.
    pub fn check_frame_ops(frame: usize, ops: usize, max: u64) -> Result<(), String> {
        if ops as u64 > max {
            return Err(format!(
                "frame {} draws over {max} cells (counting overlapping objects); \
                 use fewer or smaller overlapping objects",
                frame + 1
            ));
        }
        Ok(())
    }

    fn resolve_frame(
        source: &SourcePresentation,
        objects: &[Cow<'_, SceneObject>],
        frame: usize,
        anims: &AnimSpans,
        max_ops: u64,
    ) -> Result<ResolvedScene, String> {
        let mut ops = Vec::new();
        let ctx = ResolveCtx {
            frame,
            canvas_width: source.width,
            canvas_height: source.height,
            anims,
        };

        for obj in objects {
            obj.resolve(&ctx, &mut ops);
            Self::check_frame_ops(frame, ops.len(), max_ops)?;
        }

        Ok(ResolvedScene {
            width: source.width,
            height: source.height,
            ops,
        })
    }
}
