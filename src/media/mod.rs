use crossbeam_channel::{Receiver, Sender};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use ringbuf::wrap::CachingProd;
use ringbuf::storage::Heap;
use ringbuf::SharedRb;
use ringbuf::traits::*;
use ffmpeg_next::software::resampling::context::Context as ResamplerContext;
use ffmpeg_next::format::Sample;
use ffmpeg_next::ChannelLayout;

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
        target_sample_rate: u32,
        target_channels: usize,
        frame_tx: Sender<DecodedFrame>,
        audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
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
            let _ = run_decoding_loop(
                &path_str,
                target_sample_rate,
                target_channels,
                frame_tx,
                audio_tx,
                background_rx,
                cmd_rx,
            );
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
    frames: Vec<(usize, YuvPlanes)>,
}

impl GopCache {
    fn find(&self, target_frame: usize) -> Option<YuvPlanes> {
        self.frames.iter()
            .find(|&&(idx, _)| idx == target_frame)
            .map(|(_, planes)| planes.clone())
    }
}

fn run_decoding_loop(
    path: &str,
    target_sample_rate: u32,
    target_channels: usize,
    frame_tx: Sender<DecodedFrame>,
    mut audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
    background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
) -> Result<(), anyhow::Error> {
    let mut input = ffmpeg_next::format::input(&path)?;

    let video_stream = input
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("No video stream"))?;
    let video_stream_idx = video_stream.index();

    let audio_stream_opt = input.streams().best(ffmpeg_next::media::Type::Audio);
    let audio_stream_idx_opt = audio_stream_opt.as_ref().map(|s| s.index());

    // Channels for routing raw packets to decoder threads
    let (video_packet_tx, video_packet_rx) = crossbeam_channel::bounded::<ffmpeg_next::Packet>(32);
    let (audio_packet_tx, audio_packet_rx) = crossbeam_channel::bounded::<ffmpeg_next::Packet>(64);
    let (video_seek_tx, video_seek_rx) = crossbeam_channel::bounded::<(usize, Sender<bool>)>(1);
    let (audio_seek_tx, audio_seek_rx) = crossbeam_channel::bounded::<()>(1);

    // Spawn Video Decoder Thread
    let video_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
    let mut video_decoder = video_codec_ctx.decoder().video()?;
    let frame_tx_clone = frame_tx.clone();
    
    let video_thread = thread::spawn(move || {
        let mut gop_cache = GopCache {
            keyframe_idx: usize::MAX,
            frames: Vec::new(),
        };

        loop {
            crossbeam_channel::select! {
                recv(video_packet_rx) -> packet_res => {
                    if let Ok(packet) = packet_res {
                        if video_decoder.send_packet(&packet).is_ok() {
                            let mut frame = ffmpeg_next::util::frame::Video::empty();
                            while video_decoder.receive_frame(&mut frame).is_ok() {
                                let w = frame.width();
                                let h = frame.height();
                                
                                let planes = YuvPlanes {
                                    y: frame.data(0).to_vec(),
                                    u: frame.data(1).to_vec(),
                                    v: frame.data(2).to_vec(),
                                    width: w,
                                    height: h,
                                    y_stride: frame.stride(0) as u32,
                                    u_stride: frame.stride(1) as u32,
                                    v_stride: frame.stride(2) as u32,
                                };

                                let pts = frame.pts().unwrap_or(0) as usize;
                                let decoded = DecodedFrame::Cpu {
                                    frame_idx: pts,
                                    planes: planes.clone(),
                                };
                                
                                gop_cache.frames.push((pts, planes));
                                if gop_cache.frames.len() > 128 {
                                    gop_cache.frames.remove(0);
                                }
                                
                                if frame_tx_clone.send(decoded).is_err() {
                                    return;
                                }
                            }
                        }
                    } else {
                        break;
                    }
                }
                recv(video_seek_rx) -> seek_res => {
                    if let Ok((frame_index, resp_tx)) = seek_res {
                        if let Some(planes) = gop_cache.find(frame_index) {
                            let decoded = DecodedFrame::Cpu {
                                frame_idx: frame_index,
                                planes,
                            };
                            let _ = frame_tx_clone.send(decoded);
                            let _ = resp_tx.send(true);
                        } else {
                            while video_packet_rx.try_recv().is_ok() {}
                            video_decoder.flush();
                            let _ = resp_tx.send(false);
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    });

    // Spawn Audio Decoder Thread
    let audio_thread_opt = if let Some(ref stream) = audio_stream_opt {
        let audio_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let mut audio_decoder = audio_codec_ctx.decoder().audio()?;
        
        let audio_thread = thread::spawn(move || {
            let mut resampler: Option<ResamplerContext> = None;

            loop {
                crossbeam_channel::select! {
                    recv(audio_packet_rx) -> packet_res => {
                        if let Ok(packet) = packet_res {
                            if audio_decoder.send_packet(&packet).is_ok() {
                                let mut frame = ffmpeg_next::util::frame::Audio::empty();
                                while audio_decoder.receive_frame(&mut frame).is_ok() {
                                    let r = match resampler {
                                        Some(ref mut r) if r.input().format == frame.format()
                                            && r.input().channel_layout == frame.channel_layout()
                                            && r.input().rate == frame.rate() => r,
                                        _ => {
                                            let target_layout = if target_channels == 1 {
                                                ChannelLayout::MONO
                                            } else {
                                                ChannelLayout::STEREO
                                            };
                                            
                                            let new_r = ResamplerContext::get(
                                                frame.format(),
                                                frame.channel_layout(),
                                                frame.rate(),
                                                Sample::F32(ffmpeg_next::format::sample::Type::Packed),
                                                target_layout,
                                                target_sample_rate,
                                            ).unwrap();
                                            resampler = Some(new_r);
                                            resampler.as_mut().unwrap()
                                        }
                                    };

                                    let mut resampled = ffmpeg_next::util::frame::Audio::empty();
                                    if r.run(&frame, &mut resampled).is_ok() {
                                        let sample_count = resampled.samples();
                                        if sample_count > 0 {
                                            let data: &[f32] = bytemuck::cast_slice(&resampled.data(0)[..sample_count * 4]);
                                            for &sample in data {
                                                // Wait/block if CPAL buffer is full (providing backpressure)
                                                while audio_tx.try_push(sample).is_err() {
                                                    thread::sleep(std::time::Duration::from_millis(5));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    recv(audio_seek_rx) -> seek_res => {
                        if seek_res.is_ok() {
                            while audio_packet_rx.try_recv().is_ok() {}
                            audio_decoder.flush();
                        } else {
                            break;
                        }
                    }
                }
            }
        });
        Some(audio_thread)
    } else {
        None
    };

    // Main demuxer loop using manually advanced packets iterator
    let mut packets_iter = input.packets();
    loop {
        // Handle incoming command overrides
        let mut seek_requested = None;
        while let Ok(cmd) = background_rx.try_recv() {
            match cmd {
                MediaCommand::Shutdown => return Ok(()),
                MediaCommand::Seek { frame_index } => {
                    seek_requested = Some(frame_index);
                }
                _ => {}
            }
        }

        if let Some(frame_index) = seek_requested {
            let (resp_tx, resp_rx) = crossbeam_channel::bounded::<bool>(1);
            let _ = video_seek_tx.send((frame_index, resp_tx));
            if let Ok(found) = resp_rx.recv() {
                if !found {
                    // Drop packets_iter to release borrow on input
                    drop(packets_iter);

                    // Flush queues and seeking input context
                    let seek_pts = (frame_index as f64 * 1_000_000.0 / 30.0) as i64; // mock fps
                    let _ = input.seek(seek_pts, ..seek_pts + 1);

                    // Notify audio thread to flush
                    let _ = audio_seek_tx.send(());

                    // Recreate packets_iter
                    packets_iter = input.packets();
                }
            }
            continue;
        }

        if let Some(packet_res) = packets_iter.next() {
            let (stream, packet) = packet_res;
            if stream.index() == video_stream_idx {
                if video_packet_tx.send(packet).is_err() {
                    break;
                }
            } else if Some(stream.index()) == audio_stream_idx_opt {
                if audio_packet_tx.send(packet).is_err() {
                    break;
                }
            }
        } else {
            break;
        }
    }

    // Explicitly drop transmitters to notify receiver threads
    drop(video_packet_tx);
    drop(video_seek_tx);
    drop(audio_packet_tx);
    drop(audio_seek_tx);

    let _ = video_thread.join();
    if let Some(ath) = audio_thread_opt {
        let _ = ath.join();
    }

    Ok(())
}
