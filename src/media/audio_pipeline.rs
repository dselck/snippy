use ffmpeg_next::software::resampling::context::Context as ResamplerContext;
use ffmpeg_next::format::Sample;
use ffmpeg_next::ChannelLayout;

pub struct AudioChunk {
    pub data: Vec<f32>,
    pub tag: Option<(u64, u64)>, // (frames_written_total, pts_us)
}

pub struct AudioDecoderPipeline {
    decoder: ffmpeg_next::decoder::Audio,
    resampler: Option<ResamplerContext>,
    target_sample_rate: u32,
    target_channels: usize,
    audio_tb_f64: f64,
    video_start_time: f64,

    skip_until_time: f64,
    is_first_audio_frame: bool,
    current_audio_time: f64,
    pub frames_written_total: u64,
    last_emitted_pts: Option<f64>,
}

impl AudioDecoderPipeline {
    pub fn new(
        decoder: ffmpeg_next::decoder::Audio,
        target_sample_rate: u32,
        target_channels: usize,
        audio_tb_f64: f64,
        video_start_time: f64,
    ) -> Self {
        Self {
            decoder,
            resampler: None,
            target_sample_rate,
            target_channels,
            audio_tb_f64,
            video_start_time,
            skip_until_time: 0.0,
            is_first_audio_frame: true,
            current_audio_time: 0.0,
            frames_written_total: 0,
            last_emitted_pts: None,
        }
    }

    pub fn last_emitted_pts(&self) -> Option<f64> {
        self.last_emitted_pts
    }

    pub fn handle_seek(&mut self, target_time: f64) {
        self.decoder.flush();
        self.resampler = None;
        self.skip_until_time = target_time;
        self.is_first_audio_frame = true;
        self.current_audio_time = self.video_start_time + target_time;
        self.frames_written_total = 0;
        self.last_emitted_pts = None;
    }

    pub fn update_current_time_from_pts(&mut self, pts: i64) {
        if self.is_first_audio_frame {
            self.current_audio_time = pts as f64 * self.audio_tb_f64;
        }
    }

    pub fn send_packet(&mut self, packet: &ffmpeg_next::Packet) -> Result<(), ffmpeg_next::Error> {
        self.decoder.send_packet(packet)
    }

