#![allow(clippy::type_complexity, clippy::too_many_arguments)]
use crossbeam_channel::{Receiver, Sender};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use ringbuf::wrap::CachingProd;
use ringbuf::storage::Heap;
use ringbuf::SharedRb;


pub mod types;
pub mod cache;
pub mod sinks;
pub use crate::media::sinks::*;
pub mod mac_hw_surface;

pub mod decoder_pipeline;
pub mod ffmpeg_adapter;
pub mod audio_pipeline;
pub mod sync;

pub use crate::media::types::*;
pub use crate::media::cache::*;
pub mod session;
pub use crate::media::session::*;
pub mod thumbnails;
pub use thumbnails::*;
pub mod export;
pub use export::export_video;


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
        loudness_tx: crossbeam_channel::Sender<(usize, f32)>,  // ← new
        hw_accel: bool,
        lookahead_depth: usize,
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
                hw_accel,
                lookahead_depth,
                loudness_tx,  // ← new, last
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PresenterState {
    Paused,
    Playing,
    PlayingUnpaced { sequence_id: usize },
    Seeking {
        target_frame_idx: Option<usize>,
        sequence_id: usize,
        resume_playing_after: bool,
    },
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
    hw_accel: bool,
    lookahead_depth: usize,
    loudness_tx: crossbeam_channel::Sender<(usize, f32)>,  // ← new, last
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
            hw_accel,
            loudness_tx,  // ← new, last
        );
    });

    let mut sync_engine = crate::media::sync::VideoSyncEngine::new(master_clock.clone());
    let mut state = PresenterState::Paused;
    let mut current_frame: Option<DecodedFrame> = None;
    let mut lookahead_queue = std::collections::VecDeque::new();
    let mut last_wait_frame: Option<(usize, usize)> = None;

    let mut active_wakeup: Option<std::time::Duration> = None;

    loop {
        // Step 1: Handle input events
        let mut events = Vec::new();

        if let Some(timeout) = active_wakeup {
            let wait_timeout = timeout.min(std::time::Duration::from_millis(10));
            if wait_timeout > std::time::Duration::from_millis(1) {
                if lookahead_queue.len() < lookahead_depth {
                    crossbeam_channel::select! {
                        recv(background_rx) -> cmd => {
                            if let Ok(c) = cmd { events.push(c); } else { return Ok(()); }
                        }
                        recv(play_queue_rx) -> frame => {
                            if let Ok(f) = frame { events.push(MediaCommand::PlayUnpaced { sequence_id: 999999 }); /* hack to pass frame */
                                match &mut state {
                                    PresenterState::Playing | PresenterState::PlayingUnpaced { .. } => {
                                        lookahead_queue.push_back(f);
                                    }
                                    PresenterState::Seeking { target_frame_idx, sequence_id, resume_playing_after } => {
                                        if f.sequence_id() == *sequence_id {
                                            if let Some(target_idx) = *target_frame_idx {
                                                let f_idx = f.frame_idx();
                                                let _ = frame_tx.try_send(f.clone());
                                                if f_idx >= target_idx {
                                                    *target_frame_idx = None;
                                                }
                                            }
                                            if target_frame_idx.is_none() {
                                                if *resume_playing_after {
                                                    state = PresenterState::Playing;
                                                    sync_engine.reset();
                                                    let _ = decoder_cmd_tx.send(MediaCommand::Play);
                                                } else {
                                                    state = PresenterState::Paused;
                                                }
                                            }
                                        }
                                    }
                                    PresenterState::Paused => {}
                                }
                            } else { return Ok(()); }
                        }
                        default(wait_timeout) => {}
                    }
                } else {
                    crossbeam_channel::select! {
                        recv(background_rx) -> cmd => {
                            if let Ok(c) = cmd { events.push(c); } else { return Ok(()); }
                        }
                        default(wait_timeout) => {}
                    }
                }
            }
        } else {
            match state {
                PresenterState::Paused => {
                    if let Ok(cmd) = background_rx.recv() {
                        events.push(cmd);
                    } else { return Ok(()); }
                }
                PresenterState::Seeking { .. } => {
                    crossbeam_channel::select! {
                        recv(background_rx) -> cmd => {
                            if let Ok(c) = cmd { events.push(c); } else { return Ok(()); }
                        }
                        recv(play_queue_rx) -> f_res => {
                            if let Ok(f) = f_res {
                                if let PresenterState::Seeking { target_frame_idx, sequence_id, resume_playing_after } = &mut state
                                    && f.sequence_id() == *sequence_id {
                                        if let Some(target_idx) = *target_frame_idx {
                                            let f_idx = f.frame_idx();
                                            let _ = frame_tx.try_send(f.clone());
                                            if f_idx >= target_idx {
                                                *target_frame_idx = None;
                                            }
                                        }
                                        if target_frame_idx.is_none() {
                                            if *resume_playing_after {
                                                state = PresenterState::Playing;
                                                sync_engine.reset();
                                                let _ = decoder_cmd_tx.send(MediaCommand::Play);
                                            } else {
                                                state = PresenterState::Paused;
                                            }
                                        }
                                    }
                            } else { return Ok(()); }
                        }
                    }
                }
                _ => {
                    if lookahead_queue.len() < lookahead_depth {
                        crossbeam_channel::select! {
                            recv(background_rx) -> cmd => {
                                if let Ok(c) = cmd { events.push(c); } else { return Ok(()); }
                            }
                            recv(play_queue_rx) -> f_res => {
                                if let Ok(f) = f_res {
                                    lookahead_queue.push_back(f);
                                } else { return Ok(()); }
                            }
                            default(std::time::Duration::from_millis(2)) => {}
                        }
                    } else {
                        crossbeam_channel::select! {
                            recv(background_rx) -> cmd => {
                                if let Ok(c) = cmd { events.push(c); } else { return Ok(()); }
                            }
                            default(std::time::Duration::from_millis(2)) => {}
                        }
                    }
                }
            }
        }

        active_wakeup = None;

        for cmd in events {
            match cmd {
                MediaCommand::Shutdown => {
                    let _ = decoder_cmd_tx.send(MediaCommand::Shutdown);
                    return Ok(());
                }
                MediaCommand::Play => {
                    state = PresenterState::Playing;
                    lookahead_queue.clear();
                    current_frame = None;
                    sync_engine.reset();
                    while play_queue_rx.try_recv().is_ok() {}
                    let _ = decoder_cmd_tx.send(MediaCommand::Play);
                }
                MediaCommand::PlayUnpaced { sequence_id } => {
                    if sequence_id == 999999 { continue; } // hack skip
                    state = PresenterState::PlayingUnpaced { sequence_id };
                    lookahead_queue.clear();
                    current_frame = None;
                    while play_queue_rx.try_recv().is_ok() {}
                    let _ = decoder_cmd_tx.send(MediaCommand::PlayUnpaced { sequence_id });
                }
                MediaCommand::Pause => {
                    match &mut state {
                        PresenterState::Seeking { resume_playing_after, .. } => {
                            *resume_playing_after = false;
                            let _ = decoder_cmd_tx.send(MediaCommand::Pause);
                        }
                        _ => {
                            state = PresenterState::Paused;
                            lookahead_queue.clear();
                            current_frame = None;
                            while play_queue_rx.try_recv().is_ok() {}
                            let _ = decoder_cmd_tx.send(MediaCommand::Pause);
                        }
                    }
                }
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled } => {
                    let resume_playing_after = match state {
                        PresenterState::Playing | PresenterState::PlayingUnpaced { .. } => true,
                        PresenterState::Seeking { resume_playing_after: r, .. } => r,
                        PresenterState::Paused => false,
                    };
                    state = PresenterState::Seeking {
                        target_frame_idx: Some(frame_index),
                        sequence_id,
                        resume_playing_after,
                    };
                    current_frame = None;
                    lookahead_queue.clear();
                    while play_queue_rx.try_recv().is_ok() {}
                    let _ = decoder_cmd_tx.send(MediaCommand::Seek {
                        frame_index,
                        keyframe_only,
                        sequence_id,
                        audio_enabled,
                    });
                }
            }
        }

        // Evaluate frames presentation loop
        loop {
            if current_frame.is_none()
                && matches!(state, PresenterState::Playing | PresenterState::PlayingUnpaced { .. }) {
                    current_frame = lookahead_queue.pop_front();
                }

            if let Some(frame) = &current_frame {
                match state {
                    PresenterState::Playing => {
                        let pts_sec = frame.pts_sec();
                        match sync_engine.evaluate_frame(pts_sec) {
                            Ok(crate::media::sync::SyncAction::Present) => {
                                last_wait_frame = None;
                                let f = current_frame.take().unwrap();
                                let _ = frame_tx.try_send(f);
                            }
                            Ok(crate::media::sync::SyncAction::Wait(remaining)) => {
                                let f_idx = frame.frame_idx();
                                let count = match last_wait_frame {
                                    Some((idx, c)) if idx == f_idx => c + 1,
                                    _ => 1,
                                };
                                last_wait_frame = Some((f_idx, count));
                                active_wakeup = Some(remaining);
                                break;
                            }
                            Err(_) => {
                                last_wait_frame = None;
                                let f = current_frame.take().unwrap();
                                let _ = frame_tx.try_send(f);
                            }
                        }
                    }
                    PresenterState::PlayingUnpaced { .. } => {
                        let f = current_frame.take().unwrap();
                        let _ = frame_tx.try_send(f);
                    }
                    _ => {
                        break;
                    }
                }
            } else {
                break;
            }
        }
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
    hw_accel: bool,
    loudness_tx: crossbeam_channel::Sender<(usize, f32)>,  // ← new, last
) -> Result<(), anyhow::Error> {
    let mut input = ffmpeg_next::format::input(&path)?;

    let (video_stream_index, video_time_base_f64, video_start_time, video_decoder) = {
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

        if hw_accel {
            unsafe {
                let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
                #[cfg(target_os = "macos")]
                let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX;
                #[cfg(target_os = "windows")]
                let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA;
                #[cfg(target_os = "linux")]
                let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI;

                let res = ffmpeg_next::sys::av_hwdevice_ctx_create(&mut hw_device_ctx, hw_type, std::ptr::null(), std::ptr::null_mut(), 0);
                if res == 0 && !hw_device_ctx.is_null() {
                    (*codec_ctx.as_mut_ptr()).hw_device_ctx = ffmpeg_next::sys::av_buffer_ref(hw_device_ctx);
                    ffmpeg_next::sys::av_buffer_unref(&mut hw_device_ctx);
                }
            }
        }
        (idx, time_base_f64, start_time, codec_ctx.decoder().video()?)
    };

    let (audio_stream_index_opt, has_audio_stream, mut audio_pipeline_opt) = {
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

    let mut video_pipeline = {
        let adapter = Box::new(crate::media::ffmpeg_adapter::FfmpegDecoderAdapter::new(
            video_decoder,
            video_time_base_f64,
            video_start_time,
            fps,
        ));
        crate::media::decoder_pipeline::VideoDecoderPipeline::new(adapter, keyframes.clone())
    };

    let mut video_sink = crate::media::sinks::CrossbeamFrameSink::new(play_queue_tx, 20);
    let mut audio_sink_opt = if has_audio_stream {
        Some(crate::media::sinks::CrossbeamAudioSink::new(audio_tx, tags_tx, master_clock.clone(), 2048))
    } else {
        None
    };
    let mut audio_enabled = true;

    let mut packets_iter = input.packets();
    let mut pending_cmd: Option<MediaCommand> = None;
    let mut seek_demuxer_pts: Option<i64> = None;

    loop {
        video_sink.flush();
        if let Some(sink) = audio_sink_opt.as_mut() {
            sink.flush();
        }

        let is_decoding_active = video_pipeline.is_active();

        let cmd_opt = pending_cmd.take().or_else(|| {
            if !is_decoding_active {
                decoder_cmd_rx.recv().ok()
            } else {
                decoder_cmd_rx.try_recv().ok()
            }
        });

        if let Some(cmd) = cmd_opt {
            match cmd {
                MediaCommand::Shutdown => {
                    return Ok(());
                }
                MediaCommand::Play => {
                    if !audio_enabled {
                        audio_enabled = true;
                        master_clock.set_has_audio(has_audio_stream && audio_enabled);
                        if let Some(audio_pipeline) = audio_pipeline_opt.as_mut() {
                            let current_frame = video_pipeline.fallback_frame_idx;
                            audio_pipeline.handle_seek(current_frame as f64 / fps);
                            master_clock.inc_playback_generation();
                        }
                    }
                    video_pipeline.play();
                }
                MediaCommand::PlayUnpaced { sequence_id } => {
                    audio_enabled = false;
                    master_clock.set_has_audio(false);
                    video_pipeline.play_unpaced(sequence_id);
                }
                MediaCommand::Pause => {
                    video_pipeline.pause();
                }
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
                    audio_enabled = ae;
                    master_clock.set_has_audio(has_audio_stream && audio_enabled);
                    
                    video_sink.clear();
                    if let Some(sink) = audio_sink_opt.as_mut() {
                        sink.clear();
                    }

                    let handled = video_pipeline.seek(frame_index, keyframe_only, sequence_id, audio_enabled);

                    if let Some(audio_pipeline) = audio_pipeline_opt.as_mut()
                        && audio_enabled {
                            audio_pipeline.handle_seek(frame_index as f64 / fps);
                            master_clock.inc_playback_generation();
                        }

                    if handled {
                        if let Some(frame) = video_pipeline.get_cached_frame(frame_index) {
                            video_sink.push(std::iter::once(frame));
                        }
                    } else {
                        let target_kf = get_nearest_keyframe(&keyframes, frame_index);
                        let kf_pts = keyframes.binary_search(&target_kf)
                            .map(|idx| keyframes_pts[idx])
                            .unwrap_or(0);
                        seek_demuxer_pts = Some(kf_pts);
                    }
                }
            }
        }

        if let Some(pts) = seek_demuxer_pts.take() {
            let pts_us = (pts as f64 * video_time_base_f64 * 1_000_000.0).round() as i64;
            let _ = input.seek(pts_us, ..pts_us);
            packets_iter = input.packets();
        }

        if video_pipeline.is_active() {
            let audio_needs_frames = if let (true, Some(sink)) = (audio_enabled, audio_sink_opt.as_ref()) {
                sink.needs_audio()
            } else {
                false
            };
            
            let queue_needs_frames = video_sink.needs_frames() || audio_needs_frames;
            
            if queue_needs_frames
                && let Some(packet_res) = packets_iter.next() {
                    let (stream, packet) = packet_res;
                    let stream_index = stream.index();
                    if stream_index == video_stream_index {
                        if let Ok(frames) = video_pipeline.process_packet(&packet) {
                            video_sink.push(frames);
                        }
                    } else if Some(stream_index) == audio_stream_index_opt && audio_enabled
                        && let Some(audio_pipeline) = audio_pipeline_opt.as_mut() {
                            if let Some(pts) = packet.pts() {
                                audio_pipeline.update_current_time_from_pts(pts);
                            }
                            if audio_pipeline.send_packet(&packet).is_ok()
                                && let Ok(chunks) = audio_pipeline.receive_samples() {
                                    for chunk in &chunks {
                                        for (bucket_idx, rms) in crate::media::audio_pipeline::compute_rms_buckets(
                                            chunk,
                                            fps,
                                            target_channels,
                                            target_sample_rate,
                                        ) {
                                            let _ = loudness_tx.try_send((bucket_idx, rms));
                                        }
                                    }
                                    if let Some(sink) = audio_sink_opt.as_mut() {
                                        sink.push(chunks);
                                    }
                                }
                        }
                }
        }
    }
}
