use crossbeam_channel::Sender;

#[derive(Clone, Debug)]
pub enum GpuFrameDescriptor {
    CVPixelBuffer(*const std::ffi::c_void),
    D3D11Texture(*const std::ffi::c_void),
    Dmabuf {
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        format: u32,
    },
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
    },
    Gpu {
        frame_idx: usize,
        descriptor: GpuFrameDescriptor,
        sequence_id: usize,
        pts_sec: f64,
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
    Shutdown,
}

pub enum VideoThreadCommand {
    Seek {
        frame_index: usize,
        keyframe_only: bool,
        sequence_id: usize,
        audio_enabled: bool,
        resp_tx: Sender<bool>,
    },
    Play,
}

pub enum AudioThreadCommand {
    Seek { target_time: f64, resp_tx: Sender<()> },
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
