# Precise Seek Catch-up on Pause Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the frame jump to keyframe that happens when a video is paused or when seeking while paused, by allowing in-progress precise seeks to complete catch-up decoding before the video pipeline pauses, and preventing the Media Engine from draining the queue unless it was actually playing.

**Architecture:** Modify `video_pipeline.pause()` to keep `decode_more = true` when `seek_target_frame` is active, and update `MediaCommand::Pause` handling in the Media Engine loop to conditionally drain `play_queue_rx` only when the player was playing. Add an integration test case verifying that seeking while paused resolves to the precise target frame.

**Tech Stack:** Rust, egui, FFmpeg.

---

### Task 1: Update Video Pipeline Pause Behavior

**Files:**
- Modify: `src/media/decoder_pipeline.rs:68-72`

- [ ] **Step 1: Modify `pause` in `VideoDecoderPipeline`**
  Open `src/media/decoder_pipeline.rs` and update the `pause` method to only set `decode_more = false` if there is no active seek in progress (`seek_target_frame.is_none()`):
  
  ```rust
      pub fn pause(&mut self) {
          self.play_mode = false;
          if self.seek_target_frame.is_none() {
              self.decode_more = false;
          }
      }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/media/decoder_pipeline.rs
  git commit -m "feat(media): prevent pause from aborting active seek in video pipeline"
  ```

---

### Task 2: Update Media Engine Command Handling for Pause

**Files:**
- Modify: `src/media/mod.rs:206-212`

- [ ] **Step 1: Modify `MediaCommand::Pause` handling**
  Open `src/media/mod.rs` and update the `MediaCommand::Pause` branch in `run_decoding_loop` to only drain the play queue and clear `current_frame` if `playing` was true:

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

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat(media): conditionally drain play queue on pause only if playing"
  ```

---

### Task 3: Add Integration Test Case and Verify

**Files:**
- Modify: `src/bin/test_playback_sync.rs`

- [ ] **Step 1: Add a test case for seeking while paused**
  Open `src/bin/test_playback_sync.rs` and add the new test scenario before the `=== All Tests Passed Successfully ===` message (around line 245):

  ```rust
      // 7. Test seeking while paused (precise seek followed immediately by pause)
      println!("\n--- Test precise seek while paused: target_frame=45 ---");
      // Seek to target frame 45 (which is within GOP starting at keyframe 30)
      me.tx.send(MediaCommand::Seek { frame_index: 45, keyframe_only: false, sequence_id: 5, audio_enabled: false })?;
      // Immediately send Pause command
      me.tx.send(MediaCommand::Pause)?;

      // We expect to eventually receive frame 45 (precise target), not just keyframe 30.
      let mut received_target = false;
      let start_time = std::time::Instant::now();
      while start_time.elapsed() < Duration::from_secs(5) {
          if let Ok(frame) = frame_rx.recv_timeout(Duration::from_millis(100)) {
              if frame.sequence_id() == 5 {
                  println!("Received paused seek frame: index={}", frame.frame_idx());
                  if frame.frame_idx() == 45 {
                      received_target = true;
                      break;
                  }
              }
          }
      }
      assert!(received_target, "Should receive precise seek target frame 45 when paused, not stop at keyframe!");
  ```

- [ ] **Step 2: Correct the existing out-of-bounds target in the test file**
  Wait! The existing test has `let exact_target = 340;` which is out of bounds for the 120-frame synthetic video. Let's change `exact_target` to `50` (or similar valid frame index) to make sure the existing seek-after-play test passes successfully!
  
  Modify lines 143-146 in `src/bin/test_playback_sync.rs`:
  ```rust
      let exact_target = 50;
      println!("\n--- Test Exact Seek directly after Play: target_frame={} ---", exact_target);
      master_clock.reset(exact_target as f64 / metadata.fps);
      me.tx.send(MediaCommand::Seek { frame_index: exact_target, keyframe_only: false, sequence_id: 2, audio_enabled: true })?;
  ```

- [ ] **Step 3: Run integration tests**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS (showing "All Tests Passed Successfully")

- [ ] **Step 4: Commit**
  ```bash
  git add src/bin/test_playback_sync.rs
  git commit -m "test: add integration test for paused seek catch-up and correct seek bounds"
  ```
