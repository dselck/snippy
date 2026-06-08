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
        sequence_id: usize,
    },
    Gpu {
        frame_idx: usize,
        descriptor: GpuFrameDescriptor,
        sequence_id: usize,
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
    Seek { frame_index: usize, keyframe_only: bool, sequence_id: usize },
    Shutdown,
}

pub enum VideoThreadCommand {
    Seek {
        frame_index: usize,
        keyframe_only: bool,
        sequence_id: usize,
        resp_tx: Sender<bool>,
    },
    Play,
}

pub enum AudioThreadCommand {
    Seek { target_time: f64, resp_tx: Sender<()> },
}

pub struct VideoMetadata {
    pub total_frames: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
    pub keyframes: Vec<usize>,
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
        playback_state: Arc<crate::audio::AudioPlaybackState>,
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

        // Scan keyframes synchronously
        let keyframes = {
            let mut scan = ffmpeg_next::format::input(&path_str)?;
            let video_stream_idx = stream.index();
            let mut kfs = Vec::new();
            let mut frame_idx: usize = 0;
            for (stream, packet) in scan.packets() {
                if stream.index() == video_stream_idx {
                    if packet.is_key() {
                        kfs.push(frame_idx);
                    }
                    frame_idx += 1;
                }
            }
            if kfs.is_empty() || kfs[0] != 0 {
                kfs.insert(0, 0);
            }
            kfs
        };

        let metadata = VideoMetadata {
            total_frames,
            width: decoder.width(),
            height: decoder.height(),
            fps,
            has_audio,
            keyframes,
        };

