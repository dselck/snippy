# Arrow Key Seeking Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement high-performance, throttled frame-by-frame seeking when holding down ArrowLeft/ArrowRight, and build a dedicated benchmark test harness to measure its speed and throughput.

**Architecture:** We replace the ad-hoc timestamp tracking with a state machine (`ArrowSeekState`) in the UI thread. To prevent command queue flooding and guarantee sequential fast-path decoding, we enforce a one-in-flight seek constraint using `in_flight_seek_seq`. We fix the play-to-pause transition bug by explicitly pausing the audio engine and sending a `Pause` command to the Media Engine when starting seeking. A new headless benchmark `bench_arrow_seek.rs` will simulate this loop under different backend states (clean pause, play-to-seek buggy, play-to-seek fixed) to verify throughput.

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
      pre_play: bool,
      send_pause: bool,
  ) -> Stats {
      // Drain any stale frames first
      rig.drain_frames();

      if pre_play {
          // Put the engine in Play state
          let _ = rig.send(MediaCommand::Play);
          // Wait briefly for playback to begin
          std::thread::sleep(Duration::from_millis(50));

          if send_pause {
              // Pause the engine properly before seeking
              let _ = rig.send(MediaCommand::Pause);
              std::thread::sleep(Duration::from_millis(10));
          }
      }

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

      let count = (total_frames / 2).min(60); // Test up to 60 frames

      // ── Scenario 1: Hold Right from Paused State ──
      println!("\n--- Scenario 1: Hold Right from Paused State ---");
      let timer_start = Instant::now();
      let forward_stats = simulate_arrow_hold(&rig, 0, 1, count, &mut seq, false, false);
      let forward_duration = timer_start.elapsed();
      let forward_fps = count as f64 / forward_duration.as_secs_f64();
      println!(
          "  Hold Right: processed {} frames in {:.2}s ({:.1} FPS)",
          count,
          forward_duration.as_secs_f64(),
          forward_fps
      );
      report.add("Hold Right (Paused) Latency", forward_stats);

      // ── Scenario 2: Hold Left from Paused State ──
      println!("\n--- Scenario 2: Hold Left from Paused State ---");
      let timer_start = Instant::now();
      let backward_stats = simulate_arrow_hold(&rig, count, -1, count, &mut seq, false, false);
      let backward_duration = timer_start.elapsed();
      let backward_fps = count as f64 / backward_duration.as_secs_f64();
      println!(
          "  Hold Left: processed {} frames in {:.2}s ({:.1} FPS)",
          count,
          backward_duration.as_secs_f64(),
          backward_fps
      );
      report.add("Hold Left (Paused) Latency", backward_stats);

      // ── Scenario 3: Hold Right from Play state (Buggy UI - no Pause sent) ──
      println!("\n--- Scenario 3: Hold Right from Play (Buggy - No Pause) ---");
      let timer_start = Instant::now();
      let buggy_stats = simulate_arrow_hold(&rig, 0, 1, count, &mut seq, true, false);
      let buggy_duration = timer_start.elapsed();
      let buggy_fps = count as f64 / buggy_duration.as_secs_f64();
      println!(
          "  Buggy (No Pause): processed {} frames in {:.2}s ({:.1} FPS)",
          count,
          buggy_duration.as_secs_f64(),
          buggy_fps
      );
      report.add("Hold Right (Buggy UI) Latency", buggy_stats);

      // ── Scenario 4: Hold Right from Play state (Fixed UI - Pause sent) ──
      println!("\n--- Scenario 4: Hold Right from Play (Fixed - Pause Sent) ---");
      let timer_start = Instant::now();
      let fixed_stats = simulate_arrow_hold(&rig, 0, 1, count, &mut seq, true, true);
      let fixed_duration = timer_start.elapsed();
      let fixed_fps = count as f64 / fixed_duration.as_secs_f64();
      println!(
          "  Fixed (Pause Sent): processed {} frames in {:.2}s ({:.1} FPS)",
          count,
          fixed_duration.as_secs_f64(),
          fixed_fps
      );
      report.add("Hold Right (Fixed UI) Latency", fixed_stats);

      report.print_table();
      report.write_json("arrow_seek")?;

      let _ = rig.send(MediaCommand::Shutdown);
      Ok(())
  }
  ```

- [ ] **Step 2: Verify compilation and run the benchmark**
  Run: `cargo run --bin bench_arrow_seek`
  Expected: Successfully compiles and runs. Observe the difference in FPS between Scenario 3 (buggy) and Scenario 4 (fixed).

- [ ] **Step 3: Commit benchmark changes**
  Run:
  ```bash
  git add src/bin/bench_arrow_seek.rs
  git commit -m "bench: add scenarios for play-to-seek transitions"
  ```

---

### Task 2: Implement Play-to-Pause Fix in UI

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Update arrow key press transitions**
  Modify `src/ui/mod.rs` to call `self.set_playing(false)` when transitioning to seeking states:

  ```rust
          // State updates
          if prev_pressed {
              self.set_playing(false);
              self.arrow_seek_state = ArrowSeekState::SteppingLeft {
                  held_since: std::time::Instant::now(),
              };
              self.current_frame = self.current_frame.saturating_sub(1);
              self.seek_internal(self.current_frame, false, false);
              self.in_flight_seek_seq = Some(self.latest_sequence_id);
          } else if next_pressed {
              self.set_playing(false);
              self.arrow_seek_state = ArrowSeekState::SteppingRight {
                  held_since: std::time::Instant::now(),
              };
              self.current_frame = (self.current_frame + 1).min(self.total_frames);
              self.seek_internal(self.current_frame, false, false);
              self.in_flight_seek_seq = Some(self.latest_sequence_id);
  ```

- [ ] **Step 2: Run the benchmark again to measure new performance**
  Run the benchmark using the target video `bbb_sunflower_2160p_60fps_normal.mp4`:
  `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/bbb_sunflower_2160p_60fps_normal.mp4" cargo run --release --bin bench_arrow_seek`
  Expected: Scenario 4 runs at real-time speeds (>60 FPS).

- [ ] **Step 3: Verify the project tests**
  Run: `cargo test`
  Expected: All tests pass.

- [ ] **Step 4: Commit changes**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): pause playback properly when entering arrow seek states"
  ```
