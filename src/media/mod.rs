use crossbeam_channel::{Receiver, Sender};
use std::collections::VecDeque;
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
    mut audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    mut tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>,
    background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
    fps: f64,
    master_clock: crate::audio::clock::MasterClock,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
) -> Result<(), anyhow::Error> {
    let mut input = ffmpeg_next::format::input(&path)?;

    let video_stream = input
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("No video stream"))?;
    let video_stream_idx = video_stream.index();

    let audio_stream_opt = input.streams().best(ffmpeg_next::media::Type::Audio);
    let audio_stream_idx_opt = audio_stream_opt.as_ref().map(|s| s.index());

    let mut video_pipeline = {
        let video_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
        let video_decoder = video_codec_ctx.decoder().video()?;
        let time_base = video_stream.time_base();
        let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;
        let video_start_time = if video_stream.start_time() > -1_000_000_000_000 {
            video_stream.start_time() as f64 * time_base_f64
        } else {
            0.0
        };
        let adapter = Box::new(crate::media::ffmpeg_adapter::FfmpegDecoderAdapter::new(
            video_decoder,
            time_base_f64,
            video_start_time,
            fps,
        ));
        crate::media::decoder_pipeline::VideoDecoderPipeline::new(adapter, keyframes.clone())
    };

    let mut audio_pipeline_opt = if let Some(ref stream) = audio_stream_opt {
        let audio_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let audio_decoder = audio_codec_ctx.decoder().audio()?;
        let audio_time_base = stream.time_base();
        let audio_tb_f64 = audio_time_base.numerator() as f64 / audio_time_base.denominator() as f64;
        let video_start_time = if video_stream.start_time() > -1_000_000_000_000 {
            video_stream.start_time() as f64 * (video_stream.time_base().numerator() as f64 / video_stream.time_base().denominator() as f64)
        } else {
            0.0
        };
        Some(crate::media::audio_pipeline::AudioDecoderPipeline::new(
            audio_decoder,
            target_sample_rate,
            target_channels,
            audio_tb_f64,
            video_start_time,
        ))
    } else {
        None
    };

    let mut sync_engine = crate::media::sync::VideoSyncEngine::new(Box::new(master_clock.clone()));
    let mut audio_enabled = true;
    let mut packets_iter = input.packets();

    let mut play_queue: VecDeque<DecodedFrame> = VecDeque::new();
    const MAX_LOOKAHEAD_FRAMES: usize = 15;

    loop {
        // 1. Process incoming commands
        if let Ok(cmd) = background_rx.try_recv() {
            match cmd {
                MediaCommand::Shutdown => return Ok(()),
                MediaCommand::Play => {
                    video_pipeline.play();
                    sync_engine.reset();
                }
                MediaCommand::Pause => {}
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
                    audio_enabled = ae;
                    play_queue.clear();
                    let handled = video_pipeline.seek(frame_index, keyframe_only, sequence_id, audio_enabled);

                    if let Some(mut audio_pipeline) = audio_pipeline_opt.as_mut() {
                        if audio_enabled {
                            audio_pipeline.handle_seek(frame_index as f64 / fps);
                            master_clock.inc_playback_generation();
                        }
                    }

                    if handled {
                        if let Some(frame) = video_pipeline.get_cached_frame(frame_index) {
                            let _ = frame_tx.send(frame);
                        }
                    } else {
                        // Drop packets iterator to free mutable borrow
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

                        // Recreate the iterator
                        packets_iter = input.packets();
                    }
                    continue;
                }
            }
        }

        // 2. Present due frames from the front of the queue
        if master_clock.is_playing() {
            if let Some(front_frame) = play_queue.front() {
                let pts_sec = match front_frame {
                    DecodedFrame::Cpu { pts_sec, .. } => *pts_sec,
                    DecodedFrame::Gpu { pts_sec, .. } => *pts_sec,
                };

                let action = sync_engine.evaluate_frame(pts_sec);
                match action {
                    Ok(SyncAction::Present) => {
                        let frame = play_queue.pop_front().unwrap();
                        let _ = frame_tx.send(frame);
                    }
                    Ok(SyncAction::Wait(_)) => {
                        // Frame is not due yet, don't block. We can decode more or sleep.
                    }
                    Err(_) => {
                        let frame = play_queue.pop_front().unwrap();
                        let _ = frame_tx.send(frame);
                    }
                }
            }
        } else {
            // Paused/seek mode: drain queue and send immediately
            while let Some(frame) = play_queue.pop_front() {
                let _ = frame_tx.send(frame);
            }
        }

        // 3. Decode ahead if queue has space and decoder is active
        let queue_needs_frames = play_queue.len() < MAX_LOOKAHEAD_FRAMES;
        let is_decoding_active = video_pipeline.is_active();

        if queue_needs_frames && is_decoding_active {
            // Rate limit Demuxer if audio ringbuffer is too full (avoid blocking)
            if audio_tx.is_full() {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }

            if let Some(packet_res) = packets_iter.next() {
                let (stream, packet) = packet_res;
                let stream_idx = stream.index();

                if stream_idx == video_stream_idx {
                    if let Ok(frames) = video_pipeline.process_packet(&packet) {
                        for frame in frames {
                            play_queue.push_back(frame);
                        }
                    }
                } else if Some(stream_idx) == audio_stream_idx_opt && audio_enabled {
                    if let Some(mut audio_pipeline) = audio_pipeline_opt.as_mut() {
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
                                    for &sample in &chunk.data {
                                        while audio_tx.try_push((sample, generation)).is_err() {
                                            std::thread::sleep(std::time::Duration::from_millis(1));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                // EOF, sleep a bit to avoid hotloop
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        } else {
            // Queue is full or paused: sleep briefly to prevent 100% CPU hot loop
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}
