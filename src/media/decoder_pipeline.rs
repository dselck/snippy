use ffmpeg_next::Packet;
use crate::media::types::DecodedFrame;
use crate::media::cache::GopCache;

#[derive(Debug)]
pub enum DecodeError {
    EndOfStream,
    Fault(String),
}

pub trait DecoderAdapter {
    fn send_packet(&mut self, packet: &Packet) -> Result<(), DecodeError>;
    fn receive_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError>;
    fn flush(&mut self, fallback_frame_idx: usize);
}

pub struct VideoDecoderPipeline {
    adapter: Box<dyn DecoderAdapter>,
    gop_cache: GopCache,
    keyframes: Vec<usize>,
    
    // State
    play_mode: bool,
    decode_more: bool,
    keyframe_only_mode: bool,
    current_sequence_id: usize,
    last_decoded_frame: Option<usize>,
    seek_target_frame: Option<usize>,
    pub fallback_frame_idx: usize,
    pub is_gop_compromised: bool,
    pub last_valid_frame: Option<DecodedFrame>,
    pub fps: f64,
    pub time_base_f64: f64,
}

impl VideoDecoderPipeline {
    pub fn new(
        adapter: Box<dyn DecoderAdapter>,
        keyframes: Vec<usize>,
        fps: f64,
        time_base_f64: f64,
    ) -> Self {
        Self {
            adapter,
            gop_cache: GopCache::new(),
            keyframes,
            play_mode: false,
            decode_more: true,
            keyframe_only_mode: false,
            current_sequence_id: 0,
            last_decoded_frame: None,
            seek_target_frame: None,
            fallback_frame_idx: 0,
            is_gop_compromised: false,
            last_valid_frame: None,
            fps,
            time_base_f64,
        }
    }



    pub fn play(&mut self) {
        self.play_mode = true;
        self.seek_target_frame = None;
        self.keyframe_only_mode = false;
        self.decode_more = true;
    }

    pub fn play_unpaced(&mut self, sequence_id: usize) {
        self.play_mode = true;
        self.seek_target_frame = None;
        self.keyframe_only_mode = false;
        self.decode_more = true;
        self.current_sequence_id = sequence_id;
    }

    pub fn pause(&mut self) {
        self.play_mode = false;
        if self.seek_target_frame.is_none() {
            self.decode_more = false;
        }
    }

    pub fn is_active(&self) -> bool {
        self.decode_more
    }

    /// Prepares the pipeline for a seek.
    /// Returns `true` if the seek was satisfied from cache (in which case, the caller shouldn't seek FFmpeg).
    pub fn seek(
        &mut self,
        frame_index: usize,
        keyframe_only: bool,
        sequence_id: usize,
        audio_enabled: bool,
    ) -> bool {
        let target_kf = crate::media::types::get_nearest_keyframe(&self.keyframes, frame_index);
        self.gop_cache.clear_if_different_gop(target_kf);

        if !audio_enabled {
            // 1. Check GOP cache first
            if let Some(_cached) = self.gop_cache.get_cached(frame_index) {
                // Cache hit
                self.fallback_frame_idx = frame_index;
                self.keyframe_only_mode = keyframe_only;
                self.current_sequence_id = sequence_id;
                self.decode_more = false;
                self.play_mode = false;
                self.seek_target_frame = None;
                self.last_decoded_frame = None;
                return true; 
            }

            // 2. Check sequential fast path
            if let Some(last) = self.last_decoded_frame {
                if frame_index >= last + 1 && frame_index <= last + 5 {
                    self.fallback_frame_idx = frame_index;
                    self.keyframe_only_mode = keyframe_only;
                    self.current_sequence_id = sequence_id;
                    self.decode_more = true;
                    self.play_mode = false;
                    self.seek_target_frame = Some(frame_index);
                    return true;
                }
            }
        }

        // Cache miss / hard seek
        self.fallback_frame_idx = frame_index;
        self.keyframe_only_mode = keyframe_only;
        self.current_sequence_id = sequence_id;
        self.play_mode = false;
        self.seek_target_frame = Some(frame_index);
        
        self.adapter.flush(target_kf);
        
        self.decode_more = true;
        self.last_decoded_frame = None;
        false
    }

    pub fn get_cached_frame(&self, frame_idx: usize) -> Option<DecodedFrame> {
        self.gop_cache.get_cached(frame_idx).map(|f| {
            let mut f_frame = f.clone();
            match &mut f_frame {
                DecodedFrame::Cpu { sequence_id, .. } => *sequence_id = self.current_sequence_id,
                DecodedFrame::Gpu { sequence_id, .. } => *sequence_id = self.current_sequence_id,
            }
            f_frame
        })
    }
    
