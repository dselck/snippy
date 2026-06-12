# Precise Seek Catch-up on Pause Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the frame jump to keyframe that happens when a video is paused or when seeking while paused, and fix the ~900ms video frame lead when pausing playback by preventing lookahead frames from being forwarded when paused.

**Architecture:** 
1. Modify `video_pipeline.pause()` to keep `decode_more = true` when `seek_target_frame` is active.
2. Update the Media Engine thread loop to track `seek_target_frame_idx`. Update `Seek` and `Pause` command handling to set/clear it.
3. Update the idle frame-fetch block in `src/media/mod.rs` to conditionally forward frames only if `seek_target_frame_idx` is `Some(...)`, and break the loop if `playing` is set to `true`.
4. Add integration tests verifying precise seek completes when paused and play-to-pause does not leak lookahead frames.

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

### Task 2: Update Media Engine Loop with Seek Tracking and Conditional Forwarding

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Add `seek_target_frame_idx` tracking variable**
  Locate the local variable declarations at the start of `run_decoding_loop` (around line 185) and add `seek_target_frame_idx`:
  
  ```rust
      let mut playing = false;
      let mut play_after_seek = false;
      let mut current_frame: Option<DecodedFrame> = None;
      let mut latest_seq = 0usize;
      let mut wait_duration: Option<std::time::Duration> = None;
      let mut seek_target_frame_idx: Option<usize> = None;
  ```

- [ ] **Step 2: Update `MediaCommand::Seek` handling**
  Locate `MediaCommand::Seek` match arm (around line 213) and set `seek_target_frame_idx = Some(frame_index)`:
  
  ```rust
                  MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled } => {
                      play_after_seek = playing || play_after_seek;
                      latest_seq = sequence_id;
                      playing = false;
                      current_frame = None;
                      seek_target_frame_idx = Some(frame_index);
                      while play_queue_rx.try_recv().is_ok() {}
                      let _ = decoder_cmd_tx.send(MediaCommand::Seek {
                          frame_index,
                          keyframe_only,
                          sequence_id,
                          audio_enabled,
                      });
                  }
  ```

- [ ] **Step 3: Update `MediaCommand::Pause` handling**
  Locate `MediaCommand::Pause` match arm (around line 206) and modify it to only clear `current_frame` and drain `play_queue_rx` when `playing` is true, and clear `play_after_seek`:

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

- [ ] **Step 4: Update the idle frame-fetch `else` block**
  Locate the `else` block under `// 2. Fetch frame if needed` (around line 235) and modify it to conditionally forward frames only when seeking, and break the loop if `playing` becomes true:

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

- [ ] **Step 5: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 6: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat(media): track seek targets and conditionally forward frames when paused"
  ```

---

### Task 3: Add Integration Test Cases and Verify

**Files:**
- Modify: `src/bin/test_playback_sync.rs`

- [ ] **Step 1: Correct search bounds and add paused seek test case**
  Open `src/bin/test_playback_sync.rs` and make sure `exact_target` is set to `50` (or similar valid frame index) and `double_seek_target` is `80`. Ensure the paused seek test case is present:

  ```rust
      // 7. Test seeking while paused (precise seek followed immediately by pause)
      println!("\n--- Test precise seek while paused: target_frame=45 ---");
      me.tx.send(MediaCommand::Seek { frame_index: 45, keyframe_only: false, sequence_id: 5, audio_enabled: false })?;
      me.tx.send(MediaCommand::Pause)?;

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

- [ ] **Step 2: Add play-to-pause lookahead leak test case**
  Add the test scenario for play-to-pause video frame lead check right after the previous test case:

  ```rust
      // 8. Test play-to-pause transition frame leak
      println!("\n--- Test play-to-pause transition: verify video does not lead after pause ---");
      me.tx.send(MediaCommand::Play)?;
      
      // Let it play for 300ms
      thread::sleep(Duration::from_millis(300));
      
      // Drain frame_rx and capture the latest frame index before pause
      let mut last_playing_frame = 0;
      while let Ok(frame) = frame_rx.try_recv() {
          last_playing_frame = frame.frame_idx();
      }
      println!("Pausing playback. Last playing frame received: {}", last_playing_frame);
      
      // Send Pause
      me.tx.send(MediaCommand::Pause)?;
      
      // Wait 500ms to allow concurrent decoder work to settle
      thread::sleep(Duration::from_millis(500));
      
      // Check if we received any more frames, and ensure none of them are significantly ahead
      let mut last_received_frame_after_pause = last_playing_frame;
      while let Ok(frame) = frame_rx.try_recv() {
          last_received_frame_after_pause = frame.frame_idx();
      }
      
      let leak_amount = last_received_frame_after_pause.saturating_sub(last_playing_frame);
      println!("Settle complete. Last received frame after pause: {} (leak = {} frames)", last_received_frame_after_pause, leak_amount);
      assert!(leak_amount <= 2, "Video frame lead should not exceed 2 frames (actual leak: {} frames)", leak_amount);
  ```

- [ ] **Step 3: Run integration tests**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS (showing "All Tests Passed Successfully")

- [ ] **Step 4: Commit**
  ```bash
  git add src/bin/test_playback_sync.rs
  git commit -m "test: add integration test for play-to-pause frame leak prevention"
  ```
