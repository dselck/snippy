# Arrow Key Seeking Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement Option 3 (Scrub-like unpaced fast playback) when holding ArrowRight for high-performance sequential seek, keep ticked seeks for ArrowLeft, and update the benchmark harness to verify it.

**Architecture:** We add `MediaCommand::PlayUnpaced` to stream sequential frames unpaced (bypassing the clock) when holding ArrowRight, and use ticked seek commands for ArrowLeft. We drain the UI channel and display the latest frame on every tick.

**Tech Stack:** Rust, egui, crossbeam channels, ffmpeg-next.

---

### Task 1: Update MediaCommand Types

**Files:**
- Modify: `src/media/types.rs:69-80`

- [ ] **Step 1: Add PlayUnpaced to MediaCommand**
  Modify the `MediaCommand` enum in `src/media/types.rs`:
  ```rust
  #[derive(Debug)]
  pub enum MediaCommand {
      Play,
      Pause,
      Seek {
          frame_index: usize,
          keyframe_only: bool,
          sequence_id: usize,
          audio_enabled: bool,
      },
      PlayUnpaced {
          sequence_id: usize,
      },
      Shutdown,
  }
  ```

- [ ] **Step 2: Commit**
  ```bash
  git add src/media/types.rs
  git commit -m "feat(media): add PlayUnpaced command variant"
  ```

---

### Task 2: Modify VideoDecoderPipeline

**Files:**
- Modify: `src/media/decoder_pipeline.rs:61-66`

- [ ] **Step 1: Add play_unpaced method**
  Add `play_unpaced` to `VideoDecoderPipeline` in `src/media/decoder_pipeline.rs`:
  ```rust
      pub fn play_unpaced(&mut self, sequence_id: usize) {
          self.play_mode = true;
          self.seek_target_frame = None;
          self.keyframe_only_mode = false;
          self.decode_more = true;
          self.current_sequence_id = sequence_id;
      }
  ```

- [ ] **Step 2: Commit**
  ```bash
  git add src/media/decoder_pipeline.rs
  git commit -m "feat(media): add play_unpaced method to VideoDecoderPipeline"
  ```

---

### Task 3: Implement PlayUnpaced in Media Engine

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Declare unpaced state and handle PlayUnpaced in run_decoding_loop**
  In `run_decoding_loop` (around line 183), declare:
  ```rust
      let mut sync_engine = crate::media::sync::VideoSyncEngine::new(Box::new(master_clock.clone()));
      let mut playing = false;
      let mut unpaced = false;
      let mut play_after_seek = false;
  ```
  And update the `MediaCommand` match arms (around line 201) to reset/set the `unpaced` flag:
  ```rust
                  MediaCommand::Play => {
                      playing = true;
                      unpaced = false;
                      play_after_seek = false;
                      sync_engine.reset();
                      let _ = decoder_cmd_tx.send(MediaCommand::Play);
                  }
                  MediaCommand::PlayUnpaced { sequence_id } => {
                      playing = true;
                      unpaced = true;
                      play_after_seek = false;
                      latest_seq = sequence_id;
                      while play_queue_rx.try_recv().is_ok() {}
                      let _ = decoder_cmd_tx.send(MediaCommand::PlayUnpaced { sequence_id });
                  }
                  MediaCommand::Pause => {
                      playing = false;
                      unpaced = false;
                      play_after_seek = false;
                      let _ = decoder_cmd_tx.send(MediaCommand::Pause);
                  }
                  MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled } => {
                      play_after_seek = playing || play_after_seek;
                      latest_seq = sequence_id;
                      playing = false;
                      unpaced = false;
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

- [ ] **Step 2: Update present current frame if due**
  Update the presentation check in `run_decoding_loop` (around line 258) to bypass clock sync when `unpaced` is true:
  ```rust
          // 3. Present current frame if due
          wait_duration = None;
          if playing {
              if let Some(frame) = &current_frame {
                  if unpaced {
                      let frame = current_frame.take().unwrap();
                      let _ = frame_tx.try_send(frame);
                  } else {
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
          }
  ```

- [ ] **Step 3: Handle PlayUnpaced in run_decoder_thread**
  Add the `PlayUnpaced` arm to the main command block in `run_decoder_thread` (around line 437):
  ```rust
                  MediaCommand::PlayUnpaced { sequence_id } => {
                      audio_enabled = false;
                      master_clock.set_has_audio(false);
                      video_pipeline.play_unpaced(sequence_id);
                  }
  ```
  Add the `PlayUnpaced` arm to the channel-wait block (around line 508):
  ```rust
                                                      MediaCommand::PlayUnpaced { sequence_id } => {
                                                          audio_enabled = false;
                                                          master_clock.set_has_audio(false);
                                                          video_pipeline.play_unpaced(sequence_id);
                                                          f_opt = Some(f);
                                                      }
  ```
  Add the `PlayUnpaced` arm to the audio deadlock check block (around line 549):
  ```rust
                                                          MediaCommand::PlayUnpaced { sequence_id } => {
                                                              audio_enabled = false;
                                                              master_clock.set_has_audio(false);
                                                              video_pipeline.play_unpaced(sequence_id);
                                                          }
  ```

- [ ] **Step 4: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat(media): implement PlayUnpaced command and unpaced presenter streaming"
  ```

---

### Task 4: Update UI State Machine and Key Handlers

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Update ArrowSeekState enum**
  Update the definition of `ArrowSeekState` (around line 30):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ArrowSeekState {
      Idle,
      SteppingLeft { held_since: std::time::Instant },
      SteppingRight { held_since: std::time::Instant, unpaced_started: bool },
  }
  ```

- [ ] **Step 2: Update initial arrow key pressed handlers**
  Update the handlers (around line 1812) to initialize `unpaced_started` to false for right seek:
  ```rust
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
                  unpaced_started: false,
              };
              self.current_frame = (self.current_frame + 1).min(self.total_frames);
              self.seek_internal(self.current_frame, false, false);
              self.in_flight_seek_seq = Some(self.latest_sequence_id);
          }
  ```

- [ ] **Step 3: Update key repeat handler for SteppingRight**
  Update the `ArrowSeekState` matching logic (around line 1842):
  ```rust
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
                          }
                          self.arrow_seek_state = ArrowSeekState::Idle;
                      }
                  }
  ```

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): update ArrowSeekState to trigger unpaced playback on Right Arrow hold"
  ```

