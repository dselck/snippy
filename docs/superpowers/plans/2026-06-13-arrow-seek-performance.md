# Arrow Key Seeking Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement high-performance, throttled frame-by-frame seeking when holding down ArrowLeft/ArrowRight, and build a dedicated benchmark test harness to measure its speed and throughput.

**Architecture:** We replace the ad-hoc timestamp tracking with a state machine (`ArrowSeekState`) in the UI thread. To prevent command queue flooding and guarantee sequential fast-path decoding, we enforce a one-in-flight seek constraint using `in_flight_seek_seq`. A new headless benchmark `bench_arrow_seek.rs` will simulate this loop to verify throughput.

**Tech Stack:** Rust, egui, crossbeam channels, ffmpeg-next.

---

### Task 1: Create the Arrow Seek Benchmark Harness

**Files:**
- Create: `src/bin/bench_arrow_seek.rs`
- Modify: `Cargo.toml` (if needed, to register binary, though src/bin/*.rs is auto-discovered)

- [ ] **Step 1: Write the benchmark harness**
  Create the file `src/bin/bench_arrow_seek.rs` with the following content:

  ```rust
  //! Headless arrow-key seeking benchmark.
  //!
  //! Simulates a user holding down ArrowRight (forward) and ArrowLeft (backward)
  //! to measure achieved FPS and frame delivery latency under one-in-flight throttling.

  use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
  use snippy2::media::MediaCommand;
  use std::time::{Duration, Instant};

  const SEEK_TIMEOUT: Duration = Duration::from_secs(5);

  fn simulate_arrow_hold(
      rig: &MediaTestRig,
      start_frame: usize,
      direction: isize,
      count: usize,
      seq: &mut usize,
  ) -> Stats {
      let mut stats = Stats::new();
      let mut current = start_frame;

      // Tap seek (first step)
      *seq += 1;
      if let Some((_, elapsed)) = rig.timed_seek(current, false, *seq, SEEK_TIMEOUT) {
          stats.push(elapsed);
      }

      // Continuous hold seeks
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

      // ── Scenario 1: Hold Right (Forward) ──
      println!("\n--- Scenario: Hold Right (Forward) ---");
      let start_forward = 0;
      let count = (total_frames / 2).min(120); // Test up to 120 frames or half the video
      let timer_start = Instant::now();
      let forward_stats = simulate_arrow_hold(&rig, start_forward, 1, count, &mut seq);
      let forward_duration = timer_start.elapsed();
      let forward_fps = count as f64 / forward_duration.as_secs_f64();
      println!(
          "  Forward Hold: processed {} frames in {:.2}s ({:.1} FPS)",
          count,
          forward_duration.as_secs_f64(),
          forward_fps
      );
      report.add("Hold Right (Forward) Latency", forward_stats);

      // ── Scenario 2: Hold Left (Backward) ──
      println!("\n--- Scenario: Hold Left (Backward) ---");
      let start_backward = count;
      let timer_start = Instant::now();
      let backward_stats = simulate_arrow_hold(&rig, start_backward, -1, count, &mut seq);
      let backward_duration = timer_start.elapsed();
      let backward_fps = count as f64 / backward_duration.as_secs_f64();
      println!(
          "  Backward Hold: processed {} frames in {:.2}s ({:.1} FPS)",
          count,
          backward_duration.as_secs_f64(),
          backward_fps
      );
      report.add("Hold Left (Backward) Latency", backward_stats);

      report.print_table();
      report.write_json("arrow_seek")?;

      let _ = rig.send(MediaCommand::Shutdown);
      Ok(())
  }
  ```

- [ ] **Step 2: Verify compilation and run the baseline benchmark**
  Run: `cargo run --bin bench_arrow_seek`
  Expected: Successfully compiles and runs, outputting seek times and achieving some FPS. Note the baseline FPS.

- [ ] **Step 3: Commit baseline**
  Run:
  ```bash
  git add src/bin/bench_arrow_seek.rs
  git commit -m "bench: add arrow key seeking benchmark harness"
  ```

---

### Task 2: Implement the State Machine and One-in-Flight Throttling in UI

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add ArrowSeekState enum**
  Modify `src/ui/mod.rs` to define `ArrowSeekState` and replace the ad-hoc timestamp options in `MainApp` struct.

  Add the enum definition:
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ArrowSeekState {
      Idle,
      SteppingLeft { held_since: std::time::Instant },
      SteppingRight { held_since: std::time::Instant },
  }
  ```

  Replace `left_arrow_held_since` and `right_arrow_held_since` in `MainApp` with:
  ```rust
      pub arrow_seek_state: ArrowSeekState,
  ```

- [ ] **Step 2: Update MainApp initialization**
  Modify the `MainApp::new` instantiation to initialize `arrow_seek_state`:
  ```rust
              arrow_seek_state: ArrowSeekState::Idle,
  ```

- [ ] **Step 3: Refactor the Arrow Key event handler**
  In the `update` function in `src/ui/mod.rs`, replace the block processing arrow keys with:
  ```rust
          // State updates
          if prev_pressed {
              self.is_playing = false;
              self.arrow_seek_state = ArrowSeekState::SteppingLeft {
                  held_since: std::time::Instant::now(),
              };
              self.current_frame = self.current_frame.saturating_sub(1);
              self.seek_internal(self.current_frame, false, false);
              self.in_flight_seek_seq = Some(self.latest_sequence_id);
          } else if next_pressed {
              self.is_playing = false;
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

- [ ] **Step 4: Run the benchmark again to measure new performance**
  Run: `cargo run --bin bench_arrow_seek`
  Expected: Success, with significantly improved FPS on forward holds.

- [ ] **Step 5: Verify the project tests**
  Run: `cargo test`
  Expected: All tests pass.

- [ ] **Step 6: Commit changes**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): implement state-based, one-in-flight throttled arrow seeking"
  ```
