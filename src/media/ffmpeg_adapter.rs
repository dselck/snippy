use ffmpeg_next::Packet;
use crate::media::types::{DecodedFrame, YuvPlanes};
use crate::media::decoder_pipeline::{DecoderAdapter, DecodeError};

pub struct FfmpegDecoderAdapter {
    video_decoder: ffmpeg_next::decoder::Video,
    scaler: Option<ffmpeg_next::software::scaling::context::Context>,
    time_base_f64: f64,
    video_start_time: f64,
    fps: f64,
    fallback_frame_idx: usize,
}

impl FfmpegDecoderAdapter {
    pub fn new(
        video_decoder: ffmpeg_next::decoder::Video,
        time_base_f64: f64,
        video_start_time: f64,
        fps: f64,
    ) -> Self {
        Self {
            video_decoder,
            scaler: None,
            time_base_f64,
            video_start_time,
            fps,
            fallback_frame_idx: 0,
        }
    }
}

impl DecoderAdapter for FfmpegDecoderAdapter {
    fn send_packet(&mut self, packet: &Packet) -> Result<(), DecodeError> {
        self.video_decoder.send_packet(packet).map_err(|e| DecodeError::Fault(e.to_string()))
    }

    fn receive_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
        let mut frame = ffmpeg_next::util::frame::Video::empty();
        match self.video_decoder.receive_frame(&mut frame) {
            Ok(_) => {
                let mut scaled_frame = ffmpeg_next::util::frame::Video::empty();
                let video_frame = if frame.format() != ffmpeg_next::format::Pixel::YUV420P {
                    let s = match self.scaler {
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
                            ).map_err(|e| DecodeError::Fault(e.to_string()))?;
                            self.scaler = Some(new_s);
                            self.scaler.as_mut().unwrap()
                        }
                    };
                    s.run(&frame, &mut scaled_frame).map_err(|e| DecodeError::Fault(e.to_string()))?;
                    &scaled_frame
                } else {
                    &frame
                };

                let planes = std::sync::Arc::new(YuvPlanes {
                    y: video_frame.data(0).to_vec(),
                    u: video_frame.data(1).to_vec(),
                    v: video_frame.data(2).to_vec(),
                    width: video_frame.width(),
                    height: video_frame.height(),
                    y_stride: video_frame.stride(0) as u32,
                    u_stride: video_frame.stride(1) as u32,
                    v_stride: video_frame.stride(2) as u32,
                });

                let pts_sec = match frame.pts() {
                    Some(pts_val) => pts_val as f64 * self.time_base_f64 - self.video_start_time,
                    None => self.fallback_frame_idx as f64 / self.fps,
                };
                let frame_idx = (pts_sec * self.fps).round() as usize;
                self.fallback_frame_idx = frame_idx + 1;

                Ok(Some(DecodedFrame::Cpu {
                    frame_idx,
                    planes,
                    sequence_id: 0,
                    pts_sec,
                }))
            }
            Err(ffmpeg_next::Error::Eof) => Ok(None),
            Err(e) => {
                if e.to_string().contains("EAGAIN") || e.to_string().contains("Resource temporarily unavailable") {
                    Ok(None)
                } else {
                    Err(DecodeError::Fault(e.to_string()))
                }
            }
        }
    }

    fn flush(&mut self) {
        self.video_decoder.flush();
    }
}