    pub fn synthesize_concealment_frame(&self, frame_idx: usize, pts_sec: f64) -> DecodedFrame {
        if let Some(valid) = &self.last_valid_frame {
            let mut concealed = valid.clone();
            let original_valid_idx = valid.frame_idx();
            match &mut concealed {
                DecodedFrame::Cpu {
                    frame_idx: f_idx,
                    pts_sec: f_pts,
                    sequence_id: f_seq,
                    is_concealed: f_conc,
                    held_frame_idx: f_held,
                    ..
                } => {
                    *f_idx = frame_idx;
                    *f_pts = pts_sec;
                    *f_seq = self.current_sequence_id;
                    *f_conc = true;
                    *f_held = Some(original_valid_idx);
                }
                DecodedFrame::Gpu {
                    frame_idx: f_idx,
                    pts_sec: f_pts,
                    sequence_id: f_seq,
                    is_concealed: f_conc,
                    held_frame_idx: f_held,
                    ..
                } => {
                    *f_idx = frame_idx;
                    *f_pts = pts_sec;
                    *f_seq = self.current_sequence_id;
                    *f_conc = true;
                    *f_held = Some(original_valid_idx);
                }
            }
            concealed
        } else {
            let width = 1920;
            let height = 1080;
            let y_size = width * height;
            let uv_size = (width / 2) * (height / 2);
            DecodedFrame::Cpu {
                frame_idx,
                planes: std::sync::Arc::new(crate::media::types::YuvPlanes {
                    y: vec![16u8; y_size],
                    u: vec![128u8; uv_size],
                    v: vec![128u8; uv_size],
                    width: width as u32,
                    height: height as u32,
                    y_stride: width as u32,
                    u_stride: (width / 2) as u32,
                    v_stride: (width / 2) as u32,
                }),
                sequence_id: self.current_sequence_id,
                pts_sec,
                color_space: ffmpeg_next::color::Space::BT709,
                color_range: ffmpeg_next::color::Range::MPEG,
                is_concealed: true,
                held_frame_idx: None,
            }
        }
    }