        // 2. Spawn the background Media Engine decoding loop
        let (cmd_tx, background_rx) = crossbeam_channel::unbounded();
        let fps_clone = metadata.fps;
        let playback_state_clone = Arc::clone(&playback_state);
        let thread_handle = thread::spawn(move || {
            let _ = run_decoding_loop(
                &path_str,
                target_sample_rate,
                target_channels,
                frame_tx,
                audio_tx,
                background_rx,
                cmd_rx,
                fps_clone,
                playback_state_clone,
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
    mut audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
    background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
    fps: f64,
    playback_state: Arc<crate::audio::AudioPlaybackState>,
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
    let (audio_packet_tx, audio_packet_rx) = crossbeam_channel::unbounded::<ffmpeg_next::Packet>();
    let (video_cmd_tx, video_cmd_rx) = crossbeam_channel::bounded::<VideoThreadCommand>(1);
    let (audio_cmd_tx, audio_cmd_rx) = crossbeam_channel::bounded::<AudioThreadCommand>(1);

    // Spawn Video Decoder Thread
    let video_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
    let mut video_decoder = video_codec_ctx.decoder().video()?;
    let frame_tx_clone = frame_tx.clone();
    let time_base = video_stream.time_base();
    let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;
    let video_start_time = if video_stream.start_time() > -1_000_000_000_000 {
        video_stream.start_time() as f64 * time_base_f64
    } else {
        0.0
    };
    
    let video_thread = thread::spawn(move || {

        let mut fallback_frame_idx = 0;
        let mut scaler: Option<ffmpeg_next::software::scaling::context::Context> = None;

        let mut keyframe_only_mode = false;
        let mut decode_more = true;
        let mut current_sequence_id = 0;

        loop {
            if decode_more {
                crossbeam_channel::select! {
                    recv(video_packet_rx) -> packet_res => {
                        if let Ok(packet) = packet_res {
                            if video_decoder.send_packet(&packet).is_ok() {
                                let mut frame = ffmpeg_next::util::frame::Video::empty();
                                while video_decoder.receive_frame(&mut frame).is_ok() {
                                    let mut scaled_frame = ffmpeg_next::util::frame::Video::empty();
                                    let video_frame = if frame.format() != ffmpeg_next::format::Pixel::YUV420P {
                                        let s = match scaler {
                                            Some(ref mut s) if s.input().format == frame.format()
                                                && s.input().width == frame.width()
                                                && s.input().height == frame.height() => s,
                                            _ => {
                                                let new_s = ffmpeg_next::software::scaling::context::Context::get(
                                                    frame.format(),
                                                    frame.width(),
                                                    frame.height(),
                                                    ffmpeg_next::format::Pixel::YUV420P,
                                                    frame.width(),
                                                    frame.height(),
                                                    ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
                                                ).unwrap();
                                                scaler = Some(new_s);
                                                scaler.as_mut().unwrap()
                                            }
                                        };
                                        s.run(&frame, &mut scaled_frame).unwrap();
                                        &scaled_frame
                                    } else {
                                        &frame
                                    };

                                    let w = video_frame.width();
                                    let h = video_frame.height();
                                    
                                    let planes = YuvPlanes {
                                        y: video_frame.data(0).to_vec(),
                                        u: video_frame.data(1).to_vec(),
                                        v: video_frame.data(2).to_vec(),
                                        width: w,
                                        height: h,
                                        y_stride: video_frame.stride(0) as u32,
                                        u_stride: video_frame.stride(1) as u32,
                                        v_stride: video_frame.stride(2) as u32,
                                    };

                                    let frame_idx = match frame.pts() {
                                        Some(pts_val) => {
                                            let time_sec = pts_val as f64 * time_base_f64 - video_start_time;
                                            (time_sec * fps).round() as usize
                                        }
                                        None => fallback_frame_idx,
                                    };
                                    fallback_frame_idx = frame_idx + 1;

                                    let decoded = DecodedFrame::Cpu {
                                        frame_idx,
                                        planes: planes.clone(),
                                        sequence_id: current_sequence_id,
                                    };
                                    

                                    
                                    let mut sent = false;
                                    crossbeam_channel::select! {
                                        send(frame_tx_clone, decoded) -> res => {
                                            if res.is_ok() {
                                                sent = true;
                                            }
                                        }
                                        recv(video_cmd_rx) -> cmd_res => {
                                            if let Ok(cmd) = cmd_res {
                                                match cmd {
                                                    VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, resp_tx } => {
                                                        fallback_frame_idx = frame_index;
                                                        keyframe_only_mode = keyframe_only;
                                                        current_sequence_id = sequence_id;
                                                        while video_packet_rx.try_recv().is_ok() {}
                                                        video_decoder.flush();
                                                        let _ = resp_tx.send(false); // Always force demuxer seek to sync audio
                                                        decode_more = true;
                                                    }
                                                    VideoThreadCommand::Play => {
                                                        keyframe_only_mode = false;
                                                        decode_more = true;
                                                    }
                                                }
                                                break; // break the receive_frame loop
                                            } else {
                                                return;
                                            }
                                        }
                                    }

                                    if !sent {
                                        break; // abort further decoding for the current packet
                                    }

                                    if keyframe_only_mode {
                                        decode_more = false;
                                        break;
                                    }
                                }
                            }
                        } else {
                            break;
                        }
                    }
                    recv(video_cmd_rx) -> cmd_res => {
                        match cmd_res {
                            Ok(VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, resp_tx }) => {
                                fallback_frame_idx = frame_index;
                                keyframe_only_mode = keyframe_only;
                                current_sequence_id = sequence_id;

                                while video_packet_rx.try_recv().is_ok() {}
                                video_decoder.flush();
                                let _ = resp_tx.send(false); // Always force demuxer seek to sync audio
                                decode_more = true;
                            }
                            Ok(VideoThreadCommand::Play) => {
                                keyframe_only_mode = false;
                                decode_more = true;
                            }
                            Err(_) => break,
                        }
                    }
                }
            } else {
                match video_cmd_rx.recv() {
                    Ok(VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, resp_tx }) => {
                        fallback_frame_idx = frame_index;
                        keyframe_only_mode = keyframe_only;
                        current_sequence_id = sequence_id;
                        while video_packet_rx.try_recv().is_ok() {}
                        video_decoder.flush();
                        let _ = resp_tx.send(false); // Always force demuxer seek to sync audio
                        decode_more = true;
                    }
                    Ok(VideoThreadCommand::Play) => {
                        keyframe_only_mode = false;
                        decode_more = true;
                    }
                    Err(_) => break,
                }
            }
        }
    });

