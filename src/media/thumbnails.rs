use crossbeam_channel::{Receiver, Sender};
use std::thread;

#[derive(Debug, Clone)]
pub enum ThumbnailCommand {
    Request { frames: Vec<usize>, height: u32 },
    Clear,
}

#[derive(Debug)]
pub struct ThumbnailResponse {
    pub frame_idx: usize,
    pub width: u32,
    pub height: u32,
    pub rgba_data: Vec<u8>,
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

pub fn spawn_thumbnail_decoder(
    path: String,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
    fps: f64,
) -> (Sender<ThumbnailCommand>, Receiver<ThumbnailResponse>, thread::JoinHandle<()>) {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
    let (resp_tx, resp_rx) = crossbeam_channel::bounded(100);

    let handle = thread::spawn(move || {
        let mut input = match ffmpeg_next::format::input(&path) {
            Ok(input) => input,
            Err(_) => return,
        };

        let stream = match input.streams().best(ffmpeg_next::media::Type::Video) {
            Some(s) => s,
            None => return,
        };
        let video_stream_index = stream.index();
        let time_base = stream.time_base();
        let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;
        let start_time = if stream.start_time() > -1_000_000_000_000 {
            stream.start_time() as f64 * time_base_f64
        } else {
            0.0
        };

        let codec_ctx = match ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()) {
            Ok(ctx) => ctx,
            Err(_) => return,
        };
        let mut decoder = match codec_ctx.decoder().video() {
            Ok(dec) => dec,
            Err(_) => return,
        };

        let aspect_ratio = if decoder.width() > 0 && decoder.height() > 0 {
            decoder.width() as f32 / decoder.height() as f32
        } else {
            16.0 / 9.0
        };

        let mut current_height: Option<u32> = None;
        let mut scaler: Option<ffmpeg_next::software::scaling::context::Context> = None;

        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                ThumbnailCommand::Clear => {
                    // Drain pending commands
                    while cmd_rx.try_recv().is_ok() {}
                }
                ThumbnailCommand::Request { frames, height } => {
                    // Recreate scaler if requested height changes
                    if current_height != Some(height) {
                        let thumb_width = (height as f32 * aspect_ratio).round() as u32;
                        if let Ok(s) = ffmpeg_next::software::scaling::context::Context::get(
                            decoder.format(),
                            decoder.width(),
                            decoder.height(),
                            ffmpeg_next::format::Pixel::RGBA,
                            thumb_width,
                            height,
                            ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
                        ) {
                            scaler = Some(s);
                            current_height = Some(height);
                        }
                    }

                    let current_scaler = match scaler.as_mut() {
                        Some(s) => s,
                        None => continue,
                    };
                    let active_height = current_height.unwrap_or(72);
                    let active_width = (active_height as f32 * aspect_ratio).round() as u32;

                    for target_frame in frames {
                        // Check if a Clear command has arrived
                        let mut aborted = false;
                        loop {
                            match cmd_rx.try_recv() {
                                Ok(ThumbnailCommand::Clear) => {
                                    aborted = true;
                                    break;
                                }
                                Ok(ThumbnailCommand::Request { .. }) => {
                                    // Skip outdated requests
                                }
                                Err(_) => break,
                            }
                        }
                        if aborted {
                            break;
                        }

                        let target_kf = get_nearest_keyframe(&keyframes, target_frame);
                        let kf_idx = keyframes.binary_search(&target_kf).unwrap_or(0);
                        let pts = if kf_idx < keyframes_pts.len() {
                            keyframes_pts[kf_idx]
                        } else {
                            0
                        };
                        let pts_us = (pts as f64 * time_base_f64 * 1_000_000.0).round() as i64;

                        if input.seek(pts_us, ..pts_us).is_ok() {
                            decoder.flush();
                            let mut fallback_frame_idx = target_kf;
                            let mut packets_iter = input.packets();

                            while let Some((stream_ref, packet)) = packets_iter.next() {
                                // Preemption check inside decoding loop
                                if cmd_rx.try_recv().is_ok() {
                                    aborted = true;
                                    break;
                                }

                                if stream_ref.index() == video_stream_index {
                                    if decoder.send_packet(&packet).is_ok() {
                                        let mut frame = ffmpeg_next::util::frame::Video::empty();
                                        let mut frame_found = false;

                                        while decoder.receive_frame(&mut frame).is_ok() {
                                            let pts_sec = if let Some(timestamp) = frame.timestamp() {
                                                timestamp as f64 * time_base_f64 - start_time
                                            } else if let Some(pts) = frame.pts() {
                                                pts as f64 * time_base_f64 - start_time
                                            } else {
                                                let pkt_dts = unsafe { (*frame.as_ptr()).pkt_dts };
                                                if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                                                    pkt_dts as f64 * time_base_f64 - start_time
                                                } else {
                                                    fallback_frame_idx as f64 / fps
                                                }
                                            };
                                            let frame_idx = (pts_sec * fps).round() as usize;
                                            fallback_frame_idx = frame_idx + 1;

                                            if frame_idx >= target_frame {
                                                let mut scaled_frame = ffmpeg_next::util::frame::Video::empty();
                                                if current_scaler.run(&frame, &mut scaled_frame).is_ok() {
                                                    let _ = resp_tx.send(ThumbnailResponse {
                                                        frame_idx,
                                                        width: active_width,
                                                        height: active_height,
                                                        rgba_data: scaled_frame.data(0).to_vec(),
                                                    });
                                                    frame_found = true;
                                                }
                                                break;
                                            }
                                        }
                                        if frame_found {
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        if aborted {
                            break;
                        }
                    }
                }
            }
        }
    });

    (cmd_tx, resp_rx, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_thumbnail_decoder_lifecycle() {
        let test_file = format!("{}/test-files/bbb_sunflower_2160p_60fps_normal.mp4", env!("CARGO_MANIFEST_DIR"));
        let keyframes = vec![0, 60, 120];
        let keyframes_pts = vec![0, 60 * 1000, 120 * 1000]; // Dummy pts to check seek conversion
        
        let (cmd_tx, resp_rx, handle) = spawn_thumbnail_decoder(test_file, keyframes, keyframes_pts, 60.0);
        
        // Request frames 0 and 60
        cmd_tx.send(ThumbnailCommand::Request { frames: vec![0, 60], height: 72 }).unwrap();
        
        // We expect to receive two thumbnail responses within 5 seconds
        let resp1 = resp_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(resp1.height, 72);
        assert!(resp1.rgba_data.len() > 0);
        
        let resp2 = resp_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(resp2.height, 72);
        
        // Cleanup
        drop(cmd_tx);
        let _ = handle.join();
    }
}