    /// Processes a video packet, returning any frames that are ready to be displayed.
    pub fn process_packet(&mut self, packet: &Packet) -> Result<Vec<DecodedFrame>, DecodeError> {
        if !self.decode_more {
            return Ok(Vec::new());
        }

        let is_corrupt = packet.is_corrupt()
            || packet
                .flags()
                .contains(ffmpeg_next::packet::Flags::from_bits_truncate(
                    ffmpeg_next::sys::AV_PKT_FLAG_CORRUPT,
                ));

        if packet.is_key() {
            self.is_gop_compromised = false;
        }

        let mut frames = Vec::new();

        if is_corrupt || self.is_gop_compromised {
            self.is_gop_compromised = true;
            // Suppress packet submission to adapter while GOP is compromised (protects hardware decoder surface pool).
            // Directly synthesize a single concealment frame for this presentation slot.
            let pts_sec = if let Some(pts) = packet.pts() {
                pts as f64 * self.time_base_f64
            } else if let Some(last) = self.last_decoded_frame {
                (last + 1) as f64 / self.fps
            } else {
                self.fallback_frame_idx as f64 / self.fps
            };
            let frame_idx = (pts_sec * self.fps).round() as usize;
            let output_frame = self.synthesize_concealment_frame(frame_idx, pts_sec);
            self.last_decoded_frame = Some(frame_idx);
            frames.push(output_frame);
            return Ok(frames);
        } else if let Err(_e) = self.adapter.send_packet(packet) {
            self.is_gop_compromised = true;
            // Decode fault on send_packet: synthesize a single concealment frame for this slot
            let pts_sec = if let Some(pts) = packet.pts() {
                pts as f64 * self.time_base_f64
            } else if let Some(last) = self.last_decoded_frame {
                (last + 1) as f64 / self.fps
            } else {
                self.fallback_frame_idx as f64 / self.fps
            };
            let frame_idx = (pts_sec * self.fps).round() as usize;
            let output_frame = self.synthesize_concealment_frame(frame_idx, pts_sec);
            self.last_decoded_frame = Some(frame_idx);
            frames.push(output_frame);
            return Ok(frames);
        }

        // Bounded frame reception loop (safeguard against runaway/infinite mock decoders)
        let mut loop_count = 0;
        while let Some(mut decoded) = self.adapter.receive_frame()? {
            loop_count += 1;
            if loop_count > 64 {
                // Safeguard: no single packet should ever yield >64 frames
                break;
            }
            match &mut decoded {
                DecodedFrame::Cpu { sequence_id, .. } => {
                    *sequence_id = self.current_sequence_id;
                }
                DecodedFrame::Gpu { sequence_id, .. } => {
                    *sequence_id = self.current_sequence_id;
                }
            }

            let frame_idx = decoded.frame_idx();
            let pts_sec = decoded.pts_sec();

            let output_frame = if self.is_gop_compromised {
                // Suppress damaged frame output and synthesize concealment frame holding last valid frame
                self.synthesize_concealment_frame(frame_idx, pts_sec)
            } else {
                self.last_valid_frame = Some(decoded.clone());
                decoded
            };

            // GOP Cache population: limit cache to the current GOP to prevent unbounded memory growth
            let nearest_kf = crate::media::types::get_nearest_keyframe(&self.keyframes, frame_idx);
            self.gop_cache.clear_if_different_gop(nearest_kf);
            self.gop_cache.insert_frame(frame_idx, output_frame.clone());

            self.last_decoded_frame = Some(frame_idx);

            if !self.play_mode {
                if let Some(target) = self.seek_target_frame {
                    if frame_idx >= target {
                        self.decode_more = false;
                        self.seek_target_frame = None;
                        frames.push(output_frame);
                        break;
                    }
                }
            } else {
                frames.push(output_frame.clone());
            }

            if self.keyframe_only_mode && !self.play_mode {
                self.decode_more = false;
                frames.push(output_frame);
                break;
            }
        }
        
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MockDecoderAdapter {
        fallback_frame_idx: usize,
        has_frame: bool,
        is_gpu: bool,
        fail_send: Arc<AtomicBool>,
    }

    impl DecoderAdapter for MockDecoderAdapter {
        fn send_packet(&mut self, _packet: &ffmpeg_next::Packet) -> Result<(), DecodeError> {
            if self.fail_send.load(Ordering::SeqCst) {
                return Err(DecodeError::Fault("Simulated decode fault".to_string()));
            }
            self.has_frame = true;
            Ok(())
        }

        fn receive_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
            if !self.has_frame {
                return Ok(None);
            }
            self.has_frame = false;
            let pts_sec = self.fallback_frame_idx as f64 / 24.0;
            let frame_idx = (pts_sec * 24.0).round() as usize;
            self.fallback_frame_idx = frame_idx + 1;

            if self.is_gpu {
                #[cfg(target_os = "macos")]
                let descriptor = crate::media::types::GpuFrameDescriptor::CVPixelBuffer(std::ptr::null());
                #[cfg(target_os = "linux")]
                let descriptor = crate::media::types::GpuFrameDescriptor::LinuxHwFrame(std::sync::Arc::new(
                    crate::media::types::HwFrameWrapper {
                        frame: ffmpeg_next::util::frame::Video::empty(),
                    },
                ));
                #[cfg(not(any(target_os = "macos", target_os = "linux")))]
                let descriptor = panic!("Unsupported OS for GPU frame in mock");

                Ok(Some(DecodedFrame::Gpu {
                    frame_idx,
                    descriptor,
                    sequence_id: 0,
                    pts_sec,
                    color_space: ffmpeg_next::color::Space::Unspecified,
                    color_range: ffmpeg_next::color::Range::Unspecified,
                    is_concealed: false,
                    held_frame_idx: None,
                }))
            } else {
                Ok(Some(DecodedFrame::Cpu {
                    frame_idx,
                    planes: std::sync::Arc::new(crate::media::types::YuvPlanes::default()),
                    sequence_id: 0,
                    pts_sec,
                    color_space: ffmpeg_next::color::Space::Unspecified,
                    color_range: ffmpeg_next::color::Range::Unspecified,
                    is_concealed: false,
                    held_frame_idx: None,
                }))
            }
        }

        fn flush(&mut self, fallback_frame_idx: usize) {
            self.fallback_frame_idx = fallback_frame_idx;
        }
    }

