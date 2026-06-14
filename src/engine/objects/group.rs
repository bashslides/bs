use serde::{Deserialize, Serialize};

use crate::types::DrawOp;

use super::super::source::FrameRange;
use super::{Resolve, ResolveCtx};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Indices into `SourcePresentation::objects`.
    pub members: Vec<usize>,
    /// Frame range of the group.
    ///
    /// * `None` — *auto*: the group has no range of its own; its members render
    ///   on their own frame ranges, and the group's effective span is the union
    ///   of its members' ranges (see [`SourcePresentation::effective_frame_range`]).
    /// * `Some(range)` — *explicit*: the range **overrides** every member, which
    ///   then renders on `range` instead of its own
    ///   (see [`SourcePresentation::member_overrides`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frames: Option<FrameRange>,
    #[serde(default)]
    pub z_order: i32,
}

impl Resolve for Group {
    fn resolve(&self, _ctx: &ResolveCtx, _ops: &mut Vec<DrawOp>) {
        // Groups emit no DrawOps; their members render independently.
    }
}
