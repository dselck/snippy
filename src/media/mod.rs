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

pub mod decoder_pipeline;
pub mod ffmpeg_adapter;
pub mod audio_pipeline;
pub mod sync;

pub use crate::media::types::*;
pub use crate::media::cache::*;
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
        tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>,
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

fn get_nearest_keyframe(keyframes: &[usize], target: usize) -> usize {
    if keyframes.is_empty() {
        return target;
    }
    match keyframes.binary_search(&target) {
        Ok(i) => keyframes[i],
        Err(0) => keyframes[0],
        Err(i) => keyframes[i - 1],
    }
}

fn run_decoding_loop(
    path: &str,
    target_sample_rate: u32,
    target_channels: usize,
    frame_tx: Sender<DecodedFrame>,
    audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>,
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

    // Run Presenter Thread (current thread)
    let mut sync_engine = crate::media::sync::VideoSyncEngine::new(Box::new(master_clock.clone()));
    let mut playing = false;
    let mut current_frame: Option<DecodedFrame> = None;
    let mut pending_cmd: Option<MediaCommand> = None;
    let mut wait_duration: Option<std::time::Duration>;

    loop {
        // 1. Process incoming commands from UI/test rig
        let cmd_opt = pending_cmd.take().or_else(|| background_rx.try_recv().ok());
        if let Some(cmd) = cmd_opt {
            match cmd {
                MediaCommand::Shutdown => {
                    let _ = decoder_cmd_tx.send(MediaCommand::Shutdown);
                    return Ok(());
                }
                MediaCommand::Play => {
                    playing = true;
                    sync_engine.reset();
                    let _ = decoder_cmd_tx.send(MediaCommand::Play);
                }
                MediaCommand::Pause => {
                    playing = false;
                    current_frame = None;
                    while play_queue_rx.try_recv().is_ok() {}
                    let _ = decoder_cmd_tx.send(MediaCommand::Pause);
                }
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled } => {
                    playing = false;
                    current_frame = None;
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

        // 2. Fetch frame if needed
        if current_frame.is_none() {
            if playing {
                if let Ok(frame) = play_queue_rx.try_recv() {
                    current_frame = Some(frame);
                }
            } else {
                while let Ok(frame) = play_queue_rx.try_recv() {
                    let _ = frame_tx.send(frame);
                }
            }
        }

        // 3. Present current frame if due
        wait_duration = None;
        if playing {
            if let Some(frame) = &current_frame {
                let pts_sec = frame.pts_sec();
                let action = if master_clock.is_playing() {
                    sync_engine.evaluate_frame(pts_sec)
                } else {
                    Ok(SyncAction::Present)
                };
                match action {
                    Ok(SyncAction::Present) => {
                        let frame = current_frame.take().unwrap();
                        let _ = frame_tx.send(frame);
                    }
                    Ok(SyncAction::Wait(remaining)) => {
                        wait_duration = Some(remaining);
                    }
                    Err(_) => {
                        let frame = current_frame.take().unwrap();
                        let _ = frame_tx.send(frame);
                    }
                }
            }
        }

        // 4. Wait / sleep efficiently
        if playing {
            if let Some(remaining) = wait_duration {
                if remaining > std::time::Duration::from_millis(1) {
                    let wait_cap = std::time::Duration::from_millis(10);
                    if let Ok(cmd) = background_rx.recv_timeout(remaining.min(wait_cap)) {
                        pending_cmd = Some(cmd);
                    }
                }
            } else {
                crossbeam_channel::select! {
                    recv(background_rx) -> cmd => {
                        if let Ok(c) = cmd {
                            pending_cmd = Some(c);
                        }
                    }
                    recv(play_queue_rx) -> frame => {
                        if let Ok(f) = frame {
                            current_frame = Some(f);
                        }
                    }
                    default(std::time::Duration::from_millis(2)) => {}
                }
            }
        } else {
            crossbeam_channel::select! {
                recv(background_rx) -> cmd => {
                    if let Ok(c) = cmd {
                        pending_cmd = Some(c);
                    }
                }
                recv(play_queue_rx) -> frame => {
                    if let Ok(f) = frame {
                        let _ = frame_tx.send(f);
                    }
                }
            }
        }
    }
}

fn run_decoder_thread(
    path: String,
    target_sample_rate: u32,
    target_channels: usize,
    play_queue_tx: Sender<DecodedFrame>,
    mut audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    mut tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>,
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
        let codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
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

    let mut video_pipeline = {
        let adapter = Box::new(crate::media::ffmpeg_adapter::FfmpegDecoderAdapter::new(
            video_decoder,
            video_time_base_f64,
            video_start_time,
            fps,
        ));
        crate::media::decoder_pipeline::VideoDecoderPipeline::new(adapter, keyframes.clone())
    };

    let mut audio_pipeline_opt = audio_pipeline_opt;

    let mut audio_enabled = true;
    master_clock.set_has_audio(has_audio_stream && audio_enabled);
    let mut packets_iter = input.packets();
    let mut is_decoding_active = video_pipeline.is_active();
    let mut pending_cmd: Option<MediaCommand> = None;

    loop {
        let cmd_opt = pending_cmd.take().or_else(|| {
            if !is_decoding_active {
                decoder_cmd_rx.recv().ok()
            } else {
                decoder_cmd_rx.try_recv().ok()
            }
        });

        if let Some(cmd) = cmd_opt {
            match cmd {
                MediaCommand::Shutdown => return Ok(()),
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
                MediaCommand::Pause => {
                    video_pipeline.pause();
                }
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
                    audio_enabled = ae;
                    master_clock.set_has_audio(has_audio_stream && audio_enabled);
                    let handled = video_pipeline.seek(frame_index, keyframe_only, sequence_id, audio_enabled);

                    if let Some(audio_pipeline) = audio_pipeline_opt.as_mut() {
                        if audio_enabled {
                            audio_pipeline.handle_seek(frame_index as f64 / fps);
                            master_clock.inc_playback_generation();
                        }
                    }

                    if handled {
                        if let Some(frame) = video_pipeline.get_cached_frame(frame_index) {
                            let _ = play_queue_tx.send(frame);
                        }
                    } else {
                        drop(packets_iter);

                        let target_kf = get_nearest_keyframe(&keyframes, frame_index);
                        let kf_pts = keyframes.binary_search(&target_kf)
                            .map(|idx| keyframes_pts[idx])
                            .unwrap_or(0);


                        unsafe {
                            let _ = ffmpeg_next::ffi::avformat_seek_file(
                                input.as_mut_ptr(),
                                video_stream_idx as i32,
                                i64::MIN,
                                kf_pts,
                                kf_pts,
                                0,
                            );
                        }

                        packets_iter = input.packets();
                    }
                }
            }
            is_decoding_active = video_pipeline.is_active();

            continue;
        }

        if is_decoding_active {
            let audio_needs_frames = audio_enabled && (audio_tx.vacant_len() >= 2048);
            let queue_needs_frames = (play_queue_tx.len() < 20) || audio_needs_frames;

            if queue_needs_frames {
                if let Some(packet_res) = packets_iter.next() {
                    let (stream, packet) = packet_res;
                    let stream_idx = stream.index();

                    if stream_idx == video_stream_idx {
                        if let Ok(frames) = video_pipeline.process_packet(&packet) {

                            for frame in frames {
                                let mut f_opt = Some(frame);
                                while let Some(f) = f_opt.take() {
                                    crossbeam_channel::select! {
                                        send(play_queue_tx, f) -> res => {
                                            if res.is_err() {
                                                return Ok(());
                                            }
                                        }
                                        recv(decoder_cmd_rx) -> cmd => {
                                            if let Ok(c) = cmd {
                                                match c {
                                                    MediaCommand::Shutdown => return Ok(()),
                                                    MediaCommand::Play => {
                                                        video_pipeline.play();
                                                        f_opt = Some(f);
                                                    }
                                                    MediaCommand::Pause => {
                                                        video_pipeline.pause();
                                                    }
                                                    MediaCommand::Seek { .. } => {
                                                        pending_cmd = Some(c);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if Some(stream_idx) == audio_stream_idx_opt && audio_enabled {
                        if let Some(audio_pipeline) = audio_pipeline_opt.as_mut() {
                            if let Some(pts) = packet.pts() {
                                audio_pipeline.update_current_time_from_pts(pts);
                            }
                            if audio_pipeline.send_packet(&packet).is_ok() {
                                if let Ok(chunks) = audio_pipeline.receive_samples() {
                                    let generation = master_clock.playback_generation();
                                    for chunk in chunks {
                                        if let Some(tag) = chunk.tag {
                                            let _ = tags_tx.try_push(tag);
                                        }
                                        let mut pushed_all = true;
                                        for &sample in &chunk.data {
                                            let mut retries = 0;
                                            while audio_tx.try_push((sample, generation)).is_err() {
                                                if retries >= 3 {
                                                    pushed_all = false;
                                                    break;
                                                }
                                                std::thread::sleep(std::time::Duration::from_millis(5));
                                                retries += 1;
                                            }
                                            if !pushed_all {
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {

                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            } else {
                crossbeam_channel::select! {
                    recv(decoder_cmd_rx) -> cmd => {
                        if let Ok(c) = cmd {
                            pending_cmd = Some(c);
                        }
                    }
                    default(std::time::Duration::from_millis(5)) => {}
                }
            }
        }
        is_decoding_active = video_pipeline.is_active();
    }
}
