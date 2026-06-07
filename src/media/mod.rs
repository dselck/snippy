use crossbeam_channel::{Receiver, Sender};
use std::path::Path;
use std::sync::Arc;
use std::thread;

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

// SAFETY: GpuFrameDescriptor transfers raw GPU resource handles across threads.
// The pointers are only used as handles to bind to graphics contexts.
unsafe impl Send for GpuFrameDescriptor {}
unsafe impl Sync for GpuFrameDescriptor {}

#[derive(Clone, Debug)]
pub enum DecodedFrame {
    Cpu {
        frame_idx: usize,
        planes: YuvPlanes,
    },
    Gpu {
        frame_idx: usize,
        descriptor: GpuFrameDescriptor,
    },
}

impl DecodedFrame {
    pub fn frame_idx(&self) -> usize {
        match self {
            DecodedFrame::Cpu { frame_idx, .. } => *frame_idx,
            DecodedFrame::Gpu { frame_idx, .. } => *frame_idx,
        }
    }
}

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

pub enum MediaCommand {
    Play,
    Pause,
    Seek { frame_index: usize },
    Shutdown,
}

pub struct VideoMetadata {
    pub total_frames: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

pub struct MediaEngine {
    pub tx: Sender<MediaCommand>,
    _thread_handle: thread::JoinHandle<()>,
}

impl MediaEngine {
    pub fn new<P: AsRef<Path>>(
        path: P,
        frame_tx: Sender<YuvPlanes>,
        cmd_rx: Receiver<MediaCommand>,
    ) -> Result<(Self, VideoMetadata), anyhow::Error> {
        // Initialize FFmpeg
        ffmpeg_next::init()?;

        let path_str = path.as_ref().to_string_lossy().to_string();

        // 1. Read metadata synchronously first to confirm the file is valid
        let input = ffmpeg_next::format::input(&path_str)?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;

        let codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = codec_ctx.decoder().video()?;

        let metadata = VideoMetadata {
            total_frames: stream.frames() as usize,
            width: decoder.width(),
            height: decoder.height(),
            fps: stream.avg_frame_rate().numerator() as f64
                / stream.avg_frame_rate().denominator() as f64,
        };

        // 2. Spawn the background Media Engine decoding loop
        let (cmd_tx, background_rx) = crossbeam_channel::unbounded();
        let thread_handle = thread::spawn(move || {
            let _ = run_decoding_loop(&path_str, frame_tx, background_rx, cmd_rx);
        });

        Ok((
            Self {
                tx: cmd_tx,
                _thread_handle: thread_handle,
            },
            metadata,
        ))
    }
}

#[allow(dead_code)]
struct GopCache {
    keyframe_idx: usize,
    frames: Vec<(usize, Arc<YuvPlanes>)>,
}

fn run_decoding_loop(
    _path: &str,
    _frame_tx: Sender<YuvPlanes>,
    _background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
) -> Result<(), anyhow::Error> {
    // This is where the background FFmpeg demuxing & decoding loop runs,
    // populating the GOP cache and pushing decoded frames to the frame channel.
    Ok(())
}
