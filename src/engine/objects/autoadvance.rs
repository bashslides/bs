use serde::{Deserialize, Serialize};

use crate::types::{AutoAdvanceRegion, DrawOp};

use super::super::source::FrameRange;
use super::{Resolve, ResolveCtx};

fn default_delay_ms() -> u64 {
    5000
}

/// A play-time auto-advance marker over a range of frames.
///
/// Like `Loop` and `Animation`, an `AutoAdvance` is a runtime behavior that
/// cannot be baked into the static frames: it draws **nothing**. Instead it
/// emits an [`AutoAdvanceRegion`] sidecar (see [`AutoAdvance::region`]) telling
/// the `Player` to advance to the next frame on its own, after `delay_ms`, for
/// every frame in `[frames.start, frames.end)` (end exclusive). The presenter
/// can still navigate manually with the arrow keys at any time; auto-advance is
/// suppressed on the last frame (there is nowhere to advance to) and while a
/// `Loop` drives playback.
///
/// An `AutoAdvance` is created from the editor's **frame** sub-menu (the
/// auto-advance action), not the Add-Object menu — so, like `Animation`, it is
/// absent from `OBJECT_TYPES`. It is still selectable and editable like a
/// `Loop` (it has a frame range but renders nothing). The frame sub-action
/// creates a single-frame marker; widening its range in the properties panel
/// makes a whole run of slides auto-advance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoAdvance {
    /// Frames on which the deck auto-advances (end exclusive).
    pub frames: FrameRange,
    /// Delay before advancing to the next frame, in milliseconds.
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u64,
}

impl AutoAdvance {
    /// Resolve this marker into its runtime sidecar spec.
    pub fn region(&self) -> AutoAdvanceRegion {
        AutoAdvanceRegion {
            start_frame: self.frames.start,
            end_frame: self.frames.end,
            delay_ms: self.delay_ms,
        }
    }
}

impl Resolve for AutoAdvance {
    fn resolve(&self, _ctx: &ResolveCtx, _ops: &mut Vec<DrawOp>) {
        // Auto-advance draws nothing; it emits an `AutoAdvanceRegion` sidecar
        // (see `region`) that the player consumes at play time.
    }
}
