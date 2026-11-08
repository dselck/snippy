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
}

impl VideoDecoderPipeline {
    pub fn new(adapter: Box<dyn DecoderAdapter>, keyframes: Vec<usize>) -> Self {
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
    
    /// Processes a video packet, returning any frames that are ready to be displayed.
    pub fn process_packet(&mut self, packet: &Packet) -> Result<Vec<DecodedFrame>, DecodeError> {
        if !self.decode_more {
            return Ok(Vec::new());
        }

        self.adapter.send_packet(packet)?;
        
        let mut frames = Vec::new();

        while let Some(mut decoded) = self.adapter.receive_frame()? {
            match &mut decoded {
                DecodedFrame::Cpu { sequence_id, .. } => {
                    *sequence_id = self.current_sequence_id;
                }
                DecodedFrame::Gpu { sequence_id, .. } => {
                    *sequence_id = self.current_sequence_id;
                }
            }

            let frame_idx = decoded.frame_idx();
            let _pts_sec = decoded.pts_sec();

            // GOP Cache population: limit cache to the current GOP to prevent unbounded memory growth
            let nearest_kf = crate::media::types::get_nearest_keyframe(&self.keyframes, frame_idx);
            self.gop_cache.clear_if_different_gop(nearest_kf);
            self.gop_cache.insert_frame(frame_idx, decoded.clone());

            self.last_decoded_frame = Some(frame_idx);

            if !self.play_mode {
                if let Some(target) = self.seek_target_frame {
                    if frame_idx >= target {
                        self.decode_more = false;
                        self.seek_target_frame = None;
                        frames.push(decoded);
                        break;
                    }
                }
            } else {
                frames.push(decoded.clone());
            }

            if self.keyframe_only_mode && !self.play_mode {
                self.decode_more = false;
                frames.push(decoded);
                break;
            }
        }
        
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockDecoderAdapter {
        fallback_frame_idx: usize,
        has_frame: bool,
        is_gpu: bool,
    }

    impl DecoderAdapter for MockDecoderAdapter {
        fn send_packet(&mut self, _packet: &ffmpeg_next::Packet) -> Result<(), DecodeError> {
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
                }))
            } else {
                Ok(Some(DecodedFrame::Cpu {
                    frame_idx,
                    planes: std::sync::Arc::new(crate::media::types::YuvPlanes::default()),
                    sequence_id: 0,
                    pts_sec,
                    color_space: ffmpeg_next::color::Space::Unspecified,
                    color_range: ffmpeg_next::color::Range::Unspecified,
                }))
            }
        }

        fn flush(&mut self, fallback_frame_idx: usize) {
            self.fallback_frame_idx = fallback_frame_idx;
        }
    }

    fn run_seek_backward_bug_frame_by_frame(is_gpu: bool) {
        let adapter = Box::new(MockDecoderAdapter { fallback_frame_idx: 0, has_frame: false, is_gpu });
        let keyframes = vec![0, 50, 100];
        let mut pipeline = VideoDecoderPipeline::new(adapter, keyframes);

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
        let adapter = Box::new(MockDecoderAdapter { fallback_frame_idx: 0, has_frame: false, is_gpu });
        let keyframes = vec![0, 50, 100];
        let mut pipeline = VideoDecoderPipeline::new(adapter, keyframes);

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
}
