# Design Spec: Lookahead Pre-decoding to Prevent GOP Boundary Spikes

To eliminate video playback stutters at GOP (Group of Pictures) boundaries by decoupling frame decoding from real-time presentation timing using a single-threaded lookahead play queue.

## Problem Statement
During sequential playback, the background Media Engine thread blocks and sleeps until the *current* frame's presentation timestamp (PTS) matches the `MasterClock`. Only after presenting a frame does it demux and decode the next frame. When the playhead crosses a GOP boundary (every 139 frames on HEVC 10-bit), the decoder must perform a cold decode of the next keyframe. This results in a **~55ms pause**, causing visible stuttering because decoding is not performed in advance.

## Proposed Solution
Decouple decoding from presentation by introducing an internal `play_queue` (`std::collections::VecDeque`) in the Media Engine thread.
1. The thread will check if the front frame of the queue is due for presentation. If it is, it pops and sends the frame to the UI immediately. If not, it does not sleep or block.
2. If the queue has fewer than `MAX_LOOKAHEAD_FRAMES` (e.g., 15 frames, representing ~600ms of lookahead), the thread demuxes and decodes the next packets from FFmpeg, pushing them to the back of the queue.
3. If the queue is full or paused, the thread sleeps briefly (2ms) to avoid CPU hot looping.
4. When a seek occurs, the queue is cleared.

This allows the decoding loop to work ahead. By the time a keyframe is reached, it has already been decoded and buffered in the queue, eliminating the 55ms playback stutter.

---

## Detailed Changes

### `src/media/mod.rs`
Update the `MediaEngine` background thread loop.

1. **Import VecDeque**:
   ```rust
   use std::collections::VecDeque;
   ```

2. **Initialize Play Queue**:
   Before the loop:
   ```rust
   let mut play_queue: VecDeque<DecodedFrame> = VecDeque::new();
   const MAX_LOOKAHEAD_FRAMES: usize = 15;
   ```

3. **Flush Play Queue on Seek**:
   ```rust
   MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
       audio_enabled = ae;
       play_queue.clear(); // Clear the lookahead buffer
       // ...
   ```

4. **Update Demux and Presentation Loops**:
   Rewrite the main `loop` to process commands, check the lookahead queue for due frames, and decode packets when the queue has capacity.

---

## Verification Plan

### Automated Tests
Run unit tests to ensure no regressions:
```bash
cargo test
```

### Profiling and Benchmarking
Verify that GOP boundary intervals no longer spike:
```bash
SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv" cargo run --release --bin bench_decode
```

Compare the **GOP boundary intervals** results:
- Baseline: Spikes up to 26ms (or 55ms in the profile harness) were recorded.
- Expected: No spikes (maximum frame interval stays under 2× the median, i.e., <25ms, and 0 spikes detected).