    pub fn receive_samples(&mut self) -> Result<Vec<AudioChunk>, ffmpeg_next::Error> {
        let mut chunks = Vec::new();
        let mut frame = ffmpeg_next::util::frame::Audio::empty();
        
        while self.decoder.receive_frame(&mut frame).is_ok() {
            let duration_sec = frame.samples() as f64 / frame.rate() as f64;
            let pts_sec = if let Some(timestamp) = frame.timestamp() {
                timestamp as f64 * self.audio_tb_f64
            } else if let Some(pts) = frame.pts() {
                pts as f64 * self.audio_tb_f64
            } else {
                let pkt_dts = unsafe { (*frame.as_ptr()).pkt_dts };
                if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                    pkt_dts as f64 * self.audio_tb_f64
                } else {
                    self.current_audio_time
                }
            };
            self.current_audio_time = pts_sec + duration_sec;

            if self.is_first_audio_frame {
                let target_container_time = self.video_start_time + self.skip_until_time;
                if pts_sec + duration_sec <= target_container_time {
                    continue; // Discard whole frame before seek target
                }
            }

            let r = match self.resampler {
                Some(ref mut r) if r.input().format == frame.format()
                    && r.input().channel_layout == frame.channel_layout()
                    && r.input().rate == frame.rate() => r,
                _ => {
                    let target_layout = if self.target_channels == 1 {
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
                        self.target_sample_rate,
                    ).unwrap();
                    self.resampler = Some(new_r);
                    self.resampler.as_mut().unwrap()
                }
            };

            let mut resampled = ffmpeg_next::util::frame::Audio::empty();
            if r.run(&frame, &mut resampled).is_ok() {
                let sample_count = resampled.samples();
                if sample_count > 0 {
                    let byte_count = sample_count * self.target_channels * 4;
                    let data_slice: &[f32] = bytemuck::cast_slice(&resampled.data(0)[..byte_count]);
                    let mut data = data_slice.to_vec();
                    let mut data_frames = (data.len() / self.target_channels) as u64;
                    
                    if self.is_first_audio_frame {
                        let target_container_time = self.video_start_time + self.skip_until_time;
                        if pts_sec < target_container_time {
                            // Trim overlap
                            let overlap_sec = target_container_time - pts_sec;
                            let discard_frames = (overlap_sec * self.target_sample_rate as f64).round() as usize;
                            let discard_samples = discard_frames * self.target_channels;
                            if discard_samples < data.len() {
                                data = data[discard_samples..].to_vec();
                                data_frames = (data.len() / self.target_channels) as u64;
                                let pts_us = (self.skip_until_time.max(0.0) * 1_000_000.0) as u64;
                                chunks.push(AudioChunk { data, tag: Some((self.frames_written_total, pts_us)) });
                            } else {
                                continue;
                            }
                        } else {
                            // Gap: Pad with silence or handle large gap
                            let gap_sec = pts_sec - target_container_time;
                            if gap_sec <= 1.0 {
                                let pad_samples = (gap_sec * self.target_sample_rate as f64).round() as usize;
                                let pts_us = (self.skip_until_time.max(0.0) * 1_000_000.0) as u64;
                                let pad_data = vec![0.0; pad_samples * self.target_channels];
                                chunks.push(AudioChunk { data: pad_data, tag: Some((self.frames_written_total, pts_us)) });
                                self.frames_written_total += pad_samples as u64;
                                chunks.push(AudioChunk { data, tag: None }); // data follows padding
                            } else {
                                // Large Gap: No padding, tag at actual PTS
                                let pts_us = ((pts_sec - self.video_start_time).max(0.0) * 1_000_000.0) as u64;
                                chunks.push(AudioChunk { data, tag: Some((self.frames_written_total, pts_us)) });
                            }
                        }
                        self.is_first_audio_frame = false;
                    } else {
                        if let Some(last_pts) = self.last_emitted_pts {
                            let delta = pts_sec - last_pts - duration_sec;
                            if delta > 1.5 * duration_sec {
                                // Positive gap: packet dropout! Synthesize silence to keep Master Clock monotonically in sync
                                // Clamp gap to at most 10 seconds to safeguard against unbounded allocations on malformed timestamps
                                let effective_delta = delta.min(10.0);
                                let missing_samples = (effective_delta * self.target_sample_rate as f64).round() as usize;
                                let pad_data = vec![0.0_f32; missing_samples * self.target_channels];
                                let gap_pts_us = ((last_pts + duration_sec).max(0.0) * 1_000_000.0) as u64;
                                chunks.push(AudioChunk {
                                    data: pad_data,
                                    tag: Some((self.frames_written_total, gap_pts_us)),
                                });
                                self.frames_written_total += missing_samples as u64;

                                // Apply linear gain ramp on the incoming valid frame's samples (e.g. ramp over first 128 samples)
                                let ramp_frames = 128.min(data.len() / self.target_channels);
                                for f in 0..ramp_frames {
                                    let gain = f as f32 / 128.0;
                                    for c in 0..self.target_channels {
                                        data[f * self.target_channels + c] *= gain;
                                    }
                                }
                            } else if delta < -0.001 {
                                // Negative gap: overlap from duplicate packets or splices. Trim overlapping samples so frames_written_total doesn't run ahead of container PTS.
                                let overlap_sec = -delta;
                                let discard_frames = (overlap_sec * self.target_sample_rate as f64).round() as usize;
                                let discard_samples = (discard_frames * self.target_channels).min(data.len());
                                data = data[discard_samples..].to_vec();
                                data_frames = (data.len() / self.target_channels) as u64;
                            }
                        }

                        if !data.is_empty() {
                            let pts_us = ((pts_sec - self.video_start_time).max(0.0) * 1_000_000.0) as u64;
                            chunks.push(AudioChunk {
                                data,
                                tag: Some((self.frames_written_total, pts_us)),
                            });
                        }
                    }
                    self.frames_written_total += data_frames;
                    self.last_emitted_pts = Some(pts_sec);
                }
            }
        }
        Ok(chunks)
    }
}

/// Splits a decoded `AudioChunk` across Loudness Bucket boundaries and returns
/// `(bucket_idx, rms)` pairs for every bucket the chunk covers.
///
/// Bucket width = 1 frame / 4  →  `bucket_idx = floor(pts_sec × fps × 4)`.
/// All channels are averaged into the RMS reading.
/// Returns an empty vec when the chunk has no `tag` (position unknown).
pub fn compute_rms_buckets(
    chunk: &AudioChunk,
    fps: f64,
    target_channels: usize,
    target_sample_rate: u32,
) -> Vec<(usize, f32)> {
    let Some((_frames_written, pts_us)) = chunk.tag else {
        return vec![];
    };
    let ch = target_channels.max(1);
    let samples_per_channel = chunk.data.len() / ch;
    if samples_per_channel == 0 {
        return vec![];
    }

    const BUCKETS_PER_FRAME: f64 = 4.0;
    let bucket_dur = 1.0 / (fps * BUCKETS_PER_FRAME);
    let pts_sec = pts_us as f64 / 1_000_000.0;
    let chunk_dur = samples_per_channel as f64 / target_sample_rate as f64;

    let first_bucket = (pts_sec / bucket_dur) as usize;
    let last_bucket = ((pts_sec + chunk_dur) / bucket_dur) as usize;

    let mut result = Vec::with_capacity(last_bucket.saturating_sub(first_bucket) + 1);

    for b in first_bucket..=last_bucket {
        let b_start = b as f64 * bucket_dur;
        let b_end = (b + 1) as f64 * bucket_dur;

        // Sample-frame indices relative to the start of this chunk
        let rel_start =
            ((b_start - pts_sec) * target_sample_rate as f64).max(0.0) as usize;
        let rel_end = ((b_end - pts_sec) * target_sample_rate as f64)
            .min(samples_per_channel as f64) as usize;

        if rel_start >= rel_end {
            continue;
        }

        let mut sum_sq = 0.0f64;
        let mut count = 0usize;
        for s in rel_start..rel_end {
            for c in 0..ch {
                let idx = s * ch + c;
                if let Some(&sample) = chunk.data.get(idx) {
                    sum_sq += (sample as f64) * (sample as f64);
                    count += 1;
                }
            }
        }

        if count > 0 {
            let rms = ((sum_sq / count as f64).sqrt() as f32).clamp(0.0, 1.0);
            result.push((b, rms));
        }
    }

    result
}

