
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
    },
    Gpu {
        frame_idx: usize,
        descriptor: GpuFrameDescriptor,
        sequence_id: usize,
        pts_sec: f64,
        color_space: ffmpeg_next::color::Space,
        color_range: ffmpeg_next::color::Range,
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

