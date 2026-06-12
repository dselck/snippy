use ffmpeg_next::Packet;
use crate::media::types::DecodedFrame;
use crate::media::cache::{GopCache, CachedFrame};

#[derive(Debug)]
pub enum DecodeError {
    EndOfStream,
    Fault(String),
}

pub trait DecoderAdapter {
    fn send_packet(&mut self, packet: &Packet) -> Result<(), DecodeError>;
    fn receive_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError>;
    fn flush(&mut self);
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

    fn get_nearest_keyframe(keyframes: &[usize], frame_idx: usize) -> usize {
        match keyframes.binary_search(&frame_idx) {
            Ok(_) => frame_idx,
            Err(pos) => {
                if pos == 0 {
                    0
                } else {
                    keyframes[pos - 1]
                }
            }
        }
    }

    pub fn play(&mut self) {
        self.play_mode = true;
        self.seek_target_frame = None;
        self.keyframe_only_mode = false;
        self.decode_more = true;
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

    pub fn current_sequence_id(&self) -> usize {
        self.current_sequence_id
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
        let target_kf = Self::get_nearest_keyframe(&self.keyframes, frame_index);
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
        
        self.adapter.flush();
        
        self.decode_more = true;
        self.last_decoded_frame = None;
        false
    }

    pub fn get_cached_frame(&self, frame_index: usize) -> Option<DecodedFrame> {
        if let Some(cached) = self.gop_cache.get_cached(frame_index) {
            Some(DecodedFrame::Cpu {
                frame_idx: frame_index,
                planes: cached.planes.clone(),
                sequence_id: self.current_sequence_id,
                pts_sec: cached.pts_sec,
            })
        } else {
            None
        }
    }
    
    /// Processes a video packet, returning any frames that are ready to be displayed.
    pub fn process_packet(&mut self, packet: &Packet) -> Result<Vec<DecodedFrame>, DecodeError> {
        if !self.decode_more {
            return Ok(Vec::new());
        }

        self.adapter.send_packet(packet)?;
        
        let mut frames = Vec::new();

        while let Some(decoded) = self.adapter.receive_frame()? {
            let (frame_idx, pts_sec, planes) = match decoded {
                DecodedFrame::Cpu { frame_idx, pts_sec, planes, .. } => (frame_idx, pts_sec, planes),
                _ => unreachable!(),
            };

            let final_frame = DecodedFrame::Cpu {
                frame_idx,
                planes: planes.clone(),
                sequence_id: self.current_sequence_id,
                pts_sec,
            };

            // GOP Cache population
            if self.keyframes.binary_search(&frame_idx).is_ok() {
                self.gop_cache.clear_if_different_gop(frame_idx);
            }
            if self.gop_cache.keyframe_idx == usize::MAX {
                let target_kf = Self::get_nearest_keyframe(&self.keyframes, frame_idx);
                self.gop_cache.clear_if_different_gop(target_kf);
            }
            self.gop_cache.insert_frame(frame_idx, CachedFrame { planes: planes.clone(), pts_sec });

            self.last_decoded_frame = Some(frame_idx);

            if !self.play_mode {
                if let Some(target) = self.seek_target_frame {
                    if frame_idx >= target {
                        self.decode_more = false;
                        self.seek_target_frame = None;
                        frames.push(final_frame);
                        break;
                    }
                }
            } else {
                frames.push(final_frame.clone());
            }

            if self.keyframe_only_mode && !self.play_mode {
                self.decode_more = false;
                frames.push(final_frame);
                break;
            }
        }
        
        Ok(frames)
    }
}
