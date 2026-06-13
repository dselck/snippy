# Benchmark Arrow Seek Model Reality Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update `bench_arrow_seek` to measure the actual rate at which frames are advanced/stepped through (including skipped frames) during unpaced forward seek, rather than capping the measured FPS to the VSync display rate.

**Architecture:** 
In `simulate_arrow_hold_gui`, we count every frame received from the media engine's frame channel, even those skipped during buffer draining. The benchmark loop runs until the target number of frames has been *received/processed* (representing the seek progress), and the FPS is calculated based on the total frames received over elapsed time.

**Tech Stack:** Rust.

---

### Task 1: Update Benchmark GUI Simulation

**Files:**
- Modify: `src/bin/bench_arrow_seek.rs:56-140`

- [ ] **Step 1: Modify `simulate_arrow_hold_gui` in `src/bin/bench_arrow_seek.rs`**
  Update `simulate_arrow_hold_gui` to track `frames_received` instead of `frames_displayed`, terminate based on `frames_received`, and calculate the FPS using `frames_received`:
  ```rust
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
      let mut frames_received = 0;

      let total_start = Instant::now();
      let duration_limit = Duration::from_secs(5);

      let mut unpaced_started = false;
      let mut send_times = std::collections::HashMap::<usize, Instant>::new();

      if direction > 0 {
          *seq += 1;
          unpaced_started = true;
          let _ = rig.send(MediaCommand::PlayUnpaced { sequence_id: *seq });
      }

      while frames_received < count && total_start.elapsed() < duration_limit {
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
                  frames_received += 1;
              }

              if let Some(frame) = latest_frame_received {
                  let frame_seq = frame.sequence_id();
                  if unpaced_started {
                      let frame_age = total_start.elapsed() / frames_received.max(1) as u32;
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

                  current = frame.frame_idx();
              }

              // 2. UI sends next seek unconditionally on every tick ONLY for backward seeking
              if !unpaced_started && frames_received < count {
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
      let actual_fps = frames_received as f64 / total_elapsed;

      (stats, actual_fps)
  }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Run the benchmark**
  Run: `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/bbb_sunflower_2160p_60fps_normal.mp4" cargo run --release --bin bench_arrow_seek`
  Expected: Scenario 2 (GUI Hold Seek Right) reports the actual unpaced decode framerate (e.g. 100+ FPS) since it is not bound by VSync.

- [ ] **Step 4: Commit changes**
  ```bash
  git add src/bin/bench_arrow_seek.rs
  git commit -m "bench: model reality by measuring decoded/skipped frame rate in arrow seek benchmark"
  ```
