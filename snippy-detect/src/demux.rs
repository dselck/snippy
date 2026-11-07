use std::path::Path;
use ffmpeg_next::format::Sample;
use ffmpeg_next::ChannelLayout;

pub struct MediaData {
    pub keyframes: Vec<DecodedKeyframe>,
    pub audio_pcm: Vec<f32>,
    pub audio_sample_rate: u32,
}

pub struct DecodedKeyframe {
    pub timestamp_seconds: f64,
    pub rgb_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub is_keyframe: bool,
}

pub fn decode_media<P: AsRef<Path>>(path: P) -> Result<MediaData, anyhow::Error> {
    ffmpeg_next::init().ok();

    let mut input = ffmpeg_next::format::input(&path)?;

    let video_stream = input.streams().best(ffmpeg_next::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;
    let video_stream_idx = video_stream.index();
    let video_tb = {
        let tb = video_stream.time_base();
        tb.numerator() as f64 / tb.denominator() as f64
    };
    let video_start_time = if video_stream.start_time() > -1_000_000_000_000 {
        video_stream.start_time() as f64 * video_tb
    } else {
        0.0
    };
    let codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
    let mut video_decoder = codec_ctx.decoder().video()?;
    let mut scaler: Option<ffmpeg_next::software::scaling::context::Context> = None;

    let audio_stream = input.streams().best(ffmpeg_next::media::Type::Audio);
    let mut audio_decoder = None;
    let mut audio_stream_idx = None;
    let mut resampler: Option<ffmpeg_next::software::resampling::context::Context> = None;
    let target_sample_rate = 16000;
    let target_channels = 1;

    if let Some(ref stream) = audio_stream {
        let codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let dec = codec_ctx.decoder().audio()?;
        audio_stream_idx = Some(stream.index());
        audio_decoder = Some(dec);
    }

    let mut keyframes = Vec::new();
    let mut audio_pcm = Vec::new();

    // Visual sampler: e.g. decode keyframe at least every 2 seconds
    let sampling_interval = 2.0;
    let mut last_decoded_video_time: Option<f64> = None;

    for (stream_ref, packet) in input.packets() {
        if stream_ref.index() == video_stream_idx {
            if packet.is_key() {
                let pts = packet.pts().unwrap_or_else(|| packet.dts().unwrap_or(0));
                let pts_sec = pts as f64 * video_tb - video_start_time;

                let should_decode = match last_decoded_video_time {
                    None => true,
                    Some(last_t) => pts_sec >= last_t + sampling_interval,
                };

                if should_decode {
                    video_decoder.flush();
                    if video_decoder.send_packet(&packet).is_ok() {
                        let mut raw_frame = ffmpeg_next::util::frame::Video::empty();
                        while video_decoder.receive_frame(&mut raw_frame).is_ok() {
                            let actual = if raw_frame.format() == ffmpeg_next::format::Pixel::VIDEOTOOLBOX
                                || raw_frame.format() == ffmpeg_next::format::Pixel::D3D11
                            {
                                let mut sw = ffmpeg_next::util::frame::Video::empty();
                                let ok = unsafe {
                                    ffmpeg_next::sys::av_hwframe_transfer_data(
                                        sw.as_mut_ptr(), raw_frame.as_ptr(), 0,
                                    )
                                };
                                if ok < 0 { raw_frame.clone() } else { sw }
                            } else {
                                raw_frame.clone()
                            };

                            let w = actual.width();
                            let h = actual.height();

                            let sc = match scaler.as_mut() {
                                Some(s) if s.input().format == actual.format()
                                    && s.input().width == w
                                    && s.input().height == h => s,
                                _ => {
                                    let s = ffmpeg_next::software::scaling::context::Context::get(
                                        actual.format(),
                                        w,
                                        h,
                                        ffmpeg_next::format::Pixel::RGB24,
                                        w,
                                        h,
                                        ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
                                    )?;
                                    scaler = Some(s);
                                    scaler.as_mut().unwrap()
                                }
                            };

                            let mut scaled = ffmpeg_next::util::frame::Video::empty();
                            if sc.run(&actual, &mut scaled).is_ok() {
                                let sw = scaled.width() as usize;
                                let sh = scaled.height() as usize;
                                let stride = scaled.stride(0) as usize;
                                let data = scaled.data(0);
                                let mut rgb = Vec::with_capacity(sw * sh * 3);
                                for y in 0..sh {
                                    let s = y * stride;
                                    if let Some(row) = data.get(s..s + sw * 3) {
                                        rgb.extend_from_slice(row);
                                    }
                                }

                                keyframes.push(DecodedKeyframe {
                                    timestamp_seconds: pts_sec,
                                    rgb_data: rgb,
                                    width: sw as u32,
                                    height: sh as u32,
                                    is_keyframe: true,
                                });
                                last_decoded_video_time = Some(pts_sec);
                                break; // Only need one frame per keyframe packet
                            }
                        }
                    }
                }
            }
        } else if Some(stream_ref.index()) == audio_stream_idx {
            if let Some(ref mut dec) = audio_decoder {
                if dec.send_packet(&packet).is_ok() {
                    let mut raw_frame = ffmpeg_next::util::frame::Audio::empty();
                    while dec.receive_frame(&mut raw_frame).is_ok() {
                        let r = match resampler {
                            Some(ref mut r) if r.input().format == raw_frame.format()
                                && r.input().channel_layout == raw_frame.channel_layout()
                                && r.input().rate == raw_frame.rate() => r,
                            _ => {
                                let new_r = ffmpeg_next::software::resampling::context::Context::get(
                                    raw_frame.format(),
                                    raw_frame.channel_layout(),
                                    raw_frame.rate(),
                                    Sample::F32(ffmpeg_next::format::sample::Type::Packed),
                                    ChannelLayout::MONO,
                                    target_sample_rate,
                                )?;
                                resampler = Some(new_r);
                                resampler.as_mut().unwrap()
                            }
                        };

                        let mut resampled = ffmpeg_next::util::frame::Audio::empty();
                        if r.run(&raw_frame, &mut resampled).is_ok() {
                            let sample_count = resampled.samples();
                            if sample_count > 0 {
                                let ptr = resampled.data(0).as_ptr() as *const f32;
                                if !ptr.is_null() {
                                    let data_slice = unsafe {
                                        std::slice::from_raw_parts(ptr, sample_count * target_channels)
                                    };
                                    audio_pcm.extend_from_slice(data_slice);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Flush audio decoder
    if let Some(ref mut dec) = audio_decoder {
        let _ = dec.send_eof();
        let mut raw_frame = ffmpeg_next::util::frame::Audio::empty();
        while dec.receive_frame(&mut raw_frame).is_ok() {
            let r = match resampler {
                Some(ref mut r) if r.input().format == raw_frame.format()
                    && r.input().channel_layout == raw_frame.channel_layout()
                    && r.input().rate == raw_frame.rate() => Some(r),
                _ => None,
            };
            if let Some(r) = r {
                let mut resampled = ffmpeg_next::util::frame::Audio::empty();
                if r.run(&raw_frame, &mut resampled).is_ok() {
                    let sample_count = resampled.samples();
                    if sample_count > 0 {
                        let ptr = resampled.data(0).as_ptr() as *const f32;
                        if !ptr.is_null() {
                            let data_slice = unsafe {
                                std::slice::from_raw_parts(ptr, sample_count * target_channels)
                            };
                            audio_pcm.extend_from_slice(data_slice);
                        }
                    }
                }
            }
        }
    }

    Ok(MediaData {
        keyframes,
        audio_pcm,
        audio_sample_rate: target_sample_rate,
    })
}
