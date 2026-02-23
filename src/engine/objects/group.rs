use serde::{Deserialize, Serialize};

use crate::types::DrawOp;

use super::super::source::FrameRange;
use super::Resolve;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    /// Indices into `SourcePresentation::objects`.
    pub members: Vec<usize>,
    pub frames: FrameRange,
    #[serde(default)]
    pub z_order: i32,
}

impl Resolve for Group {
    fn resolve(&self, _frame: usize, _ops: &mut Vec<DrawOp>) {
        // Groups emit no DrawOps; their members render independently.
    }
}
