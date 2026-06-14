# Extract FrameSink and AudioSink Adapters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract `FrameSink` and `AudioSink` adapter seams inside the Decoder Thread to eliminate nested blocking control flow, enabling non-allocating buffering, perfect testability, and massive locality improvements.

**Architecture:** We are replacing the raw `Vec` based `pending_video` and `pending_audio` overflow buffers and manual channel `try_send` logic inside `run_decoder_thread` with two focused adapter traits (`FrameSink` and `AudioSink`). These use `VecDeque` internally for $O(1)$ shifts.

**Tech Stack:** Rust, crossbeam-channel, ringbuf.

---

### Task 1: FrameSink Interface & Implementation

**Files:**
- Create: `src/media/sinks.rs`
- Modify: `src/media/mod.rs` (to export sinks)

- [ ] **Step 1: Export `sinks` module in `mod.rs`**

Modify `src/media/mod.rs` to add `pub mod sinks;` and `pub use crate::media::sinks::*;`. Add this right below `pub mod cache;`.

- [ ] **Step 2: Write the failing test for `CrossbeamFrameSink`**

Create `src/media/sinks.rs` and write the initial failing test:

```rust
use crate::media::types::{DecodedFrame, YuvPlanes};
use crossbeam_channel::{unbounded, bounded};
use std::collections::VecDeque;

pub trait FrameSink: Send {
    fn push(&mut self, frames: impl IntoIterator<Item = DecodedFrame>);
    fn flush(&mut self);
    fn needs_frames(&self) -> bool;
    fn clear(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;
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
            color_space: ffmpeg_next::format::ColorSpace::Unspecified,
            color_range: ffmpeg_next::format::ColorRange::Unspecified,
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
        
        // Now it needs frames
        assert!(sink.needs_frames());
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test test_crossbeam_frame_sink_backpressure_and_flush`
Expected: FAIL because `CrossbeamFrameSink` doesn't exist.

- [ ] **Step 4: Write minimal implementation**

Add to `src/media/sinks.rs`:

```rust
use crossbeam_channel::Sender;

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
        while let Some(frame) = self.internal_queue.front() {
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
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_crossbeam_frame_sink_backpressure_and_flush`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/media/mod.rs src/media/sinks.rs
git commit -m "feat: implement FrameSink interface and CrossbeamFrameSink adapter"
```

### Task 2: AudioSink Interface & Implementation

**Files:**
- Modify: `src/media/sinks.rs`

- [ ] **Step 1: Write the failing test for `CrossbeamAudioSink`**

Add to `src/media/sinks.rs`:

```rust
use crate::media::audio_pipeline::AudioChunk;
use ringbuf::{storage::Heap, traits::*, CachingProd, SharedRb};
use crate::audio::clock::MasterClock;

pub trait AudioSink: Send {
    fn push(&mut self, chunks: impl IntoIterator<Item = AudioChunk>);
    fn flush(&mut self);
    fn needs_audio(&self) -> bool;
    fn clear(&mut self);
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
        sink.clear();
        assert!(sink.needs_audio());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_crossbeam_audio_sink`
Expected: FAIL because `CrossbeamAudioSink` doesn't exist.

- [ ] **Step 3: Write minimal implementation**

Add to `src/media/sinks.rs`:

```rust
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
        let gen = self.master_clock.playback_generation();
        for chunk in chunks {
            self.internal_queue.push_back((chunk.data, chunk.tag, gen));
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
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_crossbeam_audio_sink`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/media/sinks.rs
git commit -m "feat: implement AudioSink and CrossbeamAudioSink"
```

### Task 3: Refactor Decoder Thread

**Files:**
- Modify: `src/media/mod.rs` (lines ~487-630)

- [ ] **Step 1: Refactor `run_decoder_thread` to use the Sinks**

In `src/media/mod.rs`, locate `run_decoder_thread`.

1. Remove the old buffer variables:
```rust
// Remove these:
// let mut pending_video = Vec::new();
// let mut pending_audio = Vec::new();
```

2. Instantiate the new Sinks instead:
```rust
    let mut video_sink = crate::media::sinks::CrossbeamFrameSink::new(play_queue_tx, 20);
    let mut audio_sink_opt = if has_audio_stream {
        Some(crate::media::sinks::CrossbeamAudioSink::new(audio_tx, tags_tx, master_clock.clone(), 2048))
    } else {
        None
    };
```

3. At the top of the `loop {`, replace the manual flush logic:
```rust
    loop {
        // Replace the `while !pending_video.is_empty()` and `while !pending_audio.is_empty()` blocks with:
        video_sink.flush();
        if let Some(sink) = audio_sink_opt.as_mut() {
            sink.flush();
        }
```

4. In the `MediaCommand::Seek` handler, replace `.clear()` calls:
```rust
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
                    // ...
                    // Replace pending_video.clear() / pending_audio.clear() with:
                    video_sink.clear();
                    if let Some(sink) = audio_sink_opt.as_mut() {
                        sink.clear();
                    }
```

5. In the decode logic (the `if video_pipeline.is_active()` block), replace the manual checks and `.push()` with Sink calls:
```rust
        if video_pipeline.is_active() {
            let audio_needs_frames = if let (true, Some(sink)) = (audio_enabled, audio_sink_opt.as_ref()) {
                sink.needs_audio()
            } else {
                false
            };
            
            let queue_needs_frames = video_sink.needs_frames() || audio_needs_frames;
            
            if queue_needs_frames {
                if let Some(packet_res) = packets_iter.next() {
                    let (stream, packet) = packet_res;
                    let stream_index = stream.index();
                    if stream_index == video_stream_index {
                        if let Ok(frames) = video_pipeline.process_packet(&packet) {
                            video_sink.push(frames);
                        }
                    } else if Some(stream_index) == audio_stream_index_opt && audio_enabled {
                        if let Some(audio_pipeline) = audio_pipeline_opt.as_mut() {
                            if let Some(pts) = packet.pts() {
                                audio_pipeline.update_current_time_from_pts(pts);
                            }
                            if audio_pipeline.send_packet(&packet).is_ok() {
                                if let Ok(chunks) = audio_pipeline.receive_samples() {
                                    if let Some(sink) = audio_sink_opt.as_mut() {
                                        sink.push(chunks);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
```

- [ ] **Step 2: Check for compilation errors**

Run: `cargo check`
Fix any lifetime, mutability, or missing import issues. Note: `audio_tx` and `tags_tx` were originally mutable, you may need to drop `mut` from the function signature since they are moved into `CrossbeamAudioSink`. `play_queue_tx` is moved into `CrossbeamFrameSink`.

- [ ] **Step 3: Run the full test suite**

Run: `cargo test`
Expected: PASS. The behavior should be identical but massive chunks of code are removed from the loop.

- [ ] **Step 4: Commit**

```bash
git add src/media/mod.rs
git commit -m "refactor: use FrameSink and AudioSink inside run_decoder_thread"
```