#[cfg(test)]
mod tests_rms {
    use super::*;

    #[test]
    fn test_rms_no_tag_returns_empty() {
        let chunk = AudioChunk { data: vec![1.0; 1024], tag: None };
        let result = compute_rms_buckets(&chunk, 30.0, 1, 44100);
        assert!(result.is_empty());
    }

    #[test]
    fn test_rms_silence_produces_zero() {
        let samples_per_bucket = (44100.0f64 / (30.0 * 4.0)).ceil() as usize;
        let chunk = AudioChunk {
            data: vec![0.0; samples_per_bucket],
            tag: Some((0, 0)),
        };
        let result = compute_rms_buckets(&chunk, 30.0, 1, 44100);
        assert!(!result.is_empty());
        for (_, rms) in &result {
            assert_eq!(*rms, 0.0, "silent audio should have rms=0.0");
        }
    }

    #[test]
    fn test_rms_full_scale_sine_near_0707() {
        // RMS of a full-scale sine wave is 1/sqrt(2) ≈ 0.707
        let sample_rate = 44100u32;
        let fps = 30.0;
        let samples_per_bucket = (sample_rate as f64 / (fps * 4.0)) as usize;
        let n_samples = samples_per_bucket * 4;
        let data: Vec<f32> = (0..n_samples)
            .map(|i| {
                (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / sample_rate as f32).sin()
            })
            .collect();
        let chunk = AudioChunk { data, tag: Some((0, 0)) };
        let result = compute_rms_buckets(&chunk, fps, 1, sample_rate);
        assert!(!result.is_empty());
        for (_, rms) in &result {
            assert!((*rms - 0.707).abs() < 0.02, "expected rms≈0.707, got {rms}");
        }
    }

    #[test]
    fn test_rms_bucket_index_at_100ms() {
        // pts=0.1s → bucket = floor(0.1 / (1/(30*4))) = floor(12.0) = 12
        let chunk = AudioChunk {
            data: vec![1.0f32; 100],
            tag: Some((0, 100_000)), // 100 000 µs = 0.1 s
        };
        let result = compute_rms_buckets(&chunk, 30.0, 1, 44100);
        assert!(!result.is_empty());
        assert_eq!(result[0].0, 12, "wrong bucket index");
    }
}

#[cfg(test)]
mod tests_padding {
    #[test]
    fn test_silence_padding_gain_ramp_and_overlap_math() {
        let target_sample_rate = 48000;
        let target_channels = 2;
        let last_pts = 1.0f64;
        let duration_sec = 0.02f64; // nominal frame duration
        let pts_sec = 1.10f64; // gap of 0.08s after frame at 1.00s..1.02s

        let delta = pts_sec - last_pts - duration_sec;
        assert!(delta > 1.5 * duration_sec);

        let missing_samples = (delta * target_sample_rate as f64).round() as usize;
        let pad_data = vec![0.0_f32; missing_samples * target_channels];
        let gap_pts_us = ((last_pts + duration_sec).max(0.0) * 1_000_000.0) as u64;

        assert_eq!(missing_samples, (0.08 * 48000.0) as usize);
        assert_eq!(pad_data.len(), missing_samples * 2);
        assert_eq!(gap_pts_us, 1_020_000);

        // Test linear gain ramp on resumption:
        let mut data = vec![1.0_f32; 256 * target_channels];
        let ramp_frames = 128.min(data.len() / target_channels);
        for f in 0..ramp_frames {
            let gain = f as f32 / 128.0;
            for c in 0..target_channels {
                data[f * target_channels + c] *= gain;
            }
        }
        // First sample frame is 0.0
        assert_eq!(data[0], 0.0);
        assert_eq!(data[1], 0.0);
        // Halfway through ramp (frame 64) is 0.5
        assert!((data[64 * 2] - 0.5).abs() < 1e-4);
        assert!((data[64 * 2 + 1] - 0.5).abs() < 1e-4);
        // Frame 128 (after ramp) is 1.0
        assert_eq!(data[128 * 2], 1.0);

        // Test overlap trimming formula:
        let overlap_delta = -0.005f64; // 5ms overlap
        let overlap_sec = -overlap_delta;
        let discard_frames = (overlap_sec * target_sample_rate as f64).round() as usize;
        let discard_samples = (discard_frames * target_channels).min(data.len());
        let trimmed_data = data[discard_samples..].to_vec();
        assert_eq!(discard_frames, 240);
        assert_eq!(discard_samples, 240 * 2);
        assert_eq!(trimmed_data.len(), data.len() - 240 * 2);
    }
}
