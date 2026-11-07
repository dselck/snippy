use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use ffmpeg_next as ffmpeg;
use crate::media::types::{ExportStatus, VideoMetadata};
use crate::ui::ExportRange;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GopType {
    Copy,
    Transcode,
    Cut,
}

fn create_video_encoder(
    decoder: &ffmpeg::codec::decoder::Video,
    video_in_time_base: ffmpeg::Rational,
    video_in_stream_rate: ffmpeg::Rational,
) -> Result<ffmpeg::codec::encoder::Video, anyhow::Error> {
    let codec_id = decoder.codec().unwrap().id();
    let encoder_codec = ffmpeg::encoder::find(codec_id)
        .or_else(|| {
            if codec_id == ffmpeg::codec::Id::H264 {
                ffmpeg::encoder::find_by_name("libx264")
            } else if codec_id == ffmpeg::codec::Id::H265 || codec_id == ffmpeg::codec::Id::HEVC {
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
    video_enc.set_frame_rate(Some(video_in_stream_rate));
    let max_b_frames = 0;
    video_enc.set_max_b_frames(max_b_frames); // `/ponytail`: simplify PTS/DTS re-encoding math

    // We do NOT set GLOBAL_HEADER here. This forces the encoder to write VPS, SPS, and PPS
    // headers in-band at the start of keyframes. This is critical for smart rendering
    // because it allows the decoder to switch parameters between copied and transcoded segments.
    // The resulting Annex B packets will be converted to HVCC format by write_video_packet.

    let mut opts = ffmpeg::Dictionary::new();
    if codec_id == ffmpeg::codec::Id::H264 {
        opts.set("crf", "21");
        opts.set("preset", "medium");
        // Force libx264 to repeat headers in-band so that SPS/PPS are written on every keyframe,
        // allowing decoders to recover when transitioning from transcoded to copied segments.
        opts.set("x264-params", "repeat-headers=1");
    } else if codec_id == ffmpeg::codec::Id::H265 {
        opts.set("crf", "24");
        opts.set("preset", "medium");
        // Force libx265 to repeat headers in-band at every keyframe.
        opts.set("x265-params", &format!("bframes={}:repeat-headers=1", max_b_frames));
    }
    
    Ok(video_enc.open_with(opts)?)
}

fn annex_b_to_hvcc(annex_b_data: &[u8]) -> Vec<u8> {
    let mut hvcc_data = Vec::new();
    let mut pos = 0;
    let n = annex_b_data.len();
    
    // Find all start code positions and lengths
    let mut start_codes = Vec::new();
    while pos < n {
        if pos + 2 < n && annex_b_data[pos..pos+3] == [0, 0, 1] {
            let mut start = pos;
            let mut len = 3;
            // check if there is a leading zero
            if pos > 0 && annex_b_data[pos - 1] == 0 {
                start = pos - 1;
                len = 4;
            }
            start_codes.push((start, len));
            pos += 3;
        } else {
            pos += 1;
        }
    }
    
    if start_codes.is_empty() {
        return annex_b_data.to_vec();
    }
    
    // Now extract NAL units
    for i in 0..start_codes.len() {
        let start = start_codes[i].0 + start_codes[i].1;
        let end = if i + 1 < start_codes.len() {
            start_codes[i + 1].0
        } else {
            n
        };
        
        // Strip trailing zeros from the NAL unit (Annex B trailing zero padding)
        let mut nal_end = end;
        while nal_end > start && annex_b_data[nal_end - 1] == 0 {
            nal_end -= 1;
        }
        
        let nal_len = nal_end - start;
        if nal_len > 0 {
            hvcc_data.extend_from_slice(&(nal_len as u32).to_be_bytes());
            hvcc_data.extend_from_slice(&annex_b_data[start..nal_end]);
        }
    }
    
    hvcc_data
}

fn extract_parameter_sets(extradata: &[u8], codec_id: ffmpeg::codec::Id) -> Vec<u8> {
    let mut out = Vec::new();
    
    if codec_id == ffmpeg::codec::Id::H264 {
        if extradata.len() < 6 { return out; }
        let num_sps = (extradata[5] & 0x1F) as usize;
        let mut pos = 6;
        
        for _ in 0..num_sps {
            if pos + 2 > extradata.len() { return out; }
            let len = u16::from_be_bytes([extradata[pos], extradata[pos+1]]) as usize;
            pos += 2;
            if pos + len > extradata.len() { return out; }
            out.extend_from_slice(&(len as u32).to_be_bytes());
            out.extend_from_slice(&extradata[pos..pos+len]);
            pos += len;
        }
        
        if pos + 1 > extradata.len() { return out; }
        let num_pps = extradata[pos] as usize;
        pos += 1;
        
        for _ in 0..num_pps {
            if pos + 2 > extradata.len() { return out; }
            let len = u16::from_be_bytes([extradata[pos], extradata[pos+1]]) as usize;
            pos += 2;
            if pos + len > extradata.len() { return out; }
            out.extend_from_slice(&(len as u32).to_be_bytes());
            out.extend_from_slice(&extradata[pos..pos+len]);
            pos += len;
        }
    } else if codec_id == ffmpeg::codec::Id::H265 || codec_id == ffmpeg::codec::Id::HEVC {
        if extradata.len() < 23 { return out; }
        let num_arrays = extradata[22] as usize;
        let mut pos = 23;
        
        for _ in 0..num_arrays {
            if pos + 3 > extradata.len() { return out; }
            let num_nalus = u16::from_be_bytes([extradata[pos+1], extradata[pos+2]]) as usize;
            pos += 3;
            
            for _ in 0..num_nalus {
                if pos + 2 > extradata.len() { return out; }
                let len = u16::from_be_bytes([extradata[pos], extradata[pos+1]]) as usize;
                pos += 2;
                if pos + len > extradata.len() { return out; }
                out.extend_from_slice(&(len as u32).to_be_bytes());
                out.extend_from_slice(&extradata[pos..pos+len]);
                pos += len;
            }
        }
    }
    
    out
}

fn write_copied_video_packet(
    mut packet: ffmpeg::Packet,
    video_codec_id: ffmpeg::codec::Id,
    original_extradata: Option<&[u8]>,
    ost_idx: usize,
    output: &mut ffmpeg::format::context::Output,
) -> Result<(), anyhow::Error> {
    if packet.is_key() {
        println!("write_copied_video_packet: packet is a keyframe! codec_id={:?}", video_codec_id);
        if let Some(extradata) = original_extradata {
            let param_sets = extract_parameter_sets(extradata, video_codec_id);
            println!("write_copied_video_packet: extracted param_sets len={}", param_sets.len());
            if !param_sets.is_empty() && packet.data().is_some() {
                let original_data = packet.data().unwrap();
                let mut new_pkt = ffmpeg::Packet::new(param_sets.len() + original_data.len());
                new_pkt.data_mut().unwrap()[..param_sets.len()].copy_from_slice(&param_sets);
                new_pkt.data_mut().unwrap()[param_sets.len()..].copy_from_slice(original_data);
                
                new_pkt.set_pts(packet.pts());
                new_pkt.set_dts(packet.dts());
                new_pkt.set_duration(packet.duration());
                new_pkt.set_flags(packet.flags());
                packet = new_pkt;
                println!("write_copied_video_packet: prepended param sets! new size={}", packet.data().unwrap().len());
            }
        }
    }
    
    packet.set_stream(ost_idx);
    packet.write_interleaved(output)?;
    Ok(())
}

fn is_annex_b(data: &[u8]) -> bool {
    if data.len() >= 3 && data[0..3] == [0, 0, 1] {
        true
    } else if data.len() >= 4 && data[0..4] == [0, 0, 0, 1] {
        true
    } else {
        false
    }
}

fn write_video_packet(
    out_pkt: ffmpeg::Packet,
    video_codec_id: ffmpeg::codec::Id,
    last_video_dts: &mut Option<i64>,
    ost_idx: usize,
    output: &mut ffmpeg::format::context::Output,
) -> Result<(), anyhow::Error> {
    let mut final_pkt = out_pkt;
    
    if (video_codec_id == ffmpeg::codec::Id::H264 || video_codec_id == ffmpeg::codec::Id::H265 || video_codec_id == ffmpeg::codec::Id::HEVC) && final_pkt.data().is_some() {
        let data = final_pkt.data().unwrap();
        if is_annex_b(data) {
            let converted_data = annex_b_to_hvcc(data);
            let mut new_pkt = ffmpeg::Packet::new(converted_data.len());
            new_pkt.data_mut().unwrap().copy_from_slice(&converted_data);
            new_pkt.set_pts(final_pkt.pts());
            new_pkt.set_dts(final_pkt.dts());
            new_pkt.set_duration(final_pkt.duration());
            new_pkt.set_flags(final_pkt.flags());
            final_pkt = new_pkt;
        }
    }
    
    if let Some(dts) = final_pkt.dts() {
        let mut adjusted_dts = dts;
        if let Some(last_dts) = *last_video_dts {
            if adjusted_dts <= last_dts {
                adjusted_dts = last_dts + 1;
            }
        }
        final_pkt.set_dts(Some(adjusted_dts));
        *last_video_dts = final_pkt.dts();
    }
    if let Some(pts) = final_pkt.pts() {
        if let Some(dts) = final_pkt.dts() {
            if pts < dts {
                final_pkt.set_pts(Some(dts));
            }
        }
    }
    
    final_pkt.set_stream(ost_idx);
    final_pkt.write_interleaved(output)?;
    Ok(())
}

fn flush_and_reset_video_pipeline(
    decoder: &mut Option<ffmpeg::codec::decoder::Video>,
    encoder: &mut Option<ffmpeg::codec::encoder::Video>,
    output: &mut ffmpeg::format::context::Output,
    stream_mapping: &[isize],
    video_in_stream_idx: usize,
    video_codec_id: ffmpeg::codec::Id,
    video_in_time_base: ffmpeg::Rational,
    video_out_time_base: ffmpeg::Rational,
    last_video_dts: &mut Option<i64>,
    kept_segments: &[(usize, usize)],
    ranges: &[ExportRange],
    cut_mode: bool,
    fps: f64,
    output_start_pts_sec: &[f64],
    video_start_time: f64,
) -> Result<(), anyhow::Error> {
    let decoder = match decoder.as_mut() {
        Some(d) => d,
        None => return Ok(()),
    };
    
    // 1. Send empty packet to decoder to flush it
    decoder.send_packet(&ffmpeg::Packet::empty())?;
    
    let mut decoded_frame = ffmpeg::util::frame::Video::empty();
    while decoder.receive_frame(&mut decoded_frame).is_ok() {
        let f_pts = decoded_frame.pts().unwrap_or(0);
        let time_base_f64 = video_in_time_base.numerator() as f64 / video_in_time_base.denominator() as f64;
        let f_pts_sec = f_pts as f64 * time_base_f64 - video_start_time;
        let f_idx = (f_pts_sec * fps).round() as usize;

        if !crate::ui::is_frame_cut_helper(f_idx, ranges, cut_mode) {
            if let Some(seg_idx) = kept_segments.iter().position(|&(start, end)| f_idx >= start && f_idx < end) {
                let start_frame = kept_segments[seg_idx].0;
                let out_pts_sec = (f_idx - start_frame) as f64 / fps + output_start_pts_sec[seg_idx];
                
                if let Some(enc) = encoder {
                    let enc_pts = (out_pts_sec / (enc.time_base().numerator() as f64 / enc.time_base().denominator() as f64)).round() as i64;
                    decoded_frame.set_pts(Some(enc_pts));
                    unsafe {
                        (*decoded_frame.as_mut_ptr()).pict_type = ffmpeg::sys::AVPictureType::AV_PICTURE_TYPE_NONE;
                    }
                    enc.send_frame(&decoded_frame)?;

                    let mut out_pkt = ffmpeg::Packet::empty();
                    while enc.receive_packet(&mut out_pkt).is_ok() {
                        out_pkt.rescale_ts(enc.time_base(), video_out_time_base);
                        write_video_packet(
                            out_pkt,
                            video_codec_id,
                            last_video_dts,
                            stream_mapping[video_in_stream_idx] as usize,
                            output,
                        )?;
                        out_pkt = ffmpeg::Packet::empty();
                    }
                }
            }
        }
    }

    // 2. Send EOF to encoder to flush it
    if let Some(enc) = encoder {
        enc.send_eof()?;
        let mut out_pkt = ffmpeg::Packet::empty();
        while enc.receive_packet(&mut out_pkt).is_ok() {
            out_pkt.rescale_ts(enc.time_base(), video_out_time_base);
            write_video_packet(
                out_pkt,
                video_codec_id,
                last_video_dts,
                stream_mapping[video_in_stream_idx] as usize,
                output,
            )?;
            out_pkt = ffmpeg::Packet::empty();
        }
    }

    // 3. Clear/reset decoder & drop encoder
    decoder.flush();
    *encoder = None;

    Ok(())
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
            GopType::Copy
        } else if cut_count == (end_frame - start_frame) {
            GopType::Cut
        } else {
            GopType::Transcode
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
    let video_codec_id = video_in_stream.parameters().id();
    let original_extradata = unsafe {
        let raw = video_in_stream.parameters().as_ptr();
        if !(*raw).extradata.is_null() && (*raw).extradata_size > 0 {
            Some(std::slice::from_raw_parts((*raw).extradata, (*raw).extradata_size as usize))
        } else {
            None
        }
    };
    let video_in_time_base = video_in_stream.time_base();
    let time_base_f64 = video_in_time_base.numerator() as f64 / video_in_time_base.denominator() as f64;
    
    let video_start_time = if video_in_stream.start_time() > -1_000_000_000_000 {
        video_in_stream.start_time() as f64 * time_base_f64
    } else {
        0.0
    };

    let mut current_output_sec = 0.0;
    for &(start_frame, end_frame) in &kept_segments {
        let nominal_start = start_frame as f64 / fps;
        let mut actual_start_pts = nominal_start;
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
    let has_transcode = gop_types.iter().any(|&t| t == GopType::Transcode);
    
    let mut video_decoder = if has_transcode && let Some(v_idx) = video_in_stream_idx {
        let video_in_stream = input.stream(v_idx).unwrap();
        let decoder = ffmpeg::codec::context::Context::from_parameters(video_in_stream.parameters())?
            .decoder()
            .video()?;
        Some(decoder)
    } else {
        None
    };

    let mut video_encoder: Option<ffmpeg::codec::encoder::Video> = None;

    // Tracking variables for progress & DTS monotonicity & transitions
    let mut total_exported_frames = 0;
    for &(start, end) in &kept_segments {
        total_exported_frames += end - start;
    }
    let total_exported_duration_sec = total_exported_frames as f64 / fps;

    let mut max_written_pts_sec = 0.0f64;
    let get_progress = |max_pts: f64| -> f32 {
        if total_exported_duration_sec <= 0.0 {
            1.0
        } else {
            (max_pts / total_exported_duration_sec).clamp(0.0, 0.99) as f32
        }
    };

    let mut video_packet_idx = 0;
    let mut last_video_dts: Option<i64> = None;
    let mut last_audio_dts: Option<i64> = None;
    let mut last_gop_type: Option<GopType> = None;

    let video_in_stream_rate = video_in_stream_idx.map(|idx| input.stream(idx).unwrap().rate()).unwrap_or(ffmpeg::Rational(24, 1));
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

            // Check if we transitioned from Transcode to another type, which requires flushing the encoder
            if let Some(last) = last_gop_type {
                if last == GopType::Transcode && gop_type != GopType::Transcode {
                    flush_and_reset_video_pipeline(
                        &mut video_decoder,
                        &mut video_encoder,
                        &mut output,
                        &stream_mapping,
                        video_in_stream_idx.unwrap(),
                        video_codec_id,
                        video_in_time_base,
                        video_out_time_base,
                        &mut last_video_dts,
                        &kept_segments,
                        &ranges,
                        cut_mode,
                        fps,
                        &output_start_pts_sec,
                        video_start_time,
                    )?;
                }
            }
            last_gop_type = Some(gop_type);

            match gop_type {
                GopType::Copy => {
                    // Find segment
                    if let Some(seg_idx) = kept_segments.iter().position(|&(start, end)| frame_idx >= start && frame_idx < end) {
                        let offset_sec = output_start_pts_sec[seg_idx] - input_start_pts_sec[seg_idx];
                        let out_pts_sec = pts_sec + offset_sec;
                        max_written_pts_sec = max_written_pts_sec.max(out_pts_sec);

                        packet.rescale_ts(video_in_time_base, video_out_time_base);

                        let total_offset_sec = -video_start_time + offset_sec;
                        let tb_out_f64 = video_out_time_base.numerator() as f64 / video_out_time_base.denominator() as f64;
                        let offset_ticks_out = (total_offset_sec / tb_out_f64).round() as i64;
                        
                        if let Some(pts) = packet.pts() {
                            packet.set_pts(Some(pts + offset_ticks_out));
                        }
                        if let Some(dts) = packet.dts() {
                            let mut adjusted_dts = dts + offset_ticks_out;
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

                        write_copied_video_packet(
                            packet,
                            video_codec_id,
                            original_extradata,
                            ost_idx as usize,
                            &mut output,
                        )?;

                        if video_packet_idx % 30 == 0 {
                            let (start_frame, end_frame) = kept_segments[seg_idx];
                            let current_time_in_sec = (pts_sec - input_start_pts_sec[seg_idx]).max(0.0);
                            let total_time_in_sec = (end_frame - start_frame) as f64 / fps;
                            let clamped_current_time = current_time_in_sec.min(total_time_in_sec);
                            let msg = format!(
                                "Copying {} of {} from section {}",
                                format_duration(clamped_current_time),
                                format_duration(total_time_in_sec),
                                seg_idx + 1
                            );
                            let progress = get_progress(max_written_pts_sec);
                            let mut status_guard = status.lock().unwrap();
                            *status_guard = ExportStatus::Exporting {
                                progress,
                                message: msg,
                                is_encoding: false,
                                current_frame: frame_idx,
                            };
                        }
                    }
                }
                GopType::Cut => {}
                GopType::Transcode => {
                    if let Some(ref mut decoder) = video_decoder {
                        if video_encoder.is_none() {
                            video_encoder = Some(create_video_encoder(
                                decoder,
                                video_in_time_base,
                                video_in_stream_rate,
                            )?);
                        }
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
                                    max_written_pts_sec = max_written_pts_sec.max(out_pts_sec);

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
                                        write_video_packet(
                                            out_pkt,
                                            video_codec_id,
                                            &mut last_video_dts,
                                            ost_idx as usize,
                                            &mut output,
                                        )?;
                                        out_pkt = ffmpeg::Packet::empty();
                                    }

                                    // Update encoding status for every frame
                                    let gop_idx = match metadata.keyframes.binary_search(&f_idx) {
                                        Ok(i) => i,
                                        Err(0) => 0,
                                        Err(i) => i - 1,
                                    };
                                    let gop_start = metadata.keyframes[gop_idx];
                                    let gop_end = if gop_idx + 1 < metadata.keyframes.len() {
                                        metadata.keyframes[gop_idx + 1]
                                    } else {
                                        metadata.total_frames
                                    };
                                    let mut total_gop_transcode_frames = 0;
                                    for f in gop_start..gop_end {
                                        if !crate::ui::is_frame_cut_helper(f, &ranges, cut_mode) {
                                            total_gop_transcode_frames += 1;
                                        }
                                    }
                                    let mut current_gop_transcode_idx = 0;
                                    for f in gop_start..=f_idx {
                                        if !crate::ui::is_frame_cut_helper(f, &ranges, cut_mode) {
                                            current_gop_transcode_idx += 1;
                                        }
                                    }
                                    let msg = format!(
                                        "Encoding frame {} of {} in GOP {}/{}",
                                        current_gop_transcode_idx,
                                        total_gop_transcode_frames,
                                        gop_idx + 1,
                                        metadata.keyframes.len()
                                    );
                                    let progress = get_progress(max_written_pts_sec);
                                    let mut status_guard = status.lock().unwrap();
                                    *status_guard = ExportStatus::Exporting {
                                        progress,
                                        message: msg,
                                        is_encoding: true,
                                        current_frame: f_idx,
                                    };
                                }
                            }
                        }
                    }
                }
            }

            video_packet_idx += 1;
            
            // Update progress value periodically, preserving existing message & mode
            if video_packet_idx % 30 == 0 {
                let progress = get_progress(max_written_pts_sec);
                let mut status_guard = status.lock().unwrap();
                if let ExportStatus::Exporting { message, is_encoding, .. } = &*status_guard {
                    *status_guard = ExportStatus::Exporting {
                        progress,
                        message: message.clone(),
                        is_encoding: *is_encoding,
                        current_frame: frame_idx,
                    };
                } else {
                    *status_guard = ExportStatus::Exporting {
                        progress,
                        message: format!("Processing frame {}/{}...", video_packet_idx, total_packets),
                        is_encoding: false,
                        current_frame: frame_idx,
                    };
                }
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
    flush_and_reset_video_pipeline(
        &mut video_decoder,
        &mut video_encoder,
        &mut output,
        &stream_mapping,
        video_in_stream_idx.unwrap(),
        video_codec_id,
        video_in_time_base,
        video_out_time_base,
        &mut last_video_dts,
        &kept_segments,
        &ranges,
        cut_mode,
        fps,
        &output_start_pts_sec,
        video_start_time,
    )?;

    output.write_trailer()?;

    let mut status_guard = status.lock().unwrap();
    *status_guard = ExportStatus::Completed;

    Ok(())
}

fn format_duration(seconds: f64) -> String {
    let secs = seconds.max(0.0);
    let minutes = (secs / 60.0).floor() as u32;
    let remaining_secs = secs % 60.0;
    format!("{:02}:{:06.3}", minutes, remaining_secs)
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
                    GopType::Copy
                } else if cut_count == (end_frame - start_frame) {
                    GopType::Cut
                } else {
                    GopType::Transcode
                };
                gops.push(gop_type);
            }
            gops
        };

        assert_eq!(classify(&ranges), vec![GopType::Copy, GopType::Cut, GopType::Cut]);

        // Scenario 2: Keep range = [5, 14]. GOP 0 is PartiallyKept (Transcode), GOP 1 is PartiallyKept (Transcode), GOP 2 is FullyCut (Cut)
        let ranges2 = vec![ExportRange { id: 1, in_frame: 5, out_frame: 14 }];
        assert_eq!(classify(&ranges2), vec![GopType::Transcode, GopType::Transcode, GopType::Cut]);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0.0), "00:00.000");
        assert_eq!(format_duration(5.1234), "00:05.123");
        assert_eq!(format_duration(62.3456), "01:02.346"); // rounded
        assert_eq!(format_duration(-10.0), "00:00.000");
    }

    /// [DEBUG-exp1] Integration test: export a fixture video with ranges that produce
    /// both PartiallyKept and FullyKept GOPs, then validate output PTS continuity.
    ///
    /// Fixture: test-files/fixture_hevc_bframes.mkv
    ///   - 240 frames, 24000/1001 fps, HEVC Main10, has_b_frames=2
    ///   - Keyframes at PTS: 0, 2002, 4004, 6006, 8008 (ms)
    ///   - Keyframes at packet indices: 0, 46, 94, 142, 190
    ///   - Keyframes at display-order frame indices: 0, 48, 96, 144, 192
    ///
    /// Keep ranges (cut_mode=false):
    ///   - Range 1: frames 5..55 (starts mid-GOP0, ends mid-GOP1 → Partial, Partial)
    ///   - Range 2: frames 96..191 (starts on GOP2, ends at end of GOP3 → Fully, Fully)
    ///
    /// This should produce:
    ///   - Segment 1: frames 5..55 (51 frames), with GOP0 partial + GOP1 partial
    ///   - Segment 2: frames 96..191 (96 frames), with GOP2 fully + GOP3 fully
    ///
    /// Total output: 147 frames ≈ 6.131s
    #[test]
    fn test_export_pts_continuity() {
        let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/test-files/fixture_hevc_bframes.mkv");

        // Verify fixture exists
        assert!(std::path::Path::new(fixture_path).exists(),
            "Fixture not found at {}. Run the fixture generation script first.", fixture_path);

        let output_path = concat!(env!("CARGO_MANIFEST_DIR"), "/test-files/test_export_output.mkv");
        // Clean up any previous output
        let _ = std::fs::remove_file(output_path);

        // Build metadata matching what the fixed MediaEngine scanner produces.
        // After fix: keyframes are display-order frame indices derived from PTS.
        let metadata = VideoMetadata {
            total_frames: 240,
            width: 320,
            height: 240,
            fps: 24000.0 / 1001.0,
            has_audio: true,
            keyframes: vec![0, 48, 96, 144, 192], // display-order frame indices (fixed)
            keyframes_pts: vec![0, 2002, 4004, 6006, 8008], // PTS in stream timebase (1/1000)
        };

        // Keep ranges: two disjoint sections producing both Partial and Fully kept GOPs
        let ranges = vec![
            ExportRange { id: 1, in_frame: 5, out_frame: 55 },   // mid-GOP0 through mid-GOP1
            ExportRange { id: 2, in_frame: 96, out_frame: 191 },  // full GOP2 + full GOP3
        ];

        let status = Arc::new(Mutex::new(ExportStatus::Idle));
        let cancel_flag = Arc::new(AtomicBool::new(false));

        let result = export_video(
            fixture_path,
            output_path,
            ranges,
            false, // cut_mode = false → ranges are kept
            metadata,
            status.clone(),
            cancel_flag,
        );

        assert!(result.is_ok(), "export_video failed: {:?}", result.err());

        // Validate output exists
        assert!(std::path::Path::new(output_path).exists(), "Output file not created");

        // Use ffprobe to extract video PTS values and check for gaps
        let ffprobe_output = std::process::Command::new("ffprobe")
            .args([
                "-v", "error",
                "-select_streams", "v:0",
                "-show_entries", "packet=pts_time",
                "-of", "csv=p=0",
                output_path,
            ])
            .output()
            .expect("ffprobe not found");

        let pts_text = String::from_utf8_lossy(&ffprobe_output.stdout);
        let mut pts_values: Vec<f64> = pts_text
            .lines()
            .filter_map(|line| line.trim().parse::<f64>().ok())
            .collect();

        assert!(!pts_values.is_empty(), "No video PTS values in output");

        // Sort by PTS (display order)
        pts_values.sort_by(|a, b| a.partial_cmp(b).unwrap());

        // Check for gaps > 2x frame duration
        let frame_duration = 1001.0 / 24000.0; // ~0.0417s
        let max_gap = frame_duration * 2.5; // Allow some tolerance for rounding
        let mut gaps = Vec::new();
        for i in 1..pts_values.len() {
            let delta = pts_values[i] - pts_values[i - 1];
            if delta > max_gap {
                gaps.push((i, pts_values[i - 1], pts_values[i], delta));
            }
        }

        // Also check for negative deltas (non-monotonic in display order)
        let mut non_monotonic = Vec::new();
        for i in 1..pts_values.len() {
            let delta = pts_values[i] - pts_values[i - 1];
            if delta < 0.0 {
                non_monotonic.push((i, pts_values[i - 1], pts_values[i]));
            }
        }

        // Report findings
        if !gaps.is_empty() {
            eprintln!("[DEBUG-exp1] PTS GAPS FOUND ({}):", gaps.len());
            for (idx, before, after, delta) in &gaps {
                eprintln!("  Frame {}: {:.3}s → {:.3}s (gap={:.3}s, expected ≤{:.3}s)",
                    idx, before, after, delta, max_gap);
            }
        }

        if !non_monotonic.is_empty() {
            eprintln!("[DEBUG-exp1] NON-MONOTONIC PTS ({}):", non_monotonic.len());
            for (idx, before, after) in &non_monotonic {
                eprintln!("  Frame {}: {:.3}s → {:.3}s", idx, before, after);
            }
        }

        eprintln!("[DEBUG-exp1] Total output frames: {}", pts_values.len());
        eprintln!("[DEBUG-exp1] PTS range: {:.3}s → {:.3}s",
            pts_values.first().unwrap(), pts_values.last().unwrap());

        // THE ASSERTION: no PTS gaps allowed
        assert!(gaps.is_empty(),
            "Output video has {} PTS gap(s) > {:.3}s. First gap: {:.3}s → {:.3}s (delta={:.3}s). \
             This indicates the FullyKept and PartiallyKept code paths disagree on output timestamps.",
            gaps.len(), max_gap,
            gaps[0].1, gaps[0].2, gaps[0].3);

        // Clean up
        // let _ = std::fs::remove_file(output_path);
    }
}
