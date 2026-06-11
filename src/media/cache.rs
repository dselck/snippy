use crate::media::types::YuvPlanes;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CachedFrame {
    pub planes: std::sync::Arc<YuvPlanes>,
    pub pts_sec: f64,
}

pub struct GopCache {
    pub keyframe_idx: usize,
    pub frames: HashMap<usize, CachedFrame>,
}

impl GopCache {
    pub fn new() -> Self {
        Self {
            keyframe_idx: usize::MAX,
            frames: HashMap::new(),
        }
    }

    pub fn get_cached(&self, frame_idx: usize) -> Option<&CachedFrame> {
        self.frames.get(&frame_idx)
    }

    pub fn insert_frame(&mut self, frame_idx: usize, frame: CachedFrame) {
        if frame_idx >= self.keyframe_idx {
            self.frames.insert(frame_idx, frame);
        }
    }

    pub fn clear_if_different_gop(&mut self, target_kf: usize) {
        if self.keyframe_idx != target_kf {
            self.frames.clear();
            self.keyframe_idx = target_kf;
        }
    }
}
