use crossbeam_channel::{Receiver, Sender};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use ringbuf::wrap::CachingProd;
use ringbuf::storage::Heap;
use ringbuf::SharedRb;
use ringbuf::traits::*;

pub mod types;
pub mod cache;
pub mod presenter_state;
pub mod decoder_state;



pub mod decoder_pipeline;
pub mod ffmpeg_adapter;
pub mod audio_pipeline;
pub mod sync;

pub use crate::media::types::*;
pub use crate::media::cache::*;pub mod session;
pub use crate::media::session::*;

pub use crate::media::decoder_pipeline::*;
pub use crate::media::ffmpeg_adapter::*;
pub use crate::media::audio_pipeline::*;
pub use crate::media::sync::*;

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
        audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
        tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
        cmd_rx: Receiver<MediaCommand>,
        master_clock: crate::audio::clock::MasterClock,
    ) -> Result<(Self, VideoMetadata), anyhow::Error> {
        ffmpeg_next::init()?;

        let path_str = path.as_ref().to_string_lossy().to_string();

        let input = ffmpeg_next::format::input(&path_str)?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;

        let codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = codec_ctx.decoder().video()?;

        let has_audio = input
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .is_some();

        let fps = stream.avg_frame_rate().numerator() as f64
            / stream.avg_frame_rate().denominator() as f64;

        let mut total_frames = stream.frames() as usize;
        if total_frames == 0 {
            let duration_secs = input.duration() as f64 / 1_000_000.0;
            total_frames = (duration_secs * fps).round() as usize;
        }

        let (keyframes, keyframes_pts) = {
            let mut scan = ffmpeg_next::format::input(&path_str)?;
            let video_stream_idx = stream.index();
            let mut kfs = Vec::new();
            let mut kfs_pts = Vec::new();
            let mut frame_idx: usize = 0;
            for (stream, packet) in scan.packets() {
                if stream.index() == video_stream_idx {
                    if packet.is_key() {
                        kfs.push(frame_idx);
                        kfs_pts.push(packet.pts().unwrap_or(0));
                    }
                    frame_idx += 1;
                }
            }
            if kfs.is_empty() || kfs[0] != 0 {
                kfs.insert(0, 0);
                kfs_pts.insert(0, 0);
            }
            (kfs, kfs_pts)
        };

        let metadata = VideoMetadata {
            total_frames,
            width: decoder.width(),
            height: decoder.height(),
            fps,
            has_audio,
            keyframes: keyframes.clone(),
            keyframes_pts: keyframes_pts.clone(),
        };

        let (cmd_tx, background_rx) = crossbeam_channel::unbounded();
        let fps_clone = metadata.fps;
        let master_clock_clone = master_clock.clone();
        let keyframes_pts_clone = metadata.keyframes_pts.clone();
        
        let thread_handle = thread::spawn(move || {
            let _ = run_decoding_loop(
                &path_str,
                target_sample_rate,
                target_channels,
                frame_tx,
                audio_tx,
                tags_tx,
                background_rx,
                cmd_rx,
                fps_clone,
                master_clock_clone,
                keyframes,
                keyframes_pts_clone,
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


fn run_decoding_loop(
    path: &str,
    target_sample_rate: u32,
    target_channels: usize,
    frame_tx: Sender<DecodedFrame>,
    audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
    background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
    fps: f64,
    master_clock: crate::audio::clock::MasterClock,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
) -> Result<(), anyhow::Error> {
    let (play_queue_tx, play_queue_rx) = crossbeam_channel::bounded::<DecodedFrame>(20);
    let (decoder_cmd_tx, decoder_cmd_rx) = crossbeam_channel::unbounded::<MediaCommand>();

    // Spawn Decoder Thread
    let path_str = path.to_string();
    let master_clock_clone = master_clock.clone();
    let keyframes_clone = keyframes.clone();
    let keyframes_pts_clone = keyframes_pts.clone();
    
    thread::spawn(move || {
        let _ = run_decoder_thread(
            path_str,
            target_sample_rate,
            target_channels,
            play_queue_tx,
            audio_tx,
            tags_tx,
            decoder_cmd_rx,
            fps,
            master_clock_clone,
            keyframes_clone,
            keyframes_pts_clone,
        );
    });

    let sync_engine = crate::media::sync::VideoSyncEngine::new(Box::new(master_clock.clone()));
    let mut sm = crate::media::presenter_state::PresenterStateMachine::new(sync_engine);

    // Track the active wakeup duration
    let mut active_wakeup: Option<std::time::Duration> = None;

    loop {
        // Step 1: Wait for events from background command channel or play queue (with computed timeout if active_wakeup is some)
        let event = if let Some(timeout) = active_wakeup {
            // Wait with a small millisecond floor to avoid immediate spinning
            let wait_timeout = timeout.min(std::time::Duration::from_millis(10));
            if wait_timeout > std::time::Duration::from_millis(1) {
                crossbeam_channel::select! {
                    recv(background_rx) -> cmd => {
                        if let Ok(c) = cmd {
                            Some(crate::media::presenter_state::PresenterEvent::Command(c))
                        } else {
                            None
                        }
                    }
                    recv(play_queue_rx) -> frame => {
                        if let Ok(f) = frame {
                            Some(crate::media::presenter_state::PresenterEvent::FrameDecoded(f))
                        } else {
                            None
                        }
                    }
                    default(wait_timeout) => {
                        Some(crate::media::presenter_state::PresenterEvent::Wakeup)
                    }
                }
            } else {
                Some(crate::media::presenter_state::PresenterEvent::Wakeup)
            }
        } else {
            // Blocking wait or non-spinning select
            match sm.state() {
                crate::media::presenter_state::PresenterState::Paused => {
                    // Paused: only care about commands
                    if let Ok(cmd) = background_rx.recv() {
                        Some(crate::media::presenter_state::PresenterEvent::Command(cmd))
                    } else {
                        None
                    }
                }
                crate::media::presenter_state::PresenterState::Seeking { .. } => {
                    // Seeking: wait on command OR play queue
                    crossbeam_channel::select! {
                        recv(background_rx) -> cmd => {
                            if let Ok(c) = cmd {
                                Some(crate::media::presenter_state::PresenterEvent::Command(c))
                            } else {
                                None
                            }
                        }
                        recv(play_queue_rx) -> frame => {
                            if let Ok(f) = frame {
                                Some(crate::media::presenter_state::PresenterEvent::FrameDecoded(f))
                            } else {
                                None
                            }
                        }
                    }
                }
                _ => {
                    // Playing (no wakeup scheduled): check channels
                    crossbeam_channel::select! {
                        recv(background_rx) -> cmd => {
                            if let Ok(c) = cmd {
                                Some(crate::media::presenter_state::PresenterEvent::Command(c))
                            } else {
                                None
                            }
                        }
                        recv(play_queue_rx) -> frame => {
                            if let Ok(f) = frame {
                                Some(crate::media::presenter_state::PresenterEvent::FrameDecoded(f))
                            } else {
                                None
                            }
                        }
                        // Add a small default timeout if we are playing to keep checking/pacing
                        default(std::time::Duration::from_millis(2)) => {
                            Some(crate::media::presenter_state::PresenterEvent::Wakeup)
                        }
                    }
                }
            }
        };

        // If channel is disconnected/shutdown event resulted in None, we can exit or process it
        let event = match event {
            Some(ev) => ev,
            None => break,
        };

        // Reset the active wakeup before stepping the state machine
        active_wakeup = None;

        // Step the state machine
        let actions = sm.step(event);

        // Process output actions
        for action in actions {
            match action {
                crate::media::presenter_state::PresenterAction::ClearLookaheadQueue => {
                    while play_queue_rx.try_recv().is_ok() {}
                }
                crate::media::presenter_state::PresenterAction::SendDecoderCommand(cmd) => {
                    let is_shutdown = matches!(cmd, MediaCommand::Shutdown);
                    let _ = decoder_cmd_tx.send(cmd);
                    if is_shutdown {
                        return Ok(());
                    }
                }
                crate::media::presenter_state::PresenterAction::DispatchFrame(frame) => {
                    let _ = frame_tx.try_send(frame);
                }
                crate::media::presenter_state::PresenterAction::ScheduleWakeup(duration) => {
                    active_wakeup = Some(duration);
                }
            }
        }
    }

    Ok(())
}

struct ChannelFrameSink {
    tx: crossbeam_channel::Sender<DecodedFrame>,
}
impl crate::media::decoder_state::FrameSink for ChannelFrameSink {
    fn try_push(&mut self, frame: DecodedFrame) -> Result<(), DecodedFrame> {
        self.tx.try_send(frame).map_err(|e| match e {
            crossbeam_channel::TrySendError::Full(f) => f,
            crossbeam_channel::TrySendError::Disconnected(f) => f,
        })
    }
    fn len(&self) -> usize {
        self.tx.len()
    }
}

struct RingbufAudioSink {
    audio_tx: ringbuf::wrap::CachingProd<std::sync::Arc<ringbuf::SharedRb<ringbuf::storage::Heap<(f32, u64)>>>>,
    tags_tx: ringbuf::wrap::CachingProd<std::sync::Arc<ringbuf::SharedRb<ringbuf::storage::Heap<(u64, u64, u64)>>>>,
}
impl crate::media::decoder_state::AudioSink for RingbufAudioSink {
    fn try_push_slice(&mut self, data: &[f32], generation: u64) -> usize {
        let mut pushed = 0;
        for &sample in data {
            if self.audio_tx.try_push((sample, generation)).is_ok() {
                pushed += 1;
            } else {
                break;
            }
        }
        pushed
    }
    fn vacant_len(&self) -> usize {
        self.audio_tx.vacant_len()
    }
    fn push_tag(&mut self, generation: u64, idx: u64, pts_us: u64) {
        let _ = self.tags_tx.try_push((generation, idx, pts_us));
    }
}

fn run_decoder_thread(
    path: String,
    target_sample_rate: u32,
    target_channels: usize,
    play_queue_tx: Sender<DecodedFrame>,
    audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
    decoder_cmd_rx: Receiver<MediaCommand>,
    fps: f64,
    master_clock: crate::audio::clock::MasterClock,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
) -> Result<(), anyhow::Error> {
    let mut input = ffmpeg_next::format::input(&path)?;

    let (video_stream_idx, video_time_base_f64, video_start_time, video_decoder) = {
        let video_stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream"))?;
        let idx = video_stream.index();
        let time_base = video_stream.time_base();
        let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;
        let start_time = if video_stream.start_time() > -1_000_000_000_000 {
            video_stream.start_time() as f64 * time_base_f64
        } else {
            0.0
        };
        let mut codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;

        // Determine hardware acceleration pathways and attempt to create a HW device context
        unsafe {
            let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
            
            #[cfg(target_os = "macos")]
            let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX;
            #[cfg(target_os = "windows")]
            let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA;
            #[cfg(target_os = "linux")]
            let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI;

            let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
                &mut hw_device_ctx,
                hw_type,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            );

            if res == 0 && !hw_device_ctx.is_null() {
                println!("Successfully initialized hardware acceleration context: {:?}", hw_type);
                (*codec_ctx.as_mut_ptr()).hw_device_ctx = ffmpeg_next::sys::av_buffer_ref(hw_device_ctx);
                ffmpeg_next::sys::av_buffer_unref(&mut hw_device_ctx);
            } else {
                println!("Failed to initialize hardware acceleration context, falling back to CPU decoding");
            }
        }

        let decoder = codec_ctx.decoder().video()?;
        (idx, time_base_f64, start_time, decoder)
    };

    let (audio_stream_idx_opt, has_audio_stream, audio_pipeline_opt) = {
        if let Some(stream) = input.streams().best(ffmpeg_next::media::Type::Audio) {
            let idx = stream.index();
            let audio_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
            let audio_decoder = audio_codec_ctx.decoder().audio()?;
            let audio_time_base = stream.time_base();
            let audio_tb_f64 = audio_time_base.numerator() as f64 / audio_time_base.denominator() as f64;
            let pipeline = crate::media::audio_pipeline::AudioDecoderPipeline::new(
                audio_decoder,
                target_sample_rate,
                target_channels,
                audio_tb_f64,
                video_start_time,
            );
            (Some(idx), true, Some(pipeline))
        } else {
            (None, false, None)
        }
    };

    let video_pipeline = {
        let adapter = Box::new(crate::media::ffmpeg_adapter::FfmpegDecoderAdapter::new(
            video_decoder,
            video_time_base_f64,
            video_start_time,
            fps,
        ));
        crate::media::decoder_pipeline::VideoDecoderPipeline::new(adapter, keyframes.clone())
    };

    let video_sink = Box::new(ChannelFrameSink {
        tx: play_queue_tx.clone(),
    });

    let audio_sink: Option<Box<dyn crate::media::decoder_state::AudioSink>> = if has_audio_stream {
        Some(Box::new(RingbufAudioSink {
            audio_tx,
            tags_tx,
        }))
    } else {
        None
    };

    let mut sm = crate::media::decoder_state::DecoderStateMachine::new(
        video_pipeline,
        audio_pipeline_opt,
        master_clock,
        video_sink,
        audio_sink,
        keyframes,
        keyframes_pts,
        fps,
        video_stream_idx,
        audio_stream_idx_opt,
        has_audio_stream,
    );

    let mut packets_iter = input.packets();
    let mut pending_cmd: Option<MediaCommand> = None;

    loop {
        // Query incoming commands
        let cmd_opt = pending_cmd.take().or_else(|| {
            if !sm.is_decoding_active() {
                decoder_cmd_rx.recv().ok()
            } else {
                decoder_cmd_rx.try_recv().ok()
            }
        });

        let mut events = Vec::new();
        if let Some(cmd) = cmd_opt {
            if matches!(cmd, MediaCommand::Shutdown) {
                return Ok(());
            }
            events.push(crate::media::decoder_state::DecoderEvent::Command(cmd));
        } else if sm.is_decoding_active() {
            events.push(crate::media::decoder_state::DecoderEvent::Wakeup);
        }

        let mut did_work = false;
        for event in events {
            let actions = sm.step(event);
            for action in actions {
                match action {
                    crate::media::decoder_state::DecoderAction::DemuxNextPacket => {
                        did_work = true;
                        if let Some(packet_res) = packets_iter.next() {
                            let (stream, packet) = packet_res;
                            let sub_actions = sm.step(crate::media::decoder_state::DecoderEvent::PacketDemuxed {
                                packet,
                                stream_index: stream.index(),
                            });
                            for sub in sub_actions {
                                if let crate::media::decoder_state::DecoderAction::SeekDemuxer { pts } = sub {
                                    drop(packets_iter);
                                    unsafe {
                                        let _ = ffmpeg_next::ffi::avformat_seek_file(
                                            input.as_mut_ptr(),
                                            video_stream_idx as i32,
                                            i64::MIN,
                                            pts,
                                            pts,
                                            0,
                                        );
                                    }
                                    packets_iter = input.packets();
                                }
                            }
                        } else {
                            std::thread::sleep(std::time::Duration::from_millis(10));
                        }
                    }
                    crate::media::decoder_state::DecoderAction::SeekDemuxer { pts } => {
                        did_work = true;
                        drop(packets_iter);
                        unsafe {
                            let _ = ffmpeg_next::ffi::avformat_seek_file(
                                input.as_mut_ptr(),
                                video_stream_idx as i32,
                                i64::MIN,
                                pts,
                                pts,
                                0,
                            );
                        }
                        packets_iter = input.packets();
                    }
                    crate::media::decoder_state::DecoderAction::ScheduleWakeup(duration) => {
                        did_work = true;
                        std::thread::sleep(duration);
                    }
                }
            }
        }

        // Active, but maybe queues are full, so let's sleep a tiny bit to be friendly
        if sm.is_decoding_active() && pending_cmd.is_none() && !did_work {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}
