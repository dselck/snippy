pub struct HwFrameWrapper {
    pub frame: ffmpeg_next::util::frame::Video,
}

impl std::fmt::Debug for HwFrameWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HwFrameWrapper(format: {:?})", self.frame.format())
    }
}

unsafe impl Send for HwFrameWrapper {}
unsafe impl Sync for HwFrameWrapper {}

#[derive(Clone, Debug)]
pub enum GpuFrameDescriptor {
    #[cfg(target_os = "macos")]
    CVPixelBuffer(*const std::ffi::c_void),
    // macOS VideoToolbox HW frame
    #[cfg(target_os = "macos")]
    MacOsHwFrame(std::sync::Arc<HwFrameWrapper>),
    // Linux generic HW frame (VAAPI, CUDA, Vulkan, VDPAU)
    #[cfg(target_os = "linux")]
    LinuxHwFrame(std::sync::Arc<HwFrameWrapper>),
}

unsafe impl Send for GpuFrameDescriptor {}
unsafe impl Sync for GpuFrameDescriptor {}

#[derive(Clone, Default, Debug)]
pub struct YuvPlanes {
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub y_stride: u32,
    pub u_stride: u32,
    pub v_stride: u32,
}

#[derive(Clone, Debug)]
pub enum DecodedFrame {
    Cpu {
        frame_idx: usize,
        planes: std::sync::Arc<YuvPlanes>,
        sequence_id: usize,
        pts_sec: f64,
        color_space: ffmpeg_next::color::Space,
        color_range: ffmpeg_next::color::Range,
        is_concealed: bool,
        held_frame_idx: Option<usize>,
    },
    Gpu {
        frame_idx: usize,
        descriptor: GpuFrameDescriptor,
        sequence_id: usize,
        pts_sec: f64,
        color_space: ffmpeg_next::color::Space,
        color_range: ffmpeg_next::color::Range,
        is_concealed: bool,
        held_frame_idx: Option<usize>,
    },
}

impl DecodedFrame {
    pub fn frame_idx(&self) -> usize {
        match self {
            DecodedFrame::Cpu { frame_idx, .. } => *frame_idx,
            DecodedFrame::Gpu { frame_idx, .. } => *frame_idx,
        }
    }

    pub fn sequence_id(&self) -> usize {
        match self {
            DecodedFrame::Cpu { sequence_id, .. } => *sequence_id,
            DecodedFrame::Gpu { sequence_id, .. } => *sequence_id,
        }
    }

    pub fn pts_sec(&self) -> f64 {
        match self {
            DecodedFrame::Cpu { pts_sec, .. } => *pts_sec,
            DecodedFrame::Gpu { pts_sec, .. } => *pts_sec,
        }
    }

    pub fn is_concealed(&self) -> bool {
        match self {
            DecodedFrame::Cpu { is_concealed, .. } => *is_concealed,
            DecodedFrame::Gpu { is_concealed, .. } => *is_concealed,
        }
    }

    pub fn held_frame_idx(&self) -> Option<usize> {
        match self {
            DecodedFrame::Cpu { held_frame_idx, .. } => *held_frame_idx,
            DecodedFrame::Gpu { held_frame_idx, .. } => *held_frame_idx,
        }
    }
}

#[derive(Debug)]
pub enum MediaCommand {
    Play,
    Pause,
    Seek {
        frame_index: usize,
        keyframe_only: bool,
        sequence_id: usize,
        audio_enabled: bool,
    },
    PlayUnpaced {
        sequence_id: usize,
    },
    Shutdown,
}
#[derive(Clone, Debug)]
pub struct VideoMetadata {
    pub total_frames: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
    pub keyframes: Vec<usize>,
    pub keyframes_pts: Vec<i64>,
}