    fn run_seek_backward_bug_frame_by_frame(is_gpu: bool) {
        let adapter = Box::new(MockDecoderAdapter {
            fallback_frame_idx: 0,
            has_frame: false,
            is_gpu,
            fail_send: Arc::new(AtomicBool::new(false)),
        });
        let keyframes = vec![0, 50, 100];
        let mut pipeline = VideoDecoderPipeline::new(adapter, keyframes, 24.0, 1.0 / 24.0);

        // 1. Simulate normal playback up to frame 60
        pipeline.play();
        let packet = unsafe { std::mem::zeroed() };
        for i in 0..60 {
            let frames = pipeline.process_packet(&packet).unwrap();
            assert!(!frames.is_empty());
            assert_eq!(frames[0].frame_idx(), i);
        }

        // 2. Pause and then seek backward to frame 49 (cross GOP boundary to force cache miss)
        pipeline.pause();
        // Seek backward to 49. Nearest keyframe is 0.
        let handled = pipeline.seek(49, false, 1, false);
        assert!(!handled, "Should require a hard seek");

        // The first frame decoded from the keyframe position (0) should be physically frame 0.
        // With the fix, the adapter's fallback_frame_idx is correctly reset to 0 on flush,
        // so it continues decoding sequentially and returns frame 49.
        let mut decoded_frames = Vec::new();
        // We need to send packets to drive decoding from 0 to 49.
        for _ in 0..=49 {
            let frames = pipeline.process_packet(&packet).unwrap();
            decoded_frames.extend(frames);
        }
        
        assert!(!decoded_frames.is_empty());
        assert_eq!(decoded_frames.last().unwrap().frame_idx(), 49, "Asserting fixed behavior: seeks backwards correctly");
    }

    #[test]
    fn test_seek_backward_bug_frame_by_frame_cpu() {
        run_seek_backward_bug_frame_by_frame(false);
    }

    #[test]
    fn test_seek_backward_bug_frame_by_frame_gpu() {
        run_seek_backward_bug_frame_by_frame(true);
    }

    fn run_scrub_forward_stuck_bug(is_gpu: bool) {
        let adapter = Box::new(MockDecoderAdapter {
            fallback_frame_idx: 0,
            has_frame: false,
            is_gpu,
            fail_send: Arc::new(AtomicBool::new(false)),
        });
        let keyframes = vec![0, 50, 100];
        let mut pipeline = VideoDecoderPipeline::new(adapter, keyframes, 24.0, 1.0 / 24.0);

        // 1. Play slightly to frame 5
        pipeline.play();
        let packet = unsafe { std::mem::zeroed() };
        for i in 0..5 {
            let frames = pipeline.process_packet(&packet).unwrap();
            assert!(!frames.is_empty());
            assert_eq!(frames[0].frame_idx(), i);
        }

        // 2. Scrub forward to frame 80 (keyframe_only = true)
        pipeline.pause();
        let handled = pipeline.seek(80, true, 2, false);
        assert!(!handled);

        // Demuxer seeks to nearest keyframe (50). The first decoded frame is physically 50.
        // With the fix, the adapter's fallback_frame_idx is reset to 50, so it computes index 50
        // and returns it (keyframe_only is true).
        let frames = pipeline.process_packet(&packet).unwrap();
        assert!(!frames.is_empty());
        assert_eq!(frames[0].frame_idx(), 50, "Asserting fixed behavior: scrubbing returns keyframe 50");
    }

    #[test]
    fn test_scrub_forward_stuck_bug_cpu() {
        run_scrub_forward_stuck_bug(false);
    }

    #[test]
    fn test_scrub_forward_stuck_bug_gpu() {
        run_scrub_forward_stuck_bug(true);
    }

