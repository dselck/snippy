# Precise Seek Catch-up on Pause Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the frame jump to keyframe that happens when a video is paused or when seeking while paused, and fix the ~900ms video frame lead when pausing playback by preventing lookahead frames from being forwarded when paused and seeking precisely to the paused audio time on transition.

**Architecture:** 
1. Modify `video_pipeline.pause()` to keep `decode_more = true` when `seek_target_frame` is active (Completed).
2. Update the Media Engine thread loop to track `seek_target_frame_idx` and conditionally forward frames (Completed).
3. Update `set_playing` in `src/ui/mod.rs` to perform a precise seek to the exact paused time when transitioning from playing to paused.
4. Run integration tests to verify everything passes.

**Tech Stack:** Rust, egui, FFmpeg.

---

### Task 1: Update Video Pipeline Pause Behavior (Completed)

**Files:**
- Modify: `src/media/decoder_pipeline.rs:68-72`

- [x] **Step 1: Modify `pause` in `VideoDecoderPipeline`**
  Completed.

---

### Task 2: Update Media Engine Loop with Seek Tracking and Conditional Forwarding (Completed)

**Files:**
- Modify: `src/media/mod.rs`

- [x] **Step 1: Update Media Engine loop**
  Completed.

---

### Task 3: Precise Seek to Paused Audio Time on Play-to-Pause Transition

**Files:**
- Modify: `src/ui/mod.rs:214-231`

- [ ] **Step 1: Update `set_playing` in `MainApp`**
  Open `src/ui/mod.rs` and locate `set_playing` method. Update it to:
  1. Capture if we were currently playing.
  2. If transitioning from playing to paused (`was_playing` is true), pause the audio master clock, calculate the precise frame index corresponding to the pause time, and call `self.seek_internal(pause_frame, false, true)` before sending the `Pause` command to the Media Engine.
  
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

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): precisely seek to paused audio time on play-to-pause transition"
  ```

---

### Task 4: Run Integration Tests and Verify

**Files:**
- Test: `src/bin/test_playback_sync.rs`

- [ ] **Step 1: Run integration tests**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS (showing "All Tests Passed Successfully")
