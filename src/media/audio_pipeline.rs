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
        }
    }

    pub fn handle_seek(&mut self, target_time: f64) {
        self.decoder.flush();
        self.resampler = None;
        self.skip_until_time = target_time;
        self.is_first_audio_frame = true;
        self.current_audio_time = self.video_start_time + target_time;
        self.frames_written_total = 0;
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
            let pts_sec = match frame.pts() {
                Some(pts) => pts as f64 * self.audio_tb_f64,
                None => self.current_audio_time,
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
                            if gap_sec <= 0.15 {
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
                        let pts_us = ((pts_sec - self.video_start_time).max(0.0) * 1_000_000.0) as u64;
                        chunks.push(AudioChunk { data, tag: Some((self.frames_written_total, pts_us)) });
                    }
                    self.frames_written_total += data_frames;
                }
            }
        }
        Ok(chunks)
    }
}
