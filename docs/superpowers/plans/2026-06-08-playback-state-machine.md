# Playback State Machine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor Snippy2's audio-video synchronization and seek handling into an explicit, debounced state machine to eliminate post-seek fixed sync offsets.

**Architecture:** Introduce `PlaybackState` with `Seeking`, `Paused`, and `Playing` states. During `Seeking` (scrubbing/stepping), audio is ignored and the CPAL callback plays silence. Transitioning from `Seeking` to `Paused` is debounced by 500ms, at which point the media engine performs a background precise seek to prime the audio buffer. Clicking Play starts playback instantly if already primed, or cancels the debounce to prime synchronously.

**Tech Stack:** Rust, egui, crossbeam-channel, ringbuf, CPAL, FFmpeg.

---

### Task 1: Define PlaybackState Enum and Add UI State Fields

**Files:**
- Modify: `src/ui/mod.rs` (around line 29)

- [ ] **Step 1: Define PlaybackState Enum**
  Add the following enum definition to the top of `src/ui/mod.rs` (under the imports/helper enums):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum PlaybackState {
      Seeking,
      Paused,
      Playing,
  }
  ```

- [ ] **Step 2: Add State Fields to MainApp Struct**
  Modify the `MainApp` struct definition to include the new fields:
  ```rust
  pub struct MainApp {
      // ... existing fields ...
      pub ui_state: PlaybackState,
      pub last_seek_time: Option<std::time::Instant>,
      pub target_seek_frame: Option<usize>,
  }
  ```

- [ ] **Step 3: Initialize Fields in MainApp::new**
  Update `MainApp::new` to initialize these fields:
  ```rust
  Self {
      // ... existing fields ...
      ui_state: PlaybackState::Paused,
      last_seek_time: None,
      target_seek_frame: None,
  }
  ```

- [ ] **Step 4: Verify Compilation**
  Run: `cargo check`
  Expected: PASS (or minor warnings about unused fields)

- [ ] **Step 5: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: define PlaybackState enum and add state fields to MainApp"
  ```

---

### Task 2: Update MediaCommand and run_decoding_loop in Media Engine

**Files:**
- Modify: `src/media/mod.rs` (around line 73 and 208)

- [ ] **Step 1: Update MediaCommand::Seek**
  Modify `MediaCommand` in `src/media/mod.rs` to add `audio_enabled: bool`:
  ```rust
  pub enum MediaCommand {
      Play,
      Pause,
      Seek {
          frame_index: usize,
          keyframe_only: bool,
          sequence_id: usize,
          audio_enabled: bool,
      },
      Shutdown,
  }
  ```

- [ ] **Step 2: Update run_decoding_loop Seek Handlers**
  Track `mut audio_enabled = true;` inside `run_decoding_loop`.
  Update the four `Seek` command pattern matches inside `run_decoding_loop` to:
  ```rust
  MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
      audio_enabled = ae;
      seek_requested = Some((frame_index, keyframe_only, sequence_id));
  }
  ```
  And update the demuxer seek processing block around line 606 to only send the seek command to the audio thread if `audio_enabled` is true:
  ```rust
  let mut audio_resp_rx = None;
  if audio_stream_idx_opt.is_some() && audio_enabled {
      let (audio_resp_tx, rx) = crossbeam_channel::bounded::<()>(1);
      let target_time = frame_index as f64 / fps;
      let _ = audio_cmd_tx.send(AudioThreadCommand::Seek {
          target_time,
          resp_tx: audio_resp_tx,
      });
      audio_resp_rx = Some(rx);
  }
  ```
  And drop audio packets if `audio_enabled` is false around line 725:
  ```rust
  } else if Some(stream_idx) == audio_stream_idx_opt {
      if audio_enabled {
          crossbeam_channel::select! {
              send(audio_packet_tx, packet) -> res => {
                  if res.is_err() {
                      break;
                  }
              }
              recv(background_rx) -> cmd_res => {
                  // ... handle commands ...
              }
          }
      }
  }
  ```

- [ ] **Step 3: Verify Compilation**
  Run: `cargo check`
  Expected: Fails only in `src/ui/mod.rs` and `test_playback_sync.rs` due to the modified `MediaCommand::Seek` construction.

