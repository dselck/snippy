# Precise Seek Catch-up on Pause Design Spec

This document details the design for resolving the video frame jump issue when pausing playback or seeking while paused in Snippy2.

---

## 1. Problem Description

When a seek is performed while the video is paused (e.g. when dragging and releasing the timeline progress bar, or clicking on a timeline thumbnail/waveform), the video jumps or snaps back to the nearest keyframe instead of stopping at the exact target frame.

This behavior is caused by two overlapping issues in the decoder pipeline and media engine command flow:
1. **Decoder Thread Aborts Seek Catch-up**: The UI thread sends a `Seek` command followed immediately by a `Pause` command. When the video decoder thread receives the `Pause` command, it calls `video_pipeline.pause()`, which unconditionally sets `decode_more = false` and clears `seek_target_frame`. This stops the precise seek catch-up immediately, leaving the player on the keyframe it started decoding from.
2. **Media Engine Drains Queue**: In the media engine thread (`src/media/mod.rs`), receiving `Pause` unconditionally drains the decoded frame queue `play_queue_rx`. If the decoder thread managed to decode the target frame before stopping, it gets discarded here, preventing the UI from rendering it.

---

## 2. Proposed Changes

### A. Video Pipeline (`src/media/decoder_pipeline.rs`)

Modify `video_pipeline.pause()` to keep `decode_more` active if a catch-up seek is currently in progress. This ensures the decoder thread will complete decoding up to the target frame and then pause automatically.

```rust
    pub fn pause(&mut self) {
        self.play_mode = false;
        if self.seek_target_frame.is_none() {
            self.decode_more = false;
        }
    }
```

### B. Media Engine Thread Loop (`src/media/mod.rs`)

Modify the `MediaCommand::Pause` match arm so that it only drains the play queue and resets the current frame if the video was actually playing. If we are already paused/seeking, we preserve the play queue and current frame so the seek target frame isn't discarded.

```rust
                MediaCommand::Pause => {
                    if playing {
                        playing = false;
                        current_frame = None;
                        while play_queue_rx.try_recv().is_ok() {}
                    }
                    play_after_seek = false;
                    let _ = decoder_cmd_tx.send(MediaCommand::Pause);
                }
```

---

## 3. Verification Plan

* **Unit Tests**: Run `cargo test` to verify all existing playback and master clock tests pass.
* **Integration Tests**: Run `cargo run --bin test_playback_sync` to ensure synchronization integration tests pass.
