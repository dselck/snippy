# Arrow Seek Time Synchronization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure the displayed time and playhead are perfectly tied to the rendered frames during arrow key hold-seek, preventing the playhead from drifting or running ahead when the decoder stalls or encounters a GOP boundary.

**Architecture:** 
1. Guard the backward hold-seek (`SteppingLeft`) key repeat handler with `self.in_flight_seek_seq.is_none()`. This ensures we only decrement the frame index and dispatch the next seek after the previous one is successfully decoded and rendered, matching GOP boundaries naturally.
2. In the forward hold-seek (`SteppingRight`) release handler, reset the master clock to the final rendered `current_frame` index to keep playback synchronization correct when normal play resumes.

**Tech Stack:** Rust, egui, crossbeam channels.

---

### Task 1: Update Key Hold Handlers in UI

**Files:**
- Modify: `src/ui/mod.rs:1829-1856`

- [ ] **Step 1: Update the key handlers in `src/ui/mod.rs`**
  Modify the `ArrowSeekState` matching arms in `MainApp::update` to enforce the in-flight guard for left stepping and reset the master clock on right step release:
  ```rust
                  ArrowSeekState::SteppingLeft { held_since } => {
                      if prev_down {
                          if held_since.elapsed() >= std::time::Duration::from_millis(250) {
                              if self.in_flight_seek_seq.is_none() {
                                  self.is_playing = false;
                                  let target_frame = self.current_frame.saturating_sub(1);
                                  self.seek_internal(target_frame, false, false);
                                  self.in_flight_seek_seq = Some(self.latest_sequence_id);
                              }
                          }
                          ctx.request_repaint();
                      } else {
                          self.arrow_seek_state = ArrowSeekState::Idle;
                      }
                  }
                  ArrowSeekState::SteppingRight { held_since, unpaced_started } => {
                      if next_down {
                          if held_since.elapsed() >= std::time::Duration::from_millis(250) {
                              if !unpaced_started {
                                  self.latest_sequence_id += 1;
                                  if let Some(tx) = &self.media_tx {
                                      let _ = tx.send(media::MediaCommand::PlayUnpaced { sequence_id: self.latest_sequence_id });
                                  }
                                  self.arrow_seek_state = ArrowSeekState::SteppingRight {
                                      held_since,
                                      unpaced_started: true,
                                  };
                              }
                          }
                          ctx.request_repaint();
                      } else {
                          if unpaced_started {
                              self.set_playing(false);
                              if let Some(ae) = &self.audio_engine {
                                  ae.master_clock.reset(self.current_frame as f64 / self.video_fps);
                              }
                          }
                          self.arrow_seek_state = ArrowSeekState::Idle;
                      }
                  }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Run unit tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 4: Commit changes**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): tie seek time to rendered frames and fix master clock reset"
  ```
