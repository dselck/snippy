# Arrow Key Seeking Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement high-performance, pipelined frame-by-frame seeking when holding down ArrowLeft/ArrowRight, and build a dedicated benchmark test harness to measure its speed and throughput.

**Architecture:** We replace the single-in-flight `in_flight_seek_seq: Option<usize>` with a `HashSet<usize>` of `in_flight_seeks`. We allow up to 2 seeks in flight during arrow holds, and disable the stale frame discard logic during stepping. This pipelines decoding and texture uploads to achieve a solid 60 FPS in the GUI.

**Tech Stack:** Rust, egui, crossbeam channels, ffmpeg-next.

---

### Task 1: Update the Arrow Seek Benchmark Harness to Support Pipelining

**Files:**
- Modify: `src/bin/bench_arrow_seek.rs`

- [ ] **Step 1: Update the benchmark harness**
  Replace the contents of `src/bin/bench_arrow_seek.rs` with the following:

  ```rust
  //! Headless arrow-key seeking benchmark.
  //!
  //! Simulates a user holding down ArrowRight (forward) and ArrowLeft (backward)
  //! to measure achieved FPS and frame delivery latency under multi-in-flight pipelining.

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
      max_in_flight: usize,
  ) -> (Stats, f64) {
      rig.drain_frames();

      let mut stats = Stats::new();
      let mut current = start_frame;
      let mut in_flight = std::collections::HashSet::<usize>::new();
      let mut send_times = std::collections::HashMap::<usize, Instant>::new();

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
                  let frame_seq = frame.sequence_id();
                  if in_flight.remove(&frame_seq) {
                      if let Some(send_time) = send_times.remove(&frame_seq) {
                          stats.push(send_time.elapsed());
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

                      got_frame = true;
                  }
              }

              if got_frame {
                  frames_displayed += 1;
              }

              // 2. UI sends next seek if under limit
              while in_flight.len() < max_in_flight && frames_displayed + in_flight.len() < count {
                  if direction > 0 {
                      current = (current + 1).min(rig.metadata.total_frames.saturating_sub(1));
                  } else {
                      current = current.saturating_sub(1);
                  }

                  *seq += 1;
                  in_flight.insert(*seq);
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
      let (gui_stats_1, gui_fps_1) = simulate_arrow_hold_gui(&rig, 0, 1, count, &mut seq, 60.0, 1);
      println!(
          "  Simulated 60Hz GUI (1-in-flight): processed {} frames ({:.1} FPS)",
          count,
          gui_fps_1
      );
      report.add("Hold Right (Simulated 60Hz GUI 1-in-flight) Latency", gui_stats_1);

      // ── Scenario 3: Simulated GUI 60Hz Hold Seek (2-in-flight) ──
      println!("\n--- Scenario 3: Simulated GUI 60Hz Hold Seek (2-in-flight) ---");
      let (gui_stats_2, gui_fps_2) = simulate_arrow_hold_gui(&rig, 0, 1, count, &mut seq, 60.0, 2);
      println!(
          "  Simulated 60Hz GUI (2-in-flight): processed {} frames ({:.1} FPS)",
          count,
          gui_fps_2
      );
      report.add("Hold Right (Simulated 60Hz GUI 2-in-flight) Latency", gui_stats_2);

      report.print_table();
      report.write_json("arrow_seek")?;

      let _ = rig.send(MediaCommand::Shutdown);
      Ok(())
  }
  ```

---

### Task 2: Implement Multi-In-Flight Seeking and Frame Discard Guard in UI

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace `in_flight_seek_seq` with `in_flight_seeks` HashSet**
  Update the `MainApp` struct definition in `src/ui/mod.rs`:
  Remove `pub in_flight_seek_seq: Option<usize>`
  Add `pub in_flight_seeks: std::collections::HashSet<usize>`

  Update `MainApp::new` initialization:
  ```rust
              in_flight_seeks: std::collections::HashSet::new(),
  ```

- [ ] **Step 2: Update all resets of in-flight seek state**
  Replace all `self.in_flight_seek_seq = None` and `self.in_flight_seek_seq = Some(...)` calls in `src/ui/mod.rs` to insert/remove/clear from the `HashSet`.

- [ ] **Step 3: Update `try_recv()` loop in `update()`**
  Do NOT discard older frames when stepping:
  ```rust
          // Receive frames from the Media Engine
          if let Some(rx) = &self.frame_rx {
              let mut got_frame = false;
              let is_stepping = matches!(
                  self.arrow_seek_state,
                  ArrowSeekState::SteppingLeft { .. } | ArrowSeekState::SteppingRight { .. }
              );

              while let Ok(frame) = rx.try_recv() {
                  if !is_stepping && frame.sequence_id() < self.latest_sequence_id {
                      continue;
                  }
                  let frame_idx = match &frame {
                      media::DecodedFrame::Cpu { frame_idx, .. } => *frame_idx,
                      media::DecodedFrame::Gpu { frame_idx, .. } => *frame_idx,
                  };
                  
                  self.in_flight_seeks.remove(&frame.sequence_id());
                  
                  self.latest_frame = Some(frame);
                  if !self.is_scrubbing {
                      self.current_frame = frame_idx;
                  }
                  got_frame = true;
                  self.decode_fps_frame_count += 1;
              }
  ```

- [ ] **Step 4: Update the arrow key hold handlers**
  Allow up to 2 seeks in flight:
  ```rust
                  ArrowSeekState::SteppingLeft { held_since } => {
                      if prev_down {
                          if held_since.elapsed() >= std::time::Duration::from_millis(250) {
                              if self.in_flight_seeks.len() < 2 {
                                  self.is_playing = false;
                                  self.current_frame = self.current_frame.saturating_sub(1);
                                  self.seek_internal(self.current_frame, false, false);
                                  self.in_flight_seeks.insert(self.latest_sequence_id);
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
                              if self.in_flight_seeks.len() < 2 {
                                  self.is_playing = false;
                                  self.current_frame = (self.current_frame + 1).min(self.total_frames);
                                  self.seek_internal(self.current_frame, false, false);
                                  self.in_flight_seeks.insert(self.latest_sequence_id);
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
  Expected: Scenario 3 (2-in-flight) runs at a solid 60 FPS.

- [ ] **Step 6: Verify the project tests**
  Run: `cargo test`
  Expected: All tests pass.

- [ ] **Step 7: Commit changes**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): implement multi-in-flight pipelined seeking"
  ```
