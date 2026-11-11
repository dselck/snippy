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
        mut video_decoder: ffmpeg_next::decoder::Video,
        time_base_f64: f64,
        video_start_time: f64,
        fps: f64,
    ) -> Self {
        unsafe {
            let ctx = &mut *video_decoder.as_mut_ptr();
            ctx.error_concealment = ffmpeg_next::sys::FF_EC_GUESS_MVS | ffmpeg_next::sys::FF_EC_DEBLOCK;
            ctx.err_recognition = ffmpeg_next::sys::AV_EF_COMPLIANT;
        }

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
                let pts_sec = if let Some(timestamp) = frame.timestamp() {
                    timestamp as f64 * self.time_base_f64 - self.video_start_time
                } else if let Some(pts) = frame.pts() {
                    pts as f64 * self.time_base_f64 - self.video_start_time
                } else {
                    let pkt_dts = unsafe { (*frame.as_ptr()).pkt_dts };
                    if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                        pkt_dts as f64 * self.time_base_f64 - self.video_start_time
                    } else {
                        self.fallback_frame_idx as f64 / self.fps
                    }
                };
                let frame_idx = (pts_sec * self.fps).round() as usize;
                self.fallback_frame_idx = frame_idx + 1;

                #[cfg(target_os = "macos")]
                if frame.format() == ffmpeg_next::format::Pixel::VIDEOTOOLBOX {
                    // Clone the frame so FFmpeg doesn't reuse the CVPixelBuffer while we are rendering it.
                    let mut hw_clone = ffmpeg_next::util::frame::Video::empty();
                    unsafe {
                        let ret = ffmpeg_next::sys::av_frame_ref(hw_clone.as_mut_ptr(), frame.as_ptr());
                        if ret < 0 {
                            return Err(DecodeError::Fault(format!("Failed to clone hardware frame: {}", ret)));
                        }
                    }
                    
                    let wrapper = std::sync::Arc::new(crate::media::types::HwFrameWrapper {
                        frame: hw_clone,
                    });

                    return Ok(Some(DecodedFrame::Gpu {
                        frame_idx,
                        descriptor: crate::media::types::GpuFrameDescriptor::MacOsHwFrame(wrapper),
                        sequence_id: 0,
                        pts_sec,
                        color_space: frame.color_space(),
                        color_range: frame.color_range(),
                        is_concealed: false,
                        held_frame_idx: None,
                    }));
                }

                #[cfg(target_os = "linux")]
                if crate::media::hw_context::is_hw_pixel_format(frame.format()) {
                    let mut sw_frame = ffmpeg_next::util::frame::Video::empty();
                    let ret = unsafe {
                        ffmpeg_next::sys::av_hwframe_transfer_data(
                            sw_frame.as_mut_ptr(),
                            frame.as_ptr(),
                            0,
                        )
                    };
                    if ret < 0 {
                        return Err(DecodeError::Fault(format!("Failed to transfer hardware frame: {}", ret)));
                    }
                    sw_frame.set_pts(frame.pts());
                    sw_frame.set_color_space(frame.color_space());
                    sw_frame.set_color_range(frame.color_range());

                    if sw_frame.format() == ffmpeg_next::format::Pixel::NV12
                        || sw_frame.format() == ffmpeg_next::format::Pixel::P010LE
                        || sw_frame.format() == ffmpeg_next::format::Pixel::P010BE
                    {
                        let wrapper = std::sync::Arc::new(crate::media::types::HwFrameWrapper {
                            frame: sw_frame,
                        });

                        return Ok(Some(DecodedFrame::Gpu {
                            frame_idx,
                            descriptor: crate::media::types::GpuFrameDescriptor::LinuxHwFrame(wrapper),
                            sequence_id: 0,
                            pts_sec,
                            color_space: frame.color_space(),
                            color_range: frame.color_range(),
                            is_concealed: false,
                            held_frame_idx: None,
                        }));
                    }

                    frame = sw_frame;
                }

                let actual_frame = frame.clone();

                let actual_color_space = actual_frame.color_space();
                let actual_color_range = actual_frame.color_range();

                let mut scaled_frame = ffmpeg_next::util::frame::Video::empty();
                let video_frame = if actual_frame.format() != ffmpeg_next::format::Pixel::YUV420P {
                    let s = match self.scaler {
                        Some(ref mut s) if s.input().format == actual_frame.format()
                            && s.input().width == actual_frame.width()
                            && s.input().height == actual_frame.height() => s,
                        _ => {
                            let new_s = ffmpeg_next::software::scaling::context::Context::get(
                                actual_frame.format(),
                                actual_frame.width(),
                                actual_frame.height(),
                                ffmpeg_next::format::Pixel::YUV420P,
                                actual_frame.width(),
                                actual_frame.height(),
                                ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
                            ).map_err(|e| DecodeError::Fault(e.to_string()))?;
                            self.scaler = Some(new_s);
                            self.scaler.as_mut().unwrap()
                        }
                    };
                    s.run(&actual_frame, &mut scaled_frame).map_err(|e| DecodeError::Fault(e.to_string()))?;
                    scaled_frame
                } else {
                    actual_frame
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

                Ok(Some(DecodedFrame::Cpu {
                    frame_idx,
                    planes,
                    sequence_id: 0,
                    pts_sec,
                    color_space: actual_color_space,
                    color_range: actual_color_range,
                    is_concealed: false,
                    held_frame_idx: None,
                }))
            }
            Err(ffmpeg_next::Error::Eof) => Ok(None),
            Err(ffmpeg_next::Error::Other { errno }) if errno == ffmpeg_next::sys::EAGAIN => Ok(None),
            Err(e) => {
                if e.to_string().contains("EAGAIN") || e.to_string().contains("Resource temporarily unavailable") {
                    Ok(None)
                } else {
                    Err(DecodeError::Fault(e.to_string()))
                }
            }
        }
    }

    fn flush(&mut self, fallback_frame_idx: usize) {
        self.video_decoder.flush();
        self.fallback_frame_idx = fallback_frame_idx;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::hw_context::probe_and_attach_hw_context;
    use crate::media::types::GpuFrameDescriptor;

    #[test]
    fn test_ffmpeg_adapter_hw_decode() {
        let _ = ffmpeg_next::init();
        let test_path = std::path::Path::new("/tmp/test_h264.mp4");
        if !test_path.exists() {
            eprintln!("Test file /tmp/test_h264.mp4 does not exist, skipping test");
            return;
        }

        let mut input = ffmpeg_next::format::input(&test_path).expect("Failed to open test file");
        let stream = input.streams().best(ffmpeg_next::media::Type::Video).expect("No video stream");
        let stream_idx = stream.index();
        let time_base = stream.time_base();
        let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;

        let mut codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
            .expect("Failed to create context");
        let hw_info = probe_and_attach_hw_context(&mut codec_ctx);
        let decoder = codec_ctx.decoder().video().expect("Failed to create decoder");

        let mut adapter = FfmpegDecoderAdapter::new(decoder, time_base_f64, 0.0, 30.0);

        let mut decoded_any = false;
        let mut got_hw_frame = false;

        for (st, packet) in input.packets() {
            if st.index() == stream_idx {
                adapter.send_packet(&packet).expect("Failed to send packet");
                while let Ok(Some(frame)) = adapter.receive_frame() {
                    decoded_any = true;
                    if let DecodedFrame::Gpu { descriptor, .. } = frame {
                        #[cfg(target_os = "linux")]
                        {
                            let GpuFrameDescriptor::LinuxHwFrame(hw) = descriptor;
                            let fmt = hw.frame.format();
                            assert!(
                                fmt == ffmpeg_next::format::Pixel::NV12
                                    || fmt == ffmpeg_next::format::Pixel::P010LE
                                    || fmt == ffmpeg_next::format::Pixel::P010BE
                            );
                            got_hw_frame = true;
                        }
                        #[cfg(target_os = "macos")]
                        {
                            let GpuFrameDescriptor::MacOsHwFrame(_) = descriptor;
                            got_hw_frame = true;
                        }
                    }
                    if decoded_any && (hw_info.is_none() || got_hw_frame) {
                        break;
                    }
                }
                if decoded_any && (hw_info.is_none() || got_hw_frame) {
                    break;
                }
            }
        }

        assert!(decoded_any, "Expected to decode at least one frame");
        if hw_info.is_some() {
            #[cfg(target_os = "linux")]
            assert!(got_hw_frame, "Expected LinuxHwFrame when hardware acceleration is active");
            #[cfg(target_os = "macos")]
            assert!(got_hw_frame, "Expected MacOsHwFrame when hardware acceleration is active");
        }
    }
}
