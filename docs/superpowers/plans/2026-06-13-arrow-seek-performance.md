# Arrow Key Seeking Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement high-performance, single-in-flight throttled frame-by-frame seeking when holding down ArrowLeft/ArrowRight, and build a dedicated benchmark test harness to measure its speed and throughput.

**Architecture:** We use a single-in-flight `in_flight_seek_seq: Option<usize>` state. We allow only one seek in flight during arrow holds, and discard older frames in the UI receive loop (intelligent buffer draining). This guarantees sequential fast-path decoding and avoids rendering stalls, achieving a smooth 40-60 FPS (VSync-capped).

**Tech Stack:** Rust, egui, crossbeam channels, ffmpeg-next.

---

### Task 1: Update the Arrow Seek Benchmark Harness

**Files:**
- Modify: `src/bin/bench_arrow_seek.rs`

- [ ] **Step 1: Update the benchmark harness**
  Replace the contents of `src/bin/bench_arrow_seek.rs` with the following:

  ```rust
  //! Headless arrow-key seeking benchmark.
  //!
  //! Simulates a user holding down ArrowRight (forward) and ArrowLeft (backward)
  //! to measure achieved FPS and frame delivery latency under single-in-flight throttling.

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
      let mut in_flight: Option<usize> = None;
      let mut last_send_time = Instant::now();

      let tick_interval = Duration::from_secs_f64(1.0 / vsync_hz);
      let mut last_tick = Instant::now();
      let mut frames_displayed = 0;

      let total_start = Instant::now();
      let duration_limit = Duration::from_secs(5);

      while frames_displayed < count && total_start.elapsed() < duration_limit {
          let now = Instant::now();
          let elapsed = now.duration_since(last_tick);

          if elapsed >= tick_interval {
              last_tick = now;

              // 1. UI receives frames from channel
              let mut got_frame = false;
              while let Ok(frame) = rig.frame_rx.try_recv() {
                  // Intelligent Buffer Draining: discard older frames
                  if frame.sequence_id() < *seq {
                      continue;
                  }
                  
                  if Some(frame.sequence_id()) == in_flight {
                      stats.push(last_send_time.elapsed());

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

                      in_flight = None;
                      got_frame = true;
                  }
              }

              if got_frame {
                  frames_displayed += 1;
              }

              // 2. UI sends next seek if idle
              if in_flight.is_none() && frames_displayed < count {
                  if direction > 0 {
                      current = (current + 1).min(rig.metadata.total_frames.saturating_sub(1));
                  } else {
                      current = current.saturating_sub(1);
                  }

                  *seq += 1;
                  in_flight = Some(*seq);
                  last_send_time = Instant::now();
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

      // ── Scenario 2: Simulated GUI 60Hz Hold Seek (1-in-flight) ──
      println!("\n--- Scenario 2: Simulated GUI 60Hz Hold Seek (1-in-flight) ---");
      let (gui_stats_1, gui_fps_1) = simulate_arrow_hold_gui(&rig, 0, 1, count, &mut seq, 60.0);
      println!(
          "  Simulated 60Hz GUI (1-in-flight): processed {} frames ({:.1} FPS)",
          count,
          gui_fps_1
      );
      report.add("Hold Right (Simulated 60Hz GUI 1-in-flight) Latency", gui_stats_1);

      report.print_table();
      report.write_json("arrow_seek")?;

      let _ = rig.send(MediaCommand::Shutdown);
      Ok(())
  }
  ```

---

### Task 2: Implement Single-In-Flight seeking with Intelligent Buffer Draining in UI

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Revert `in_flight_seeks` back to `in_flight_seek_seq` Option**
  Update the `MainApp` struct definition in `src/ui/mod.rs`:
  Remove `pub in_flight_seeks: std::collections::HashSet<usize>`
  Add `pub in_flight_seek_seq: Option<usize>`

  Update `MainApp::new` initialization:
  ```rust
              in_flight_seek_seq: None,
  ```

- [ ] **Step 2: Update try-receive frame loop to do intelligent buffer draining**
  Ensure we discard older frames and update `in_flight_seek_seq`:
  ```rust
          // Receive frames from the Media Engine
          if let Some(rx) = &self.frame_rx {
              let mut got_frame = false;
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
  ```

- [ ] **Step 3: Update all resets of in-flight seek state in `src/ui/mod.rs`**
  Restore the original `self.in_flight_seek_seq = None` and `self.in_flight_seek_seq = Some(self.latest_sequence_id)` logic in `scrub_to`, `drag_stopped`, and the arrow key hold handlers.

- [ ] **Step 4: Update the arrow key hold handlers**
  Ensure only 1 seek is in flight:
  ```rust
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
  ```

- [ ] **Step 5: Run the benchmark again to measure new performance**
  Run the benchmark using the target video `bbb_sunflower_2160p_60fps_normal.mp4`:
  `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/bbb_sunflower_2160p_60fps_normal.mp4" cargo run --release --bin bench_arrow_seek`
  Expected: Successful run, Simulated GUI runs at ~56 FPS.

- [ ] **Step 6: Verify the project tests**
  Run: `cargo test`
  Expected: All tests pass.

- [ ] **Step 7: Commit changes**
  Run:
  ```bash
  git add src/ui/mod.rs src/media/mod.rs
  git commit -m "feat(ui): revert to stable single-in-flight seek with intelligent buffer draining"
  ```
