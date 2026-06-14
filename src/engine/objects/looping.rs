use serde::{Deserialize, Serialize};

use crate::types::{DrawOp, LoopRegion};

use super::super::source::FrameRange;
use super::{Resolve, ResolveCtx};

fn default_delay_ms() -> u64 {
    500
}

fn default_true() -> bool {
    true
}

/// A play-time loop over a range of frames.
///
/// Like `Command`, a `Loop` is a runtime behavior that cannot be baked into the
/// static frames: it draws **nothing**. Instead it emits a [`LoopRegion`]
/// sidecar (see [`Loop::region`]) telling the `Player` to auto-advance — and,
/// when `bounce` is set, ping-pong back and forth — across `frames` on a timer,
/// until the presenter navigates out with an arrow key. The editor shows the
/// loop only as a selectable, range-editable object; nothing is rendered.
///
/// Loops may not overlap or nest (see
/// [`SourcePresentation::validate_loops`](super::super::source::SourcePresentation::validate_loops)),
/// which is enforced both in the editor and at compile time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Loop {
    /// Frames the loop spans (end exclusive). This *is* the loop range.
    pub frames: FrameRange,
    /// Delay between auto-advanced frames, in milliseconds.
    #[serde(default = "default_delay_ms")]
    pub delay_ms: u64,
    /// Number of times to play before moving on; `0` = loop forever.
    #[serde(default)]
    pub count: usize,
    /// Play the range forward then backward (`5,6,7,8,7,6,…`) instead of
    /// restarting (`5,6,7,8,5,6,…`). On by default.
    #[serde(default = "default_true")]
    pub bounce: bool,
}

impl Loop {
    /// Resolve this loop into its runtime sidecar spec.
    pub fn region(&self) -> LoopRegion {
        LoopRegion {
            start_frame: self.frames.start,
            end_frame: self.frames.end,
            delay_ms: self.delay_ms,
            count: self.count,
            bounce: self.bounce,
        }
    }
}

impl Resolve for Loop {
    fn resolve(&self, _ctx: &ResolveCtx, _ops: &mut Vec<DrawOp>) {
        // A loop draws nothing; it emits a `LoopRegion` sidecar (see `region`).
    }
}
