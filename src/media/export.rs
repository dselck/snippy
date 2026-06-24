use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use ffmpeg_next as ffmpeg;
use crate::media::types::{ExportStatus, VideoMetadata};
use crate::ui::ExportRange;

#[derive(Clone, Copy, Debug, PartialEq)]
enum GopType {
    FullyKept,
    FullyCut,
    PartiallyKept,
}

pub fn export_video(
    input_path: &str,
    output_path: &str,
    ranges: Vec<ExportRange>,
    cut_mode: bool,
    metadata: VideoMetadata,
    status: Arc<Mutex<ExportStatus>>,
    cancel_flag: Arc<AtomicBool>,
) -> Result<(), anyhow::Error> {
    // 1. Group frames into contiguous kept segments
    let mut kept_segments: Vec<(usize, usize)> = Vec::new();
    let mut current_segment: Option<(usize, usize)> = None;

    for f in 0..metadata.total_frames {
        let is_cut = crate::ui::is_frame_cut_helper(f, &ranges, cut_mode);
        if !is_cut {
            if let Some(ref mut seg) = current_segment {
                seg.1 = f + 1;
            } else {
                current_segment = Some((f, f + 1));
            }
        } else if let Some(seg) = current_segment.take() {
            kept_segments.push(seg);
        }
    }
    if let Some(seg) = current_segment {
        kept_segments.push(seg);
    }

    if kept_segments.is_empty() {
        return Err(anyhow::anyhow!("No frames are selected for export."));
    }

    // Precalculate segment timings
    let fps = metadata.fps;
    let mut input_start_pts_sec = Vec::new();
    let mut output_start_pts_sec = Vec::new();

    // 2. Classify GOPs
    let mut gop_types = Vec::new();
    for i in 0..metadata.keyframes.len() {
        let start_frame = metadata.keyframes[i];
        let end_frame = if i + 1 < metadata.keyframes.len() {
            metadata.keyframes[i + 1]
        } else {
            metadata.total_frames
        };

        let mut kept_count = 0;
        let mut cut_count = 0;
        for f in start_frame..end_frame {
            if crate::ui::is_frame_cut_helper(f, &ranges, cut_mode) {
                cut_count += 1;
            } else {
                kept_count += 1;
            }
        }

        let gop_type = if kept_count == (end_frame - start_frame) {
            GopType::FullyKept
        } else if cut_count == (end_frame - start_frame) {
            GopType::FullyCut
        } else {
            GopType::PartiallyKept
        };
        gop_types.push(gop_type);
    }

    // Initialize FFmpeg
    ffmpeg::init()?;

    let mut input = ffmpeg::format::input(&input_path)?;
    let mut output = ffmpeg::format::output(&output_path)?;

    let mut stream_mapping = vec![-1isize; input.nb_streams() as usize];
    let mut out_stream_idx = 0;

    let mut video_in_stream_idx = None;
    let mut audio_in_stream_idx = None;

    if let Some(video_stream) = input.streams().best(ffmpeg::media::Type::Video) {
        video_in_stream_idx = Some(video_stream.index());
    }
    if let Some(audio_stream) = input.streams().best(ffmpeg::media::Type::Audio) {
        audio_in_stream_idx = Some(audio_stream.index());
    }

    for (ist_idx, ist) in input.streams().enumerate() {
        if Some(ist_idx) == video_in_stream_idx {
            stream_mapping[ist_idx] = out_stream_idx;
            let mut ost = output.add_stream(ffmpeg::encoder::find(ffmpeg::codec::Id::None))?;
            ost.set_parameters(ist.parameters());
            unsafe {
                (*ost.parameters().as_mut_ptr()).codec_tag = 0;
            }
            out_stream_idx += 1;
        } else if Some(ist_idx) == audio_in_stream_idx {
            stream_mapping[ist_idx] = out_stream_idx;
            let mut ost = output.add_stream(ffmpeg::encoder::find(ffmpeg::codec::Id::None))?;
            ost.set_parameters(ist.parameters());
            unsafe {
                (*ost.parameters().as_mut_ptr()).codec_tag = 0;
            }
            out_stream_idx += 1;
        }
    }

    output.write_header()?;

    // Copy stream timebases and calculate video_start_time to resolve A/V offsets
    let video_in_stream = input.stream(video_in_stream_idx.unwrap()).unwrap();
    let video_in_time_base = video_in_stream.time_base();
    let time_base_f64 = video_in_time_base.numerator() as f64 / video_in_time_base.denominator() as f64;
    
    let video_start_time = if video_in_stream.start_time() > -1_000_000_000_000 {
        video_in_stream.start_time() as f64 * time_base_f64
    } else {
        0.0
    };

    let mut current_output_sec = 0.0;
    for &(start_frame, end_frame) in &kept_segments {
        let mut actual_start_pts = start_frame as f64 / fps;
        if let Ok(idx) = metadata.keyframes.binary_search(&start_frame) {
            let kf_pts = metadata.keyframes_pts[idx];
            actual_start_pts = kf_pts as f64 * time_base_f64 - video_start_time;
        }
        input_start_pts_sec.push(actual_start_pts);
        output_start_pts_sec.push(current_output_sec);
        current_output_sec += (end_frame - start_frame) as f64 / fps;
    }

    let audio_in_time_base = audio_in_stream_idx.map(|idx| input.stream(idx).unwrap().time_base());

    let video_out_time_base = output.stream(stream_mapping[video_in_stream_idx.unwrap()] as usize).unwrap().time_base();
    let audio_out_time_base = audio_in_stream_idx.map(|idx| output.stream(stream_mapping[idx] as usize).unwrap().time_base());

    // Setup video transcoding context if there is any partially kept GOP
    let has_partially_kept = gop_types.iter().any(|&t| t == GopType::PartiallyKept);
    
    let mut video_decoder = if has_partially_kept && let Some(v_idx) = video_in_stream_idx {
        let video_in_stream = input.stream(v_idx).unwrap();
        let decoder = ffmpeg::codec::context::Context::from_parameters(video_in_stream.parameters())?
            .decoder()
            .video()?;
        Some(decoder)
    } else {
        None
    };

    let mut video_encoder = if has_partially_kept && let Some(ref decoder) = video_decoder {
        let codec_id = decoder.codec().unwrap().id();
        let encoder_codec = ffmpeg::encoder::find(codec_id)
            .or_else(|| {
                if codec_id == ffmpeg::codec::Id::H264 {
                    ffmpeg::encoder::find_by_name("libx264")
                } else if codec_id == ffmpeg::codec::Id::H265 {
                    ffmpeg::encoder::find_by_name("libx265")
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("No encoder found for codec {:?}", codec_id))?;
            
        let encoder_ctx = ffmpeg::codec::context::Context::new_with_codec(encoder_codec);
        let mut video_enc = encoder_ctx.encoder().video()?;
        
        video_enc.set_width(decoder.width());
        video_enc.set_height(decoder.height());
        video_enc.set_format(decoder.format());
        video_enc.set_time_base(video_in_time_base);
        video_enc.set_frame_rate(Some(input.stream(video_in_stream_idx.unwrap()).unwrap().rate()));
        video_enc.set_max_b_frames(0); // `/ponytail`: simplify PTS/DTS re-encoding math

        let mut opts = ffmpeg::Dictionary::new();
        if codec_id == ffmpeg::codec::Id::H264 {
            opts.set("crf", "21");
            opts.set("preset", "medium");
        } else if codec_id == ffmpeg::codec::Id::H265 {
            opts.set("crf", "24");
            opts.set("preset", "medium");
            opts.set("x265-params", "bframes=0");
        }
        
        Some(video_enc.open_with(opts)?)
    } else {
        None
    };

    // Tracking variables for progress & DTS monotonicity
    let mut video_packet_idx = 0;
    let mut last_video_dts: Option<i64> = None;
    let mut last_audio_dts: Option<i64> = None;

    let total_packets = metadata.total_frames; // approximation for progress scale

    for (stream, mut packet) in input.packets() {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(output_path);
            let mut status_guard = status.lock().unwrap();
            *status_guard = ExportStatus::Cancelled;
            return Ok(());
        }

        let ist_idx = stream.index();
        let ost_idx = stream_mapping[ist_idx];

        if ost_idx < 0 {
            continue;
        }

        let is_video = Some(ist_idx) == video_in_stream_idx;
        let is_audio = Some(ist_idx) == audio_in_stream_idx;

        if is_video {
            // Calculate frame_idx based on PTS relative to video_start_time
            let time_base_f64 = video_in_time_base.numerator() as f64 / video_in_time_base.denominator() as f64;
            let pts_sec = packet.pts().unwrap_or(0) as f64 * time_base_f64 - video_start_time;
            let frame_idx = (pts_sec * fps).round() as usize;

            let gop_idx = match metadata.keyframes.binary_search(&frame_idx) {
                Ok(i) => i,
                Err(0) => 0,
                Err(i) => i - 1,
            };
            let gop_type = gop_types[gop_idx];

            match gop_type {
                GopType::FullyKept => {
                    // Find segment
                    if let Some(seg_idx) = kept_segments.iter().position(|&(start, end)| frame_idx >= start && frame_idx < end) {
                        let offset_sec = output_start_pts_sec[seg_idx] - input_start_pts_sec[seg_idx];
                        let total_offset_sec = -video_start_time + offset_sec;
                        let offset_ticks_in = (total_offset_sec / time_base_f64).round() as i64;
                        
                        if let Some(pts) = packet.pts() {
                            packet.set_pts(Some(pts + offset_ticks_in));
                        }
                        if let Some(dts) = packet.dts() {
                            let mut adjusted_dts = dts + offset_ticks_in;
                            if let Some(last_dts) = last_video_dts {
                                if adjusted_dts <= last_dts {
                                    adjusted_dts = last_dts + 1;
                                }
                            }
                            packet.set_dts(Some(adjusted_dts));
                            last_video_dts = packet.dts();
                        }
                        if let Some(pts) = packet.pts() {
                            if let Some(dts) = packet.dts() {
                                if pts < dts {
                                    packet.set_pts(Some(dts));
                                }
                            }
                        }

                        packet.rescale_ts(video_in_time_base, video_out_time_base);
                        packet.set_stream(ost_idx as usize);
                        packet.write_interleaved(&mut output)?;
                    }
                }
                GopType::FullyCut => {}
                GopType::PartiallyKept => {
                    if let Some(ref mut decoder) = video_decoder {
                        decoder.send_packet(&packet)?;
                        
                        let mut decoded_frame = ffmpeg::util::frame::Video::empty();
                        while decoder.receive_frame(&mut decoded_frame).is_ok() {
                            let f_pts = decoded_frame.pts().unwrap_or(0);
                            let f_pts_sec = f_pts as f64 * time_base_f64 - video_start_time;
                            let f_idx = (f_pts_sec * fps).round() as usize;

                            if !crate::ui::is_frame_cut_helper(f_idx, &ranges, cut_mode) {
                                if let Some(seg_idx) = kept_segments.iter().position(|&(start, end)| f_idx >= start && f_idx < end) {
                                    let start_frame = kept_segments[seg_idx].0;
                                    let out_pts_sec = (f_idx - start_frame) as f64 / fps + output_start_pts_sec[seg_idx];
                                    let encoder = video_encoder.as_mut().unwrap();
                                    let enc_pts = (out_pts_sec / (encoder.time_base().numerator() as f64 / encoder.time_base().denominator() as f64)).round() as i64;
                                    
                                    decoded_frame.set_pts(Some(enc_pts));
                                    unsafe {
                                        (*decoded_frame.as_mut_ptr()).pict_type = ffmpeg::sys::AVPictureType::AV_PICTURE_TYPE_NONE;
                                    }
                                    encoder.send_frame(&decoded_frame)?;

                                    let mut out_pkt = ffmpeg::Packet::empty();
                                    while encoder.receive_packet(&mut out_pkt).is_ok() {
                                        out_pkt.rescale_ts(encoder.time_base(), video_out_time_base);
                                        
                                        if let Some(dts) = out_pkt.dts() {
                                            let mut adjusted_dts = dts;
                                            if let Some(last_dts) = last_video_dts {
                                                if adjusted_dts <= last_dts {
                                                    adjusted_dts = last_dts + 1;
                                                }
                                            }
                                            out_pkt.set_dts(Some(adjusted_dts));
                                            last_video_dts = out_pkt.dts();
                                        }
                                        if let Some(pts) = out_pkt.pts() {
                                            if let Some(dts) = out_pkt.dts() {
                                                if pts < dts {
                                                    out_pkt.set_pts(Some(dts));
                                                }
                                            }
                                        }

                                        out_pkt.set_stream(ost_idx as usize);
                                        out_pkt.write_interleaved(&mut output)?;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            video_packet_idx += 1;
            
            // Update status periodically
            if video_packet_idx % 30 == 0 {
                let progress = (video_packet_idx as f32 / total_packets as f32).min(0.99);
                let mut status_guard = status.lock().unwrap();
                *status_guard = ExportStatus::Exporting {
                    progress,
                    message: format!("Processing frame {}/{}...", video_packet_idx, total_packets),
                };
            }
        } else if is_audio {
            let audio_in_tb = audio_in_time_base.unwrap();
            let audio_out_tb = audio_out_time_base.unwrap();
            let time_base_f64 = audio_in_tb.numerator() as f64 / audio_in_tb.denominator() as f64;
            
            let pts_sec = packet.pts().unwrap_or(0) as f64 * time_base_f64 - video_start_time;

            // Check if audio fits into any kept video segment
            let mut found_seg = None;
            for (seg_idx, &(start_frame, end_frame)) in kept_segments.iter().enumerate() {
                let start_sec = input_start_pts_sec[seg_idx];
                let end_sec = start_sec + (end_frame - start_frame) as f64 / fps;
                if pts_sec >= start_sec && pts_sec < end_sec {
                    found_seg = Some(seg_idx);
                    break;
                }
            }

            if let Some(seg_idx) = found_seg {
                let offset_sec = output_start_pts_sec[seg_idx] - input_start_pts_sec[seg_idx];
                let total_offset_sec = -video_start_time + offset_sec;
                let offset_ticks_in = (total_offset_sec / time_base_f64).round() as i64;

                if let Some(pts) = packet.pts() {
                    packet.set_pts(Some(pts + offset_ticks_in));
                }
                if let Some(dts) = packet.dts() {
                    let mut adjusted_dts = dts + offset_ticks_in;
                    if let Some(last_dts) = last_audio_dts {
                        if adjusted_dts <= last_dts {
                            adjusted_dts = last_dts + 1;
                        }
                    }
                    packet.set_dts(Some(adjusted_dts));
                    last_audio_dts = packet.dts();
                }
                if let Some(pts) = packet.pts() {
                    if let Some(dts) = packet.dts() {
                        if pts < dts {
                            packet.set_pts(Some(dts));
                        }
                    }
                }

                packet.rescale_ts(audio_in_tb, audio_out_tb);
                packet.set_stream(ost_idx as usize);
                packet.write_interleaved(&mut output)?;
            }
        }
    }

    // Flush decoder & encoder at the end of file
    if let Some(ref mut decoder) = video_decoder {
        decoder.send_packet(&ffmpeg::Packet::empty())?;
        let mut decoded_frame = ffmpeg::util::frame::Video::empty();
        while decoder.receive_frame(&mut decoded_frame).is_ok() {
            let f_pts = decoded_frame.pts().unwrap_or(0);
            let time_base_f64 = video_in_time_base.numerator() as f64 / video_in_time_base.denominator() as f64;
            let f_pts_sec = f_pts as f64 * time_base_f64 - video_start_time;
            let f_idx = (f_pts_sec * fps).round() as usize;

            if !crate::ui::is_frame_cut_helper(f_idx, &ranges, cut_mode) {
                if let Some(seg_idx) = kept_segments.iter().position(|&(start, end)| f_idx >= start && f_idx < end) {
                    let start_frame = kept_segments[seg_idx].0;
                    let out_pts_sec = (f_idx - start_frame) as f64 / fps + output_start_pts_sec[seg_idx];
                    let encoder = video_encoder.as_mut().unwrap();
                    let enc_pts = (out_pts_sec / (encoder.time_base().numerator() as f64 / encoder.time_base().denominator() as f64)).round() as i64;
                    
                    decoded_frame.set_pts(Some(enc_pts));
                    unsafe {
                        (*decoded_frame.as_mut_ptr()).pict_type = ffmpeg::sys::AVPictureType::AV_PICTURE_TYPE_NONE;
                    }
                    encoder.send_frame(&decoded_frame)?;

                    let mut out_pkt = ffmpeg::Packet::empty();
                    while encoder.receive_packet(&mut out_pkt).is_ok() {
                        out_pkt.rescale_ts(encoder.time_base(), video_out_time_base);
                        
                        if let Some(dts) = out_pkt.dts() {
                            let mut adjusted_dts = dts;
                            if let Some(last_dts) = last_video_dts {
                                if adjusted_dts <= last_dts {
                                    adjusted_dts = last_dts + 1;
                                }
                            }
                            out_pkt.set_dts(Some(adjusted_dts));
                            last_video_dts = out_pkt.dts();
                        }
                        if let Some(pts) = out_pkt.pts() {
                            if let Some(dts) = out_pkt.dts() {
                                if pts < dts {
                                    out_pkt.set_pts(Some(dts));
                                }
                            }
                        }

                        out_pkt.set_stream(stream_mapping[video_in_stream_idx.unwrap()] as usize);
                        out_pkt.write_interleaved(&mut output)?;
                    }
                }
            }
        }
    }

    // Flush encoder
    if let Some(ref mut encoder) = video_encoder {
        encoder.send_eof()?;
        let mut out_pkt = ffmpeg::Packet::empty();
        while encoder.receive_packet(&mut out_pkt).is_ok() {
            out_pkt.rescale_ts(encoder.time_base(), video_out_time_base);
            
            if let Some(dts) = out_pkt.dts() {
                let mut adjusted_dts = dts;
                if let Some(last_dts) = last_video_dts {
                    if adjusted_dts <= last_dts {
                        adjusted_dts = last_dts + 1;
                    }
                }
                out_pkt.set_dts(Some(adjusted_dts));
                last_video_dts = out_pkt.dts();
            }
            if let Some(pts) = out_pkt.pts() {
                if let Some(dts) = out_pkt.dts() {
                    if pts < dts {
                        out_pkt.set_pts(Some(dts));
                    }
                }
            }

            out_pkt.set_stream(stream_mapping[video_in_stream_idx.unwrap()] as usize);
            out_pkt.write_interleaved(&mut output)?;
        }
    }

    output.write_trailer()?;

    let mut status_guard = status.lock().unwrap();
    *status_guard = ExportStatus::Completed;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gop_classification() {
        let keyframes = vec![0, 10, 20];
        let total_frames = 30;
        let metadata = VideoMetadata {
            total_frames,
            width: 640,
            height: 480,
            fps: 30.0,
            has_audio: false,
            keyframes,
            keyframes_pts: vec![0, 10, 20],
        };

        // Scenario 1: cut_mode = false (keep ranges). Keep range = [0, 9] (GOP 0 kept, GOP 1 and 2 cut)
        let ranges = vec![ExportRange { id: 1, in_frame: 0, out_frame: 9 }];
        
        let classify = |ranges: &[ExportRange]| {
            let mut gops = Vec::new();
            for i in 0..metadata.keyframes.len() {
                let start_frame = metadata.keyframes[i];
                let end_frame = if i + 1 < metadata.keyframes.len() {
                    metadata.keyframes[i + 1]
                } else {
                    metadata.total_frames
                };

                let mut kept_count = 0;
                let mut cut_count = 0;
                for f in start_frame..end_frame {
                    if crate::ui::is_frame_cut_helper(f, ranges, false) {
                        cut_count += 1;
                    } else {
                        kept_count += 1;
                    }
                }

                let gop_type = if kept_count == (end_frame - start_frame) {
                    GopType::FullyKept
                } else if cut_count == (end_frame - start_frame) {
                    GopType::FullyCut
                } else {
                    GopType::PartiallyKept
                };
                gops.push(gop_type);
            }
            gops
        };

        assert_eq!(classify(&ranges), vec![GopType::FullyKept, GopType::FullyCut, GopType::FullyCut]);

        // Scenario 2: Keep range = [5, 14]. GOP 0 is PartiallyKept, GOP 1 is PartiallyKept, GOP 2 is FullyCut
        let ranges2 = vec![ExportRange { id: 1, in_frame: 5, out_frame: 14 }];
        assert_eq!(classify(&ranges2), vec![GopType::PartiallyKept, GopType::PartiallyKept, GopType::FullyCut]);
    }
}
