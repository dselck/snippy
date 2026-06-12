# Arrow Key Seeking Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement high-performance, throttled frame-by-frame seeking when holding down ArrowLeft/ArrowRight, and build a dedicated benchmark test harness to measure its speed and throughput.

**Architecture:** We replace the ad-hoc timestamp tracking with a state machine (`ArrowSeekState`) in the UI thread. To prevent command queue flooding and guarantee sequential fast-path decoding, we enforce a one-in-flight seek constraint using `in_flight_seek_seq`. We fix the play-to-pause transition bug by explicitly pausing the audio engine and sending a `Pause` command to the Media Engine when starting seeking. We prevent OS auto-repeat from resetting the state machine by checking if we are already in a stepping state. We fix the debug HUD FPS display by updating the counters.

**Tech Stack:** Rust, egui, crossbeam channels, ffmpeg-next.

---

### Task 1: Update the Arrow Seek Benchmark Harness

**Files:**
- Modify: `src/bin/bench_arrow_seek.rs`

- [ ] **Step 1: Check/Update the benchmark harness**
  Verify the benchmark harness in `src/bin/bench_arrow_seek.rs` contains the clean pause, play-to-seek buggy, and play-to-seek fixed scenarios. (We already updated this in our previous commit, but confirm it compiles).

---

### Task 2: Implement UI Fixes (OS repeat guard, set_playing, and Debug HUD FPS)

**Files:**
- Modify: `src/ui/mod.rs`

- [x] **Step 1: Update arrow key press transitions with repeat guard and set_playing**
  Modify the `update` method in `src/ui/mod.rs` to add the `is_currently_stepping` check to block OS auto-repeat from resetting `held_since`, and call `self.set_playing(false)` on transition:

  ```rust
          // State updates and input logic
          let is_currently_stepping = matches!(
              self.arrow_seek_state,
              ArrowSeekState::SteppingLeft { .. } | ArrowSeekState::SteppingRight { .. }
          );

          if prev_pressed && !is_currently_stepping {
              self.set_playing(false);
              self.arrow_seek_state = ArrowSeekState::SteppingLeft {
                  held_since: std::time::Instant::now(),
              };
              self.current_frame = self.current_frame.saturating_sub(1);
              self.seek_internal(self.current_frame, false, false);
              self.in_flight_seek_seq = Some(self.latest_sequence_id);
          } else if next_pressed && !is_currently_stepping {
              self.set_playing(false);
              self.arrow_seek_state = ArrowSeekState::SteppingRight {
                  held_since: std::time::Instant::now(),
              };
              self.current_frame = (self.current_frame + 1).min(self.total_frames);
              self.seek_internal(self.current_frame, false, false);
              self.in_flight_seek_seq = Some(self.latest_sequence_id);
          } else {
              match self.arrow_seek_state {
                  ArrowSeekState::SteppingLeft { held_since } => {
                      if prev_down {
                          if held_since.elapsed() >= std::time::Duration::from_millis(250) {
                              if self.in_flight_seek_seq.is_none() {
                                  self.is_playing = false;
                                  self.current_frame = self.current_frame.saturating_sub(1);
                                  self.seek_internal(self.current_frame, false, false);
                                  self.in_flight_seek_seq = Some(self.latest_sequence_id);
                              }
                          }
                          ctx.request_repaint();
                      } else {
                          self.arrow_seek_state = ArrowSeekState::Idle;
                      }
                  }
                  ArrowSeekState::SteppingRight { held_since } => {
                      if next_down {
                          if held_since.elapsed() >= std::time::Duration::from_millis(250) {
                              if self.in_flight_seek_seq.is_none() {
                                  self.is_playing = false;
                                  self.current_frame = (self.current_frame + 1).min(self.total_frames);
                                  self.seek_internal(self.current_frame, false, false);
                                  self.in_flight_seek_seq = Some(self.latest_sequence_id);
                              }
                          }
                          ctx.request_repaint();
                      } else {
                          self.arrow_seek_state = ArrowSeekState::Idle;
                      }
                  }
                  ArrowSeekState::Idle => {}
              }
          }
  ```

- [x] **Step 2: Implement Debug HUD FPS calculation**
  Increment `self.decode_fps_frame_count` inside the frame receiving loop, and calculate `self.decode_fps` once per second:

  In the `if let Some(rx) = &self.frame_rx` block:
  ```rust
              while let Ok(frame) = rx.try_recv() {
                  if frame.sequence_id() < self.latest_sequence_id {
                      continue;
                  }
                  let frame_idx = match &frame {
                      media::DecodedFrame::Cpu { frame_idx, .. } => *frame_idx,
                      media::DecodedFrame::Gpu { frame_idx, .. } => *frame_idx,
                  };
                  
                  if Some(frame.sequence_id()) == self.in_flight_seek_seq {
                      self.in_flight_seek_seq = None;
                  }
                  
                  self.latest_frame = Some(frame);
                  if !self.is_scrubbing {
                      self.current_frame = frame_idx;
                  }
                  got_frame = true;
                  self.decode_fps_frame_count += 1;
              }
              
              let elapsed = self.decode_fps_last_reset.elapsed();
              if elapsed >= std::time::Duration::from_secs(1) {
                  self.decode_fps = self.decode_fps_frame_count as f32 / elapsed.as_secs_f32();
                  self.decode_fps_frame_count = 0;
                  self.decode_fps_last_reset = std::time::Instant::now();
              }
  ```

- [x] **Step 3: Run the benchmark again to measure new performance**
  Run the benchmark using the target video `bbb_sunflower_2160p_60fps_normal.mp4`:
  `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/bbb_sunflower_2160p_60fps_normal.mp4" cargo run --release --bin bench_arrow_seek`
  Expected: Successful run, all Scenarios show correct FPS.

- [x] **Step 4: Verify the project tests**
  Run: `cargo test`
  Expected: All tests pass.

- [x] **Step 5: Commit changes**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): fix OS repeat state bypass and debug HUD FPS calculation"
  ```