---

### Task 5: Update Benchmark Harness

**Files:**
- Modify: `src/bin/bench_arrow_seek.rs`

- [ ] **Step 1: Update simulate_arrow_hold_gui to use PlayUnpaced**
  Replace the contents of `src/bin/bench_arrow_seek.rs` with the updated code supporting `PlayUnpaced` and Scenario 3:
  ```rust
  //! Headless arrow-key seeking benchmark.
  //!
  //! Simulates a user holding down ArrowRight (forward) and ArrowLeft (backward)
  //! to measure achieved FPS and frame delivery latency under VSync-ticked seeking and buffer draining.

  use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
  use snippy2::media::MediaCommand;
  use std::time::{Duration, Instant};

  const SEEK_TIMEOUT: Duration = Duration::from_secs(5);

  fn simulate_arrow_hold_raw(
      rig: &MediaTestRig,
      start_frame: usize,
      direction: isize,
      count: usize,
      seq: &mut usize,
      pre_play: bool,
      send_pause: bool,
  ) -> Stats {
      rig.drain_frames();

      if pre_play {
          let _ = rig.send(MediaCommand::Play);
          std::thread::sleep(Duration::from_millis(50));
          if send_pause {
              let _ = rig.send(MediaCommand::Pause);
              std::thread::sleep(Duration::from_millis(10));
          }
      }

      let mut stats = Stats::new();
      let mut current = start_frame;

      *seq += 1;
      if let Some((_, elapsed)) = rig.timed_seek(current, false, *seq, SEEK_TIMEOUT) {
          stats.push(elapsed);
      }

      for _ in 0..count {
          if direction > 0 {
              current = (current + 1).min(rig.metadata.total_frames.saturating_sub(1));
          } else {
              current = current.saturating_sub(1);
          }

          *seq += 1;
          if let Some((_, elapsed)) = rig.timed_seek(current, false, *seq, SEEK_TIMEOUT) {
              stats.push(elapsed);
          }
      }

      stats
  }

  fn simulate_arrow_hold_gui(
      rig: &MediaTestRig,
      start_frame: usize,
      direction: isize,
      count: usize,
      seq: &mut usize,
      vsync_hz: f64,
  ) -> (Stats, f64) {
      rig.drain_frames();

      let mut stats = Stats::new();
      let mut current = start_frame;

      let tick_interval = Duration::from_secs_f64(1.0 / vsync_hz);
      let mut next_tick = Instant::now() + tick_interval;
      let mut frames_displayed = 0;

      let total_start = Instant::now();
      let duration_limit = Duration::from_secs(5);

      let mut unpaced_started = false;
      let mut send_times = std::collections::HashMap::<usize, Instant>::new();

      if direction > 0 {
          *seq += 1;
          unpaced_started = true;
          let _ = rig.send(MediaCommand::PlayUnpaced { sequence_id: *seq });
      }

      while frames_displayed < count && total_start.elapsed() < duration_limit {
          let now = Instant::now();

          if now >= next_tick {
              next_tick += tick_interval;

              // 1. UI receives frames from channel (draining channel)
              let mut latest_frame_received = None;

              while let Ok(frame) = rig.frame_rx.try_recv() {
                  if frame.sequence_id() < *seq {
                      continue;
                  }
                  latest_frame_received = Some(frame);
              }

              if let Some(frame) = latest_frame_received {
                  let frame_seq = frame.sequence_id();
                  if unpaced_started {
                      let frame_age = total_start.elapsed() / (frames_displayed + 1) as u32;
                      stats.push(frame_age);
                  } else {
                      if let Some(send_time) = send_times.remove(&frame_seq) {
                          stats.push(send_time.elapsed());
                      }
                  }

                  // Simulate texture upload overhead (copy YUV planes)
                  if let snippy2::media::DecodedFrame::Cpu { planes, .. } = &frame {
                      let mut dummy_y = vec![0u8; planes.y.len()];
                      let mut dummy_u = vec![0u8; planes.u.len()];
                      let mut dummy_v = vec![0u8; planes.v.len()];
                      dummy_y.copy_from_slice(&planes.y);
                      dummy_u.copy_from_slice(&planes.u);
                      dummy_v.copy_from_slice(&planes.v);
                      std::hint::black_box((dummy_y, dummy_u, dummy_v));
                  }

                  frames_displayed += 1;
                  current = frame.frame_idx();
              }

              // 2. UI sends next seek unconditionally on every tick ONLY for backward seeking
              if !unpaced_started && frames_displayed < count {
                  current = current.saturating_sub(1);
                  *seq += 1;
                  send_times.insert(*seq, Instant::now());
                  let _ = rig.send(MediaCommand::Seek {
                      frame_index: current,
                      keyframe_only: false,
                      sequence_id: *seq,
                      audio_enabled: false,
                  });
              }
          }

          std::thread::sleep(Duration::from_millis(1));
      }

      if unpaced_started {
          let _ = rig.send(MediaCommand::Pause);
      }

      let total_elapsed = total_start.elapsed().as_secs_f64();
      let actual_fps = frames_displayed as f64 / total_elapsed;

      (stats, actual_fps)
  }

  fn main() -> Result<(), anyhow::Error> {
      let video_path = TestMediaLocator::resolve()?;
      let rig = MediaTestRig::new(&video_path)?;
      let video_info = format_video_info(&rig.metadata);

      println!("=== Arrow-Key Seek Performance Benchmark ===");
      println!("  Video: {}", video_path.display());
      println!("  {video_info}");

      let total_frames = rig.metadata.total_frames;
      let mut report = BenchReport::new("Arrow-Key Seeking", &video_info);
      let mut seq = 0;

      let count = (total_frames / 2).min(60);

      // ── Scenario 1: Hold Right from Paused State (Raw) ──
      println!("\n--- Scenario 1: Hold Right from Paused State (Raw) ---");
      let timer_start = Instant::now();
      let forward_stats = simulate_arrow_hold_raw(&rig, 0, 1, count, &mut seq, false, false);
      let forward_duration = timer_start.elapsed();
      let forward_fps = count as f64 / forward_duration.as_secs_f64();
      println!(
          "  Hold Right: processed {} frames in {:.2}s ({:.1} FPS)",
          count,
          forward_duration.as_secs_f64(),
          forward_fps
      );
      report.add("Hold Right (Paused) Latency (Raw)", forward_stats);

      // ── Scenario 2: Simulated GUI 60Hz Hold Seek Right (PlayUnpaced) ──
      println!("\n--- Scenario 2: Simulated GUI 60Hz Hold Seek Right (PlayUnpaced) ---");
      let (gui_stats_1, gui_fps_1) = simulate_arrow_hold_gui(&rig, 0, 1, count, &mut seq, 60.0);
      println!(
          "  Simulated 60Hz GUI Right: processed {} frames ({:.1} FPS)",
          count,
          gui_fps_1
      );
      report.add("Hold Right (Simulated 60Hz GUI) Latency", gui_stats_1);

      // ── Scenario 3: Simulated GUI 60Hz Hold Seek Left (VSync-Ticked seeking) ──
      println!("\n--- Scenario 3: Simulated GUI 60Hz Hold Seek Left (VSync-Ticked seeking) ---");
      let (gui_stats_2, gui_fps_2) = simulate_arrow_hold_gui(&rig, count * 2, -1, count, &mut seq, 60.0);
      println!(
          "  Simulated 60Hz GUI Left: processed {} frames ({:.1} FPS)",
          count,
          gui_fps_2
      );
      report.add("Hold Left (Simulated 60Hz GUI) Latency", gui_stats_2);

      report.print_table();
      report.write_json("arrow_seek")?;

      let _ = rig.send(MediaCommand::Shutdown);
      Ok(())
  }
  ```

- [ ] **Step 2: Commit**
  ```bash
  git add src/bin/bench_arrow_seek.rs
  git commit -m "bench: update benchmark to simulate PlayUnpaced and ticked left seek"
  ```

---

### Task 6: Run Tests and Benchmarks

- [ ] **Step 1: Run unit tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 2: Run benchmark on target video**
  Run: `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/bbb_sunflower_2160p_60fps_normal.mp4" cargo run --release --bin bench_arrow_seek`
  Expected: Scenario 2 (Hold Seek Right) runs at or above the decoder limit (e.g. 100+ FPS), and Scenario 3 runs successfully.
