# Precise Seek Catch-up on Pause Design Spec

This document details the design for resolving the video frame jump issue when pausing playback or seeking while paused in Snippy2.

---

## 1. Problem Description

There are two primary scenarios where the video frame jumps:
1. **Jump to Keyframe when Pausing after Scrubbing**: When the user drags/clicks to seek while paused, the UI sends a precise `Seek` command followed immediately by a `Pause` command. When the video decoder thread receives the `Pause` command, it calls `video_pipeline.pause()`, which unconditionally sets `decode_more = false` and clears `seek_target_frame`. This stops the precise seek catch-up immediately, leaving the player on the keyframe.
2. **Video Lead by ~900ms when Pausing Playback**: During active playback, the video decoder thread decodes ahead up to 20 frames (approx. 830ms at 24fps) to keep a lookahead buffer filled. When the user clicks play/pause to pause playback, the Media Engine thread sets `playing = false` and drains the play queue. However, the decoder thread concurrently finishes decoding its current packets and pushes additional lookahead frames to the queue before processing the `Pause` command. Since `playing` is now `false`, the Media Engine thread unconditionally forwards any newly received frames in the queue to the UI, causing the video frame to jump forward by ~900ms relative to the paused audio master clock.
3. **AV Sync Disconnect on Resume**: In play-to-pause transitions, the audio clock stops immediately at time $T$. However, the UI drains lookahead frames from the channel, leaving the displayed video frame at $T + 900\text{ ms}$. When playback is resumed, the video stays frozen waiting for the audio clock to catch up by 900ms.

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

Introduce a tracking variable `seek_target_frame_idx: Option<usize>` in `run_decoding_loop` to keep track of active seeks.

1. **Modify `MediaCommand::Seek`**: Set `seek_target_frame_idx = Some(frame_index)`.
2. **Modify `MediaCommand::Pause`**: Do not unconditionally reset the play queue if not playing, and do not clear `seek_target_frame_idx` (allowing the seek to complete catch-up).
3. **Conditionally Forward Frames when Paused**: When `playing` is `false`, only forward frames from `play_queue_rx` to the UI if `seek_target_frame_idx` is `Some(...)` (meaning we are actively seeking). If it is `None`, discard the frames.

```rust
            } else {
                while let Ok(frame) = play_queue_rx.try_recv() {
                    let seq = frame.sequence_id();
                    if let Some(target_idx) = seek_target_frame_idx {
                        if seq == latest_seq {
                            let f_idx = frame.frame_idx();
                            let _ = frame_tx.try_send(frame);
                            if f_idx >= target_idx {
                                seek_target_frame_idx = None;
                            }
                        }
                    }
                    if play_after_seek && seq == latest_seq {
                        playing = true;
                        play_after_seek = false;
                        sync_engine.reset();
                        let _ = decoder_cmd_tx.send(MediaCommand::Play);
                        break;
                    }
                }
            }
```

### C. UI Thread State Alignment (`src/ui/mod.rs`)

When pausing active playback in the UI thread (`set_playing(false)`), query the exact time the audio clock stopped at, calculate the corresponding frame index, and issue a precise `Seek` command to that frame index right before sending the `Pause` command. This aligns the video frame perfectly with the audio master clock at the pause point.

```rust
    fn set_playing(&mut self, play: bool) {
        if let Some(tx) = &self.media_tx {
            if play {
                let _ = tx.send(media::MediaCommand::Play);
                self.is_playing = true;
                if let Some(ae) = &self.audio_engine {
                    ae.master_clock.play();
                }
            } else {
                let was_playing = self.is_playing;
                self.is_playing = false;
                if let Some(ae) = &self.audio_engine {
                    ae.master_clock.pause();
                }
                
                if was_playing {
                    let mut pause_time = 0.0;
                    if let Some(ae) = &self.audio_engine {
                        pause_time = ae.master_clock.current_time();
                    }
                    let pause_frame = (pause_time * self.video_fps).round() as usize;
                    self.seek_internal(pause_frame, false, true);
                }
                
                let _ = tx.send(media::MediaCommand::Pause);
            }
        }
    }
```

---

## 3. Verification Plan

* **Unit Tests**: Run `cargo test` to verify all existing playback and master clock tests pass.
* **Integration Tests**: Run `cargo run --bin test_playback_sync` to ensure synchronization integration tests pass, including the new paused precise seek and pause jump tests.
