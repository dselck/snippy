# Symmetrical Sync on Pause Design Spec

This document details the design for resolving the video frame jump issue and eliminating the pause delay in Snippy2 by preserving the lookahead queue and keeping the sync engine active when paused.

---

## 1. Problem Description

Previously, when pausing playback:
1. The UI thread immediately paused the audio master clock: `ae.master_clock.pause()`, freezing its time at $t_{pause}$.
2. In the Media Engine loop, when the master clock paused, the loop bypassed `sync_engine.evaluate_frame` and returned `SyncAction::Present` immediately for all remaining frames.
3. This caused the Media Engine thread to dump the entire lookahead buffer (up to 20 frames, or ~830ms-1s) to the UI immediately, creating a visual jump and leaving the video leading the master clock by up to 1 second.
4. To work around this, we previously had to drain the lookahead queue, perform an expensive seek in FFmpeg, and re-decode all frames up to the pause point. This caused up to a 1-second delay when pausing/scrubbing, and threw away valuable decoded frames.

---

## 2. Proposed Changes

### A. Keep Sync Engine Active During Playback Pause (`src/media/mod.rs`)

Modify the playback frame presentation block to always evaluate the frame against the master clock, even if the master clock is paused. This ensures that any frames with `pts_sec > t_pause` will return `SyncAction::Wait`, stalling the video exactly at the pause point.

```diff
-                let action = if master_clock.is_playing() {
-                    sync_engine.evaluate_frame(pts_sec)
-                } else {
-                    Ok(SyncAction::Present)
-                };
+                let action = sync_engine.evaluate_frame(pts_sec);
```

### B. Preserve Lookahead Queue on Pause (`src/media/mod.rs`)

When `MediaCommand::Pause` is received, set `playing = false` but do not clear `current_frame` or drain `play_queue_rx`. This preserves the lookahead buffer in CPU RAM.

```rust
                MediaCommand::Pause => {
                    playing = false;
                    play_after_seek = false;
                    let _ = decoder_cmd_tx.send(MediaCommand::Pause);
                }
```

Modify the idle frame-fetch block in `run_decoding_loop` to only read from `play_queue_rx` if we are actively seeking (`seek_target_frame_idx.is_some()`) or catch-up is active (`play_after_seek` is true). When simply paused, the queue remains untouched.

```rust
            } else if seek_target_frame_idx.is_some() || play_after_seek {
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

### C. Revert UI Precise Seek on Pause (`src/ui/mod.rs`)

Revert `set_playing` back to its original clean implementation. The Media Engine now natively handles paused frame alignment using the Master Clock, so the UI thread does not need to issue any seek commands when pausing playback.

```rust
    fn set_playing(&mut self, play: bool) {
        if let Some(tx) = &self.media_tx {
            let cmd = if play {
                media::MediaCommand::Play
            } else {
                media::MediaCommand::Pause
            };
            let _ = tx.send(cmd);
            self.is_playing = play;
        }
        if let Some(ae) = &self.audio_engine {
            if play {
                ae.master_clock.play();
            } else {
                ae.master_clock.pause();
            }
        }
    }
```

---

## 3. Verification Plan

* **Unit Tests**: Run `cargo test` to verify all existing playback and master clock tests pass.
* **Integration Tests**: Run `cargo run --bin test_playback_sync` to ensure integration tests pass, including:
  * Precise seek catch-up while paused.
  * Play-to-pause transition frame leak checks.