    #[test]
    fn test_corrupt_packet_and_decode_fault_concealment_resilience() {
        let fail_send = Arc::new(AtomicBool::new(false));
        let adapter = Box::new(MockDecoderAdapter {
            fallback_frame_idx: 0,
            has_frame: false,
            is_gpu: false,
            fail_send: fail_send.clone(),
        });
        let keyframes = vec![0, 10, 20];
        let mut pipeline = VideoDecoderPipeline::new(adapter, keyframes, 24.0, 1.0 / 24.0);

        // Play with sequence ID 77
        pipeline.play_unpaced(77);

        // 1. Process valid keyframe packet 0
        let mut key_pkt = ffmpeg_next::Packet::empty();
        key_pkt.set_flags(ffmpeg_next::packet::Flags::KEY);
        let frames = pipeline.process_packet(&key_pkt).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].frame_idx(), 0);
        assert!(!frames[0].is_concealed());
        assert_eq!(frames[0].sequence_id(), 77);
        assert!(!pipeline.is_gop_compromised);
        assert_eq!(pipeline.last_valid_frame.as_ref().unwrap().frame_idx(), 0);

        // 2. Process corrupt packet -> is_gop_compromised becomes true, concealment frame emitted
        let mut corrupt_pkt = ffmpeg_next::Packet::empty();
        corrupt_pkt.set_flags(ffmpeg_next::packet::Flags::CORRUPT);
        let frames = pipeline.process_packet(&corrupt_pkt).unwrap();
        assert!(pipeline.is_gop_compromised, "GOP must be compromised after corrupt packet");
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_concealed(), "Frame emitted during compromised GOP must be concealed");
        assert_eq!(frames[0].held_frame_idx(), Some(0), "Concealment must hold last valid frame index");
        assert_eq!(frames[0].sequence_id(), 77, "Concealment frame must stamp current_sequence_id");

        // 3. Process normal non-keyframe packet -> remains compromised
        let normal_pkt = ffmpeg_next::Packet::empty();
        let frames = pipeline.process_packet(&normal_pkt).unwrap();
        assert!(pipeline.is_gop_compromised);
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_concealed());
        assert_eq!(frames[0].held_frame_idx(), Some(0));
        assert_eq!(frames[0].sequence_id(), 77);

        // 4. Process keyframe packet 10 -> resets is_gop_compromised
        let mut key_pkt_10 = ffmpeg_next::Packet::empty();
        key_pkt_10.set_flags(ffmpeg_next::packet::Flags::KEY);
        let frames = pipeline.process_packet(&key_pkt_10).unwrap();
        assert!(!pipeline.is_gop_compromised, "Keyframe packet must reset is_gop_compromised");
        assert_eq!(frames.len(), 1);
        assert!(!frames[0].is_concealed(), "Keyframe decode must not be concealed");
        assert_eq!(frames[0].held_frame_idx(), None);
        assert_eq!(frames[0].sequence_id(), 77);
        assert_eq!(pipeline.last_valid_frame.as_ref().unwrap().frame_idx(), frames[0].frame_idx());

        // 5. Simulate decode fault on send_packet
        fail_send.store(true, Ordering::SeqCst);
        let frames = pipeline.process_packet(&normal_pkt).unwrap();
        assert!(pipeline.is_gop_compromised, "GOP must be compromised after decode fault");
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_concealed(), "Concealment frame must be emitted on fault");
        assert_eq!(frames[0].held_frame_idx(), Some(1)); // last valid frame from keyframe was index 1
        assert_eq!(frames[0].sequence_id(), 77);

        // 6. Reset adapter fault and send keyframe packet -> resets is_gop_compromised
        fail_send.store(false, Ordering::SeqCst);
        let mut key_pkt_20 = ffmpeg_next::Packet::empty();
        key_pkt_20.set_flags(ffmpeg_next::packet::Flags::KEY);
        let frames = pipeline.process_packet(&key_pkt_20).unwrap();
        assert!(!pipeline.is_gop_compromised, "Keyframe must reset is_gop_compromised");
        assert_eq!(frames.len(), 1);
        assert!(!frames[0].is_concealed());
    }

    #[test]
    fn test_initial_compromised_frame_synthesizes_black() {
        let adapter = Box::new(MockDecoderAdapter {
            fallback_frame_idx: 0,
            has_frame: false,
            is_gpu: false,
            fail_send: Arc::new(AtomicBool::new(false)),
        });
        let keyframes = vec![0, 10];
        let mut pipeline = VideoDecoderPipeline::new(adapter, keyframes, 24.0, 1.0 / 24.0);
        pipeline.play_unpaced(88);

        // Corrupt packet before any valid frames
        let mut corrupt_pkt = ffmpeg_next::Packet::empty();
        corrupt_pkt.set_flags(ffmpeg_next::packet::Flags::CORRUPT);
        let frames = pipeline.process_packet(&corrupt_pkt).unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].is_concealed());
        assert_eq!(frames[0].held_frame_idx(), None);
        assert_eq!(frames[0].sequence_id(), 88);

        if let DecodedFrame::Cpu { planes, color_space, color_range, .. } = &frames[0] {
            assert_eq!(planes.width, 1920);
            assert_eq!(planes.height, 1080);
            assert_eq!(planes.y[0], 16);
            assert_eq!(planes.u[0], 128);
            assert_eq!(planes.v[0], 128);
            assert_eq!(*color_space, ffmpeg_next::color::Space::BT709);
            assert_eq!(*color_range, ffmpeg_next::color::Range::MPEG);
        } else {
            panic!("Expected CPU frame with solid black planes");
        }
    }
}
