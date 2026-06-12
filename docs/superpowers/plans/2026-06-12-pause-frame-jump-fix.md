# Precise Seek Catch-up on Pause Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the frame jump to keyframe that happens when a video is paused or when seeking while paused, and fix the ~900ms video frame lead when pausing playback by keeping the sync engine active when paused and preserving the lookahead queue.

**Architecture:** 
1. Modify `video_pipeline.pause()` to keep `decode_more = true` when `seek_target_frame` is active (Completed).
2. Update the Media Engine thread loop to track `seek_target_frame_idx`. Update `Seek` and `Pause` command handling to set/clear it.
3. Update the idle frame-fetch block in `src/media/mod.rs` to only read from `play_queue_rx` if seeking or catch-up is active, preserving lookahead frames when simply paused.
4. Keep the sync engine active during playback pause in `src/media/mod.rs` by always calling `sync_engine.evaluate_frame(pts_sec)`.
5. Revert the UI precise seek on pause in `src/ui/mod.rs` back to its original clean implementation.
6. Run integration tests to verify everything passes.

**Tech Stack:** Rust, egui, FFmpeg.

---

### Task 1: Update Video Pipeline Pause Behavior (Completed)

**Files:**
- Modify: `src/media/decoder_pipeline.rs:68-72`

- [x] **Step 1: Modify `pause` in `VideoDecoderPipeline`**
  Completed.

---

### Task 2: Symmetrical Sync and Queue Preservation in Media Engine Loop

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Declare `seek_target_frame_idx` tracking variable**
  Open `src/media/mod.rs` and update the variable declarations around lines 184-190 to include `mut seek_target_frame_idx: Option<usize> = None;`:
  
  ```rust
      let mut playing = false;
      let mut play_after_seek = false;
      let mut latest_seq = 0;
      let mut current_frame: Option<DecodedFrame> = None;
      let mut pending_cmd: Option<MediaCommand> = None;
      let mut wait_duration: Option<std::time::Duration>;
      let mut seek_target_frame_idx: Option<usize> = None;
  ```

- [ ] **Step 2: Update `MediaCommand::Seek` handling**
  Update the `MediaCommand::Seek` match arm (around line 215) to set `seek_target_frame_idx = Some(frame_index)`:
  
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
  Modify the `MediaCommand::Pause` match arm (around line 206) to only set `playing = false` and `play_after_seek = false`, keeping `current_frame` and the queue intact:

  ```rust
                  MediaCommand::Pause => {
                      playing = false;
                      play_after_seek = false;
                      let _ = decoder_cmd_tx.send(MediaCommand::Pause);
                  }
  ```

- [ ] **Step 4: Update the idle frame-fetch `else` block**
  Update the frame-fetch `else` block (around line 237) to only read from `play_queue_rx` if we are actively seeking or catch-up is active, and break the loop if `playing` becomes true:

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

- [ ] **Step 5: Keep Sync Engine active during playback pause**
  Locate the frame evaluation block under Step 3 (around line 253) and change it to always evaluate the frame against the master clock, regardless of whether the master clock is playing:

  ```rust
          // 3. Present current frame if due
          wait_duration = None;
          if playing {
              if let Some(frame) = &current_frame {
                  let pts_sec = frame.pts_sec();
                  let action = sync_engine.evaluate_frame(pts_sec);
                  match action {
                      Ok(SyncAction::Present) => {
                          let frame = current_frame.take().unwrap();
                          let _ = frame_tx.try_send(frame);
                      }
                      Ok(SyncAction::Wait(remaining)) => {
                          wait_duration = Some(remaining);
                      }
                      Err(_) => {
                          let frame = current_frame.take().unwrap();
                          let _ = frame_tx.try_send(frame);
                      }
                  }
              }
          }
  ```

- [ ] **Step 6: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 7: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat(media): implement Symmetrical Sync on Pause and preserve lookahead queue"
  ```

---

### Task 3: Revert UI Precise Seek on Playback Pause

**Files:**
- Modify: `src/ui/mod.rs:214-242`

- [ ] **Step 1: Revert `set_playing` in `MainApp`**
  Open `src/ui/mod.rs` and locate the `set_playing` method. Revert it back to its original clean implementation:
  
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

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): revert UI play-to-pause seek alignment"
  ```

---

### Task 4: Run Integration Tests and Verify

**Files:**
- Test: `src/bin/test_playback_sync.rs`

- [ ] **Step 1: Run integration tests**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS (showing "All Tests Passed Successfully")
