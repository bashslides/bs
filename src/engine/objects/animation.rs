use serde::{Deserialize, Serialize};

use crate::types::{AnimationRegion, DrawOp};

use super::super::source::FrameRange;
use super::Resolve;

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
    /// Frames the animation spans (end exclusive). This *is* the animation span.
    pub frames: FrameRange,
    /// Whether the deck auto-advances across this span at play time.
    #[serde(default = "default_true")]
    pub auto_play: bool,
    /// Delay between auto-advanced frames, in milliseconds.
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u64,
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
    fn resolve(&self, _frame: usize, _canvas_width: u16, _ops: &mut Vec<DrawOp>) {
        // An animation span draws nothing; it emits an `AnimationRegion` sidecar
        // (see `region`). The motion itself is on the objects' coordinates.
    }
}