pub fn get_nearest_keyframe(keyframes: &[usize], target: usize) -> usize {
    if keyframes.is_empty() {
        return target;
    }
    match keyframes.binary_search(&target) {
        Ok(i) => keyframes[i],
        Err(0) => keyframes[0],
        Err(i) => keyframes[i - 1],
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExportStatus {
    Idle,
    Exporting { progress: f32, message: String, is_encoding: bool, current_frame: usize },
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct StreamTypeMask(pub u8);

impl StreamTypeMask {
    pub const AUDIO: Self = Self(0b01);
    pub const VIDEO: Self = Self(0b10);
    pub const BOTH: Self = Self(0b11);

    pub fn contains(&self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectErrorType {
    TimestampDiscontinuity,
    CorruptBitstreamPayload,
    DecoderFault,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContiguousDefectRange {
    pub start_pts_sec: f64,
    pub end_pts_sec: f64,
    pub affected_streams: StreamTypeMask,
    pub error_type: DefectErrorType,
    pub packet_count: usize,
    pub affected_gop_indices: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct DefectMap {
    ranges: Vec<ContiguousDefectRange>,
    total_defective_packets: usize,
    total_audio_dropout_sec: f64,
    total_video_defect_sec: f64,
}

impl DefectMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_or_coalesce(
        &mut self,
        pts_sec: f64,
        duration_sec: f64,
        stream: StreamTypeMask,
        error: DefectErrorType,
        gop_idx: Option<usize>,
    ) {
        self.total_defective_packets += 1;
        if stream.contains(StreamTypeMask::AUDIO) {
            self.total_audio_dropout_sec += duration_sec;
        }
        if stream.contains(StreamTypeMask::VIDEO) {
            self.total_video_defect_sec += duration_sec;
        }

        let end_pts_sec = pts_sec + duration_sec.max(0.001);

        // Coalesce with adjacent or overlapping range if within 500ms
        for range in self.ranges.iter_mut() {
            if pts_sec <= range.end_pts_sec + 0.500 && end_pts_sec + 0.500 >= range.start_pts_sec {
                range.start_pts_sec = range.start_pts_sec.min(pts_sec);
                range.end_pts_sec = range.end_pts_sec.max(end_pts_sec);
                range.affected_streams.insert(stream);
                range.packet_count += 1;
                if let Some(g_idx) = gop_idx
                    && !range.affected_gop_indices.contains(&g_idx)
                {
                    range.affected_gop_indices.push(g_idx);
                }
                return;
            }
        }

        self.ranges.push(ContiguousDefectRange {
            start_pts_sec: pts_sec,
            end_pts_sec,
            affected_streams: stream,
            error_type: error,
            packet_count: 1,
            affected_gop_indices: gop_idx.into_iter().collect(),
        });
        self.ranges.sort_by(|a, b| a.start_pts_sec.partial_cmp(&b.start_pts_sec).unwrap_or(std::cmp::Ordering::Equal));
    }

    pub fn intersects_video_gop(&self, gop_start_sec: f64, gop_end_sec: f64) -> bool {
        self.ranges.iter().any(|r| {
            r.affected_streams.contains(StreamTypeMask::VIDEO)
                && r.start_pts_sec < gop_end_sec
                && r.end_pts_sec > gop_start_sec
        })
    }

    pub fn ranges(&self) -> &[ContiguousDefectRange] {
        &self.ranges
    }

    pub fn summary(&self) -> (usize, f64, usize) {
        (
            self.ranges.len(),
            self.total_audio_dropout_sec.max(self.total_video_defect_sec),
            self.total_defective_packets,
        )
    }

    pub fn total_defective_packets(&self) -> usize {
        self.total_defective_packets
    }

    pub fn total_audio_dropout_sec(&self) -> f64 {
        self.total_audio_dropout_sec
    }

    pub fn total_video_defect_sec(&self) -> f64 {
        self.total_video_defect_sec
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_type_mask() {
        let mut mask = StreamTypeMask::AUDIO;
        assert!(mask.contains(StreamTypeMask::AUDIO));
        assert!(!mask.contains(StreamTypeMask::VIDEO));
        assert!(!mask.contains(StreamTypeMask::BOTH));

        mask.insert(StreamTypeMask::VIDEO);
        assert!(mask.contains(StreamTypeMask::AUDIO));
        assert!(mask.contains(StreamTypeMask::VIDEO));
        assert!(mask.contains(StreamTypeMask::BOTH));
    }

    #[test]
    fn test_defect_map_coalescing_and_intersects() {
        let mut map = DefectMap::new();
        // Insert first defect at 1.0s, duration 0.2s (ends at 1.2s)
        map.insert_or_coalesce(
            1.0,
            0.2,
            StreamTypeMask::VIDEO,
            DefectErrorType::CorruptBitstreamPayload,
            Some(0),
        );
        assert_eq!(map.ranges().len(), 1);
        assert_eq!(map.total_defective_packets(), 1);
        assert_eq!(map.ranges()[0].packet_count, 1);
        assert_eq!(map.ranges()[0].affected_gop_indices, vec![0]);

        // Insert adjacent defect at 1.5s (within 500ms of 1.2s), duration 0.1s
        map.insert_or_coalesce(
            1.5,
            0.1,
            StreamTypeMask::AUDIO,
            DefectErrorType::TimestampDiscontinuity,
            Some(1),
        );
        // Should coalesce
        assert_eq!(map.ranges().len(), 1);
        assert_eq!(map.total_defective_packets(), 2);
        assert_eq!(map.ranges()[0].packet_count, 2);
        assert!(map.ranges()[0].affected_streams.contains(StreamTypeMask::BOTH));
        assert_eq!(map.ranges()[0].affected_gop_indices, vec![0, 1]);
        assert_eq!(map.ranges()[0].start_pts_sec, 1.0);
        assert_eq!(map.ranges()[0].end_pts_sec, 1.6);

        // Intersects video GOP: GOP from 0.5s to 1.5s should intersect
        assert!(map.intersects_video_gop(0.5, 1.5));
        // GOP after defect range: 2.0s to 3.0s should not intersect
        assert!(!map.intersects_video_gop(2.0, 3.0));

        // Insert separate defect at 5.0s (outside 500ms window)
        map.insert_or_coalesce(
            5.0,
            0.5,
            StreamTypeMask::AUDIO,
            DefectErrorType::DecoderFault,
            None,
        );
        assert_eq!(map.ranges().len(), 2);

        let (range_count, max_sec, total_pkts) = map.summary();
        assert_eq!(range_count, 2);
        assert_eq!(total_pkts, 3);
        assert!((max_sec - 0.6).abs() < 1e-6); // audio: 0.1 + 0.5 = 0.6, video: 0.2
    }
}