    // Spawn Audio Decoder Thread
    let audio_thread_opt = if let Some(ref stream) = audio_stream_opt {
        let audio_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let mut audio_decoder = audio_codec_ctx.decoder().audio()?;
        let audio_time_base = stream.time_base();
        let audio_tb_f64 = audio_time_base.numerator() as f64 / audio_time_base.denominator() as f64;
        
        let audio_thread = thread::spawn(move || {
            let mut resampler: Option<ResamplerContext> = None;
            let mut skip_until_time = 0.0;
            let mut is_first_audio_frame = true;
            let mut current_audio_time = 0.0;

            loop {
                if let Ok(AudioThreadCommand::Seek { target_time, resp_tx }) = audio_cmd_rx.try_recv() {
                    while audio_packet_rx.try_recv().is_ok() {}
                    audio_decoder.flush();
                    resampler = None;
                    skip_until_time = target_time;
                    is_first_audio_frame = true;
                    current_audio_time = video_start_time + target_time;
                    playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                    let _ = resp_tx.send(());
                    continue;
                }

                crossbeam_channel::select! {
                    recv(audio_packet_rx) -> packet_res => {
                        if let Ok(packet) = packet_res {
                            if is_first_audio_frame {
                                if let Some(pts) = packet.pts() {
                                    current_audio_time = pts as f64 * audio_tb_f64;
                                }
                            }
                            if audio_decoder.send_packet(&packet).is_ok() {
                                    let mut seek_aborted = false;
                                    let mut frame = ffmpeg_next::util::frame::Audio::empty();
                                    while audio_decoder.receive_frame(&mut frame).is_ok() {
                                         if let Ok(AudioThreadCommand::Seek { target_time, resp_tx }) = audio_cmd_rx.try_recv() {
                                             while audio_packet_rx.try_recv().is_ok() {}
                                             audio_decoder.flush();
                                             resampler = None;
                                             skip_until_time = target_time;
                                             is_first_audio_frame = true;
                                             current_audio_time = video_start_time + target_time;
                                             playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                                             let _ = resp_tx.send(());
                                             break;
                                         }

                                         let duration_sec = frame.samples() as f64 / frame.rate() as f64;
                                         let pts_sec = match frame.pts() {
                                             Some(pts) => pts as f64 * audio_tb_f64,
                                             None => current_audio_time,
                                         };
                                         current_audio_time = pts_sec + duration_sec;

                                         if is_first_audio_frame {
                                             let target_container_time = video_start_time + skip_until_time;
                                             if pts_sec < target_container_time {
                                                 continue; // Discard whole frame before seek target
                                             }
                                             is_first_audio_frame = false;
                                             let stream_time = pts_sec - video_start_time;
                                             playback_state.samples_played.store((stream_time * playback_state.sample_rate as f64) as u64, std::sync::atomic::Ordering::SeqCst);
                                         }

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
                                                 let byte_count = sample_count * target_channels * 4;
                                                 let data: &[f32] = bytemuck::cast_slice(&resampled.data(0)[..byte_count]);
                                                 
                                                 for &sample in data {
                                                     while audio_tx.try_push(sample).is_err() {
                                                         if let Ok(cmd) = audio_cmd_rx.try_recv() {
                                                             match cmd {
                                                                 AudioThreadCommand::Seek { target_time, resp_tx } => {
                                                                     while audio_packet_rx.try_recv().is_ok() {}
                                                                     audio_decoder.flush();
                                                                     resampler = None;
                                                                     skip_until_time = target_time;
                                                                     is_first_audio_frame = true;
                                                                     current_audio_time = video_start_time + target_time;
                                                                     playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                                                                     let _ = resp_tx.send(());
                                                                     seek_aborted = true;
                                                                     break;
                                                                 }
                                                             }
                                                         }
                                                         thread::sleep(std::time::Duration::from_millis(5));
                                                     }
                                                     if seek_aborted {
                                                         break;
                                                     }
                                                 }
                                             }
                                         }

                                         if seek_aborted {
                                             break;
                                         }
                                     }
                                }
                            } else {
                                break;
                            }
                    }
                    recv(audio_cmd_rx) -> cmd_res => {
                        match cmd_res {
                            Ok(AudioThreadCommand::Seek { target_time, resp_tx }) => {
                                while audio_packet_rx.try_recv().is_ok() {}
                                audio_decoder.flush();
                                resampler = None;
                                skip_until_time = target_time;
                                is_first_audio_frame = true;
                                current_audio_time = video_start_time + target_time;
                                playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                                let _ = resp_tx.send(());
                            }
                            Err(_) => break,
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
    let mut pending_packet = None;
    let mut seek_requested = None;

    loop {
        // 1. Handle incoming command overrides
        while let Ok(cmd) = background_rx.try_recv() {
            match cmd {
                MediaCommand::Shutdown => return Ok(()),
                MediaCommand::Play => {
                    let _ = video_cmd_tx.send(VideoThreadCommand::Play);
                }
                MediaCommand::Pause => {}
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id } => {
                    seek_requested = Some((frame_index, keyframe_only, sequence_id));
                }
            }
        }

        if let Some((frame_index, keyframe_only, sequence_id)) = seek_requested.take() {
            let (video_resp_tx, video_resp_rx) = crossbeam_channel::bounded::<bool>(1);
            let _ = video_cmd_tx.send(VideoThreadCommand::Seek {
                frame_index,
                keyframe_only,
                sequence_id,
                resp_tx: video_resp_tx,
            });

            let mut audio_resp_rx = None;
            if audio_stream_idx_opt.is_some() {
                let (audio_resp_tx, rx) = crossbeam_channel::bounded::<()>(1);
                let target_time = frame_index as f64 / fps;
                let _ = audio_cmd_tx.send(AudioThreadCommand::Seek {
                    target_time,
                    resp_tx: audio_resp_tx,
                });
                audio_resp_rx = Some(rx);
            }

            let video_found = video_resp_rx.recv().unwrap_or(false);
            if let Some(rx) = audio_resp_rx {
                let _ = rx.recv();
            }

            if !video_found {
                // Drop packets_iter to release borrow on input
                drop(packets_iter);

                // Flush queues and seeking input context
                let seek_pts = ((video_start_time + frame_index as f64 / fps) * 1_000_000.0) as i64;
                let _ = input.seek(seek_pts, ..seek_pts + 1);

                // Recreate packets_iter
                packets_iter = input.packets();
            }
            pending_packet = None; // Clear any pending packet on seek
            continue;
        }

        // 2. Demux next packet if we don't have one pending
        if pending_packet.is_none() {
            if let Some(packet_res) = packets_iter.next() {
                let (stream, packet) = packet_res;
                pending_packet = Some((stream.index(), packet));
            } else {
                // End of stream - wait for further commands (e.g. seek)
                match background_rx.recv() {
                    Ok(cmd) => {
                        match cmd {
                            MediaCommand::Shutdown => return Ok(()),
                            MediaCommand::Play => {
                                let _ = video_cmd_tx.send(VideoThreadCommand::Play);
                            }
                            MediaCommand::Pause => {}
                            MediaCommand::Seek { frame_index, keyframe_only, sequence_id } => {
                                seek_requested = Some((frame_index, keyframe_only, sequence_id));
                            }
                        }
                        if let Some((frame_index, keyframe_only, sequence_id)) = seek_requested.take() {
                            let (video_resp_tx, video_resp_rx) = crossbeam_channel::bounded::<bool>(1);
                            let _ = video_cmd_tx.send(VideoThreadCommand::Seek {
                                frame_index,
                                keyframe_only,
                                sequence_id,
                                resp_tx: video_resp_tx,
                            });

                            let mut audio_resp_rx = None;
                            if audio_stream_idx_opt.is_some() {
                                let (audio_resp_tx, rx) = crossbeam_channel::bounded::<()>(1);
                                let target_time = frame_index as f64 / fps;
                                let _ = audio_cmd_tx.send(AudioThreadCommand::Seek {
                                    target_time,
                                    resp_tx: audio_resp_tx,
                                });
                                audio_resp_rx = Some(rx);
                            }

                            let video_found = video_resp_rx.recv().unwrap_or(false);
                            if let Some(rx) = audio_resp_rx {
                                let _ = rx.recv();
                            }

                            if !video_found {
                                drop(packets_iter);
                                let seek_pts = ((video_start_time + frame_index as f64 / fps) * 1_000_000.0) as i64;
                                let _ = input.seek(seek_pts, ..seek_pts + 1);
                                packets_iter = input.packets();
                            }
                        }
                        continue;
                    }
                    Err(_) => break,
                }
            }
        }

        // 3. Send the pending packet, non-blocking on command channel
        if let Some((stream_idx, packet)) = pending_packet.take() {
            if stream_idx == video_stream_idx {
                crossbeam_channel::select! {
                    send(video_packet_tx, packet) -> res => {
                        if res.is_err() {
                            break;
                        }
                    }
                    recv(background_rx) -> cmd_res => {
                        if let Ok(cmd) = cmd_res {
                            match cmd {
                                MediaCommand::Shutdown => return Ok(()),
                                MediaCommand::Play => {
                                    let _ = video_cmd_tx.send(VideoThreadCommand::Play);
                                }
                                MediaCommand::Pause => {}
                                MediaCommand::Seek { frame_index, keyframe_only, sequence_id } => {
                                    seek_requested = Some((frame_index, keyframe_only, sequence_id));
                                }
                            }
                            pending_packet = Some((stream_idx, packet));
                            if seek_requested.is_some() {
                                pending_packet = None;
                            }
                        } else {
                            break;
                        }
                    }
                }
            } else if Some(stream_idx) == audio_stream_idx_opt {
                crossbeam_channel::select! {
                    send(audio_packet_tx, packet) -> res => {
                        if res.is_err() {
                            break;
                        }
                    }
                    recv(background_rx) -> cmd_res => {
                        if let Ok(cmd) = cmd_res {
                            match cmd {
                                MediaCommand::Shutdown => return Ok(()),
                                MediaCommand::Play => {
                                    let _ = video_cmd_tx.send(VideoThreadCommand::Play);
                                }
                                MediaCommand::Pause => {}
                                MediaCommand::Seek { frame_index, keyframe_only, sequence_id } => {
                                    seek_requested = Some((frame_index, keyframe_only, sequence_id));
                                }
                            }
                            pending_packet = Some((stream_idx, packet));
                            if seek_requested.is_some() {
                                pending_packet = None;
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
        }
    }

    // Explicitly drop transmitters to notify receiver threads
    drop(video_packet_tx);
    drop(video_cmd_tx);
    drop(audio_packet_tx);
    drop(audio_cmd_tx);

    let _ = video_thread.join();
    if let Some(ath) = audio_thread_opt {
        let _ = ath.join();
    }

    Ok(())
}
