use serde::{Deserialize, Serialize};

use crate::types::{AnimationRegion, DrawOp};

use super::super::source::{AnimId, FrameRange};
use super::{Resolve, ResolveCtx};

fn default_delay_ms() -> u64 {
    500
}

fn default_true() -> bool {
    true
}

/// A play-time animation span.
///
/// Like `Loop`, an `Animation` is a runtime behavior that cannot be baked into
/// the static frames: it draws **nothing**. The actual motion lives in the
/// objects' `Coordinate::Animated` fields; an `Animation` records the *span*
/// those coordinates play over (`frames`) plus its **auto-play** config. It
/// emits an [`AnimationRegion`] sidecar (see [`Animation::region`]) telling the
/// `Player` to auto-advance across the span on a timer when `auto_play` is set.
///
/// Animations are a first-class unit so they can be reasoned about as a whole:
/// several may **overlap** freely (unlike loops), and a `Loop` may wrap a set of
/// them but must not bisect one — its range must contain each animation entirely
/// or not at all (enforced by
/// [`validate_loops`](super::super::source::SourcePresentation::validate_loops),
/// live in the editor and at compile time).
///
/// An `Animation` is created by the editor's *animate* sub-menu, not the
/// Add-Object menu; it is still selectable and editable like a `Loop` (it has a
/// frame range but renders nothing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Animation {
    /// Stable id referenced by the `Coordinate::Animated { anim }` of every
    /// coordinate this animation drives. The hard link between an animation and
    /// its motion — unique among the presentation's animations.
    pub id: AnimId,
    /// Frames the animation spans (end exclusive). This *is* the animation span,
    /// and the **single source of truth** for it — driven coordinates reference
    /// it by `id` rather than storing their own copy.
    pub frames: FrameRange,
    /// Whether the deck auto-advances across this span at play time.
    #[serde(default = "default_true")]
    pub auto_play: bool,
    /// Delay between auto-advanced frames, in milliseconds.
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u64,
    /// Editor metadata: the gap-frames strobe used (0 = none). The strobe itself
    /// is baked into the frames as single-frame copies of the element; this just
    /// records the setting so the animate menu can recover it. Ignored at runtime.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub gap_frames: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Animation {
    /// Resolve this animation into its runtime sidecar spec.
    pub fn region(&self) -> AnimationRegion {
        AnimationRegion {
            start_frame: self.frames.start,
            end_frame: self.frames.end,
            auto_play: self.auto_play,
            delay_ms: self.delay_ms,
        }
    }
}

impl Resolve for Animation {
    fn resolve(&self, _ctx: &ResolveCtx, _ops: &mut Vec<DrawOp>) {
        // An animation span draws nothing; it emits an `AnimationRegion` sidecar
        // (see `region`). The motion itself is on the objects' coordinates.
    }
}