- [ ] **Step 4: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat: add audio_enabled flag to MediaCommand::Seek and handle it in demuxer"
  ```

---

### Task 3: Refactor Seek and Play/Pause UI Logic

**Files:**
- Modify: `src/ui/mod.rs` (around line 148 and 175)

- [ ] **Step 1: Implement seek_internal Helper**
  Add a helper function `seek_internal` to `MainApp` in `src/ui/mod.rs`:
  ```rust
  fn seek_internal(&mut self, frame: usize, keyframe_only: bool, audio_enabled: bool) {
      self.latest_sequence_id += 1;
      self.seek_in_progress = true;
      self.current_frame = frame.clamp(0, self.total_frames);
      self.playback_state.reset(self.current_frame as f64 / self.video_fps);
      self.playback_state.is_playing.store(false, std::sync::atomic::Ordering::SeqCst);
      if let Some(ref me) = self.media_engine {
          let _ = me.tx.send(crate::media::MediaCommand::Seek {
              frame_index: self.current_frame,
              keyframe_only,
              sequence_id: self.latest_sequence_id,
              audio_enabled,
          });
      }
      if let Some(ref rx) = self.frame_rx {
          while rx.try_recv().is_ok() {}
      }
      self.future_frame = None;
  }
  ```

- [ ] **Step 2: Update seek_to_frame**
  Refactor `seek_to_frame` to use `seek_internal` and manage state variables:
  ```rust
  fn seek_to_frame(&mut self, frame: usize, keyframe_only: bool) {
      self.ui_state = PlaybackState::Seeking;
      self.is_playing = false;
      self.last_seek_time = Some(std::time::Instant::now());
      self.target_seek_frame = Some(frame);
      self.seek_internal(frame, keyframe_only, false);
  }
  ```

- [ ] **Step 3: Refactor set_playing**
  Refactor `set_playing` to implement the state transitions and priming logic:
  ```rust
  fn set_playing(&mut self, play: bool) {
      if play {
          if self.ui_state == PlaybackState::Seeking {
              // Quick Play: cancel debounce timer and prime audio synchronously
              self.ui_state = PlaybackState::Playing;
              self.is_playing = true;
              self.playback_state.is_playing.store(true, std::sync::atomic::Ordering::SeqCst);
              self.last_seek_time = None;
              if let Some(target_frame) = self.target_seek_frame {
                  self.seek_internal(target_frame, false, true);
              }
          } else if self.ui_state == PlaybackState::Paused {
              self.ui_state = PlaybackState::Playing;
              self.is_playing = true;
              self.playback_state.is_playing.store(true, std::sync::atomic::Ordering::SeqCst);
              self.playback_state.audio_paused_for_sync.store(false, std::sync::atomic::Ordering::SeqCst);
              self.playback_state.start_fallback_clock();
              if let Some(ref me) = self.media_engine {
                  let _ = me.tx.send(crate::media::MediaCommand::Play);
              }
          }
      } else {
          // Pause
          self.ui_state = PlaybackState::Paused;
          self.is_playing = false;
          self.playback_state.is_playing.store(false, std::sync::atomic::Ordering::SeqCst);
          self.playback_state.stop_fallback_clock();
          if let Some(ref me) = self.media_engine {
              let _ = me.tx.send(crate::media::MediaCommand::Pause);
          }
      }
  }
  ```

- [ ] **Step 4: Verify Compilation**
  Run: `cargo check`
  Expected: Fails only in `test_playback_sync.rs` due to `MediaCommand::Seek`.

- [ ] **Step 5: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: refactor UI seek and play/pause logic to drive state transitions"
  ```

---

### Task 4: Implement 500ms Debounce and Catch-Up Logic in UI update Loop

**Files:**
- Modify: `src/ui/mod.rs` (around line 720)

- [ ] **Step 1: Add Debounce Timer Check to update**
  At the beginning of `update` (around line 720, before checking `self.is_playing`):
  ```rust
  if self.ui_state == PlaybackState::Seeking {
      if let Some(last_seek) = self.last_seek_time {
          if last_seek.elapsed() >= std::time::Duration::from_millis(500) {
              self.ui_state = PlaybackState::Paused;
              self.last_seek_time = None;
              if let Some(target_frame) = self.target_seek_frame {
                  // Debounce expired: trigger precise seek + audio priming!
                  self.seek_internal(target_frame, false, true);
              }
          }
      }
  }
  ```

- [ ] **Step 2: Update background frame catch-up check**
  Replace lines 794-816 (the `audio_paused_for_sync` catch-up check) with:
  ```rust
  if self.playback_state.audio_paused_for_sync.load(std::sync::atomic::Ordering::SeqCst) {
      let mut caught_up = false;
      if let Some(ref f) = self.latest_frame {
          if f.sequence_id() == self.latest_sequence_id {
              if self.ui_state == PlaybackState::Seeking {
                  caught_up = false; // Ignore catch-up in seeking state
              } else {
                  if f.frame_idx() >= target_frame || f.frame_idx() >= self.total_frames.saturating_sub(1) {
                      caught_up = true;
                  }
              }
          }
      }
      if caught_up {
          self.seek_in_progress = false;
          if self.ui_state == PlaybackState::Playing {
              self.playback_state.audio_paused_for_sync.store(false, std::sync::atomic::Ordering::SeqCst);
              self.playback_state.start_fallback_clock();
          }
      }
  }
  ```

- [ ] **Step 3: Verify Compilation**
  Run: `cargo check`
  Expected: Fails only in `test_playback_sync.rs` due to `MediaCommand::Seek`.

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: implement debounced seek-to-paused transition and update catch-up check"
  ```

---

### Task 5: Update Integration Tests and Verify

**Files:**
- Modify: `src/bin/test_playback_sync.rs`

- [ ] **Step 1: Update Test Seek Invocations**
  Modify the `MediaCommand::Seek` constructions in `src/bin/test_playback_sync.rs` to add `audio_enabled`:
  - Line 74:
    ```rust
    me.tx.send(MediaCommand::Seek { frame_index: nearest_kf, keyframe_only: true, sequence_id: 1, audio_enabled: false })?;
    ```
  - Line 125:
    ```rust
    me.tx.send(MediaCommand::Seek { frame_index: exact_target, keyframe_only: false, sequence_id: 2, audio_enabled: true })?;
    ```

- [ ] **Step 2: Run the Playback Sync Test**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS with "All Tests Passed Successfully"

- [ ] **Step 3: Commit**
  ```bash
  git add src/bin/test_playback_sync.rs
  git commit -m "test: update integration tests to compile and pass with new state machine commands"
  ```
