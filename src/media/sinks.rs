#![allow(clippy::type_complexity)]
use crate::media::types::DecodedFrame;
use crossbeam_channel::Sender;
use std::collections::VecDeque;

pub trait FrameSink: Send {
    fn push(&mut self, frames: impl IntoIterator<Item = DecodedFrame>);
    fn flush(&mut self);
    fn needs_frames(&self) -> bool;
    fn clear(&mut self);
}

pub struct CrossbeamFrameSink {
    tx: Sender<DecodedFrame>,
    internal_queue: VecDeque<DecodedFrame>,
    target_capacity: usize,
}

impl CrossbeamFrameSink {
    pub fn new(tx: Sender<DecodedFrame>, target_capacity: usize) -> Self {
        Self {
            tx,
            internal_queue: VecDeque::new(),
            target_capacity,
        }
    }
}

impl FrameSink for CrossbeamFrameSink {
    fn push(&mut self, frames: impl IntoIterator<Item = DecodedFrame>) {
        for frame in frames {
            self.internal_queue.push_back(frame);
        }
        self.flush();
    }

    fn flush(&mut self) {
        while self.internal_queue.front().is_some() {
            if self.tx.is_full() {
                break;
            }
            let frame = self.internal_queue.pop_front().unwrap();
            if self.tx.try_send(frame).is_err() {
                break;
            }
        }
    }

    fn needs_frames(&self) -> bool {
        self.internal_queue.is_empty() && self.tx.len() < self.target_capacity
    }

    fn clear(&mut self) {
        self.internal_queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::types::YuvPlanes;
    use crossbeam_channel::bounded;
    use std::sync::Arc;

    #[test]
    fn test_crossbeam_frame_sink_backpressure_and_flush() {
        let (tx, rx) = bounded(2);
        let mut sink = CrossbeamFrameSink::new(tx, 2);

        assert!(sink.needs_frames());

        // Push 3 frames (1 over capacity)
        let dummy_frame = DecodedFrame::Cpu {
            frame_idx: 0,
            planes: Arc::new(YuvPlanes {
                y: vec![], u: vec![], v: vec![],
                width: 0, height: 0, y_stride: 0, u_stride: 0, v_stride: 0, is_10bit: false,
            }),
            sequence_id: 0,
            pts_sec: 0.0,
            color_space: ffmpeg_next::color::Space::Unspecified,
            color_range: ffmpeg_next::color::Range::Unspecified,
        };

        sink.push(vec![dummy_frame.clone(), dummy_frame.clone(), dummy_frame.clone()]);
        
        // Channel can fit 2, 1 is buffered
        assert!(!sink.needs_frames());
        assert_eq!(rx.len(), 2);
        
        // Drain 1 from channel
        rx.recv().unwrap();
        
        // Sink still doesn't need frames because internal queue isn't empty
        assert!(!sink.needs_frames());
        
        // Flush moves the remaining frame to the channel
        sink.flush();
        assert_eq!(rx.len(), 2); // 1 remaining from before + 1 flushed
        
        // Drain both
        rx.recv().unwrap();
        rx.recv().unwrap();
        
        assert!(sink.needs_frames());
    }
}

use crate::media::audio_pipeline::AudioChunk;
use ringbuf::{storage::Heap, traits::*, CachingProd, SharedRb};
use crate::audio::clock::MasterClock;
use std::sync::Arc;

pub trait AudioSink: Send {
    fn push(&mut self, chunks: impl IntoIterator<Item = AudioChunk>);
    fn flush(&mut self);
    fn needs_audio(&self) -> bool;
    fn clear(&mut self);
}

pub struct CrossbeamAudioSink {
    audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
    master_clock: MasterClock,
    // (data, tag, generation)
    internal_queue: VecDeque<(Vec<f32>, Option<(u64, u64)>, u64)>,
    target_vacant: usize,
}

impl CrossbeamAudioSink {
    pub fn new(
        audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
        tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
        master_clock: MasterClock,
        target_vacant: usize,
    ) -> Self {
        Self {
            audio_tx,
            tags_tx,
            master_clock,
            internal_queue: VecDeque::new(),
            target_vacant,
        }
    }
}

impl AudioSink for CrossbeamAudioSink {
    fn push(&mut self, chunks: impl IntoIterator<Item = AudioChunk>) {
        let generation = self.master_clock.playback_generation();
        for chunk in chunks {
            self.internal_queue.push_back((chunk.data, chunk.tag, generation));
        }
        self.flush();
    }

    fn flush(&mut self) {
        while let Some((data, tag, generation)) = self.internal_queue.pop_front() {
            if self.audio_tx.vacant_len() < self.target_vacant {
                self.internal_queue.push_front((data, tag, generation));
                break;
            }

            if let Some(t) = tag {
                let _ = self.tags_tx.try_push((generation, t.0, t.1));
            }

            let mut pushed = 0;
            for &sample in &data {
                if self.audio_tx.try_push((sample, generation)).is_ok() {
                    pushed += 1;
                } else {
                    break;
                }
            }

            if pushed < data.len() {
                let remaining = data[pushed..].to_vec();
                self.internal_queue.push_front((remaining, None, generation));
                break;
            }
        }
    }

    fn needs_audio(&self) -> bool {
        self.internal_queue.is_empty() && self.audio_tx.vacant_len() >= self.target_vacant
    }

    fn clear(&mut self) {
        self.internal_queue.clear();
    }
}


#[cfg(test)]
mod tests_audio {
    use super::*;

    #[test]
    fn test_crossbeam_audio_sink() {
        let (master_clock, _ticker) = MasterClock::new(48000);
        let audio_rb = SharedRb::<Heap<(f32, u64)>>::new(10);
        let (audio_tx, mut _audio_rx) = audio_rb.split();
        let tags_rb = SharedRb::<Heap<(u64, u64, u64)>>::new(10);
        let (tags_tx, mut _tags_rx) = tags_rb.split();

        let mut sink = CrossbeamAudioSink::new(audio_tx, tags_tx, master_clock, 5);

        assert!(sink.needs_audio());

        // Push 6 samples, 1 over the target vacant threshold of 5
        let chunk = AudioChunk {
            data: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
            tag: Some((0, 0)),
        };

        sink.push(vec![chunk]);

        assert!(!sink.needs_audio());
        
        while _audio_rx.try_pop().is_some() {}
        while _tags_rx.try_pop().is_some() {}

        sink.clear();
        assert!(sink.needs_audio());
    }
}

