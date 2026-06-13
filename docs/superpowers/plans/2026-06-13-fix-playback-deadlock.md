# Fix Playback Deadlock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Revert the audio ring buffer capacity back to 2.0 seconds and fix the `bench_decode` benchmark to correctly toggle clock playback state, resolving playback deadlocks and hung benchmark tests.

**Architecture:** 
1. Revert `SharedRb` buffer capacity in `src/audio/mod.rs`, `src/bench/mod.rs`, `src/bin/debug_audio.rs`, and `src/bin/test_playback_sync.rs` from 200ms to 2.0 seconds to accommodate larger audio decoding chunks and avoid deadlocks during sync stalls.
2. Fix `src/bin/bench_decode.rs` to start and pause the master clock in synchronization with the `Play` and `Pause` commands.

**Tech Stack:** Rust, cpal, ringbuf, ffmpeg-next

---

### Task 1: Revert Audio Ring Buffer Size to 2.0 Seconds

**Files:**
- Modify: `src/audio/mod.rs:28-28`
- Modify: `src/bench/mod.rs:68-68`
- Modify: `src/bin/debug_audio.rs:12-12`
- Modify: `src/bin/test_playback_sync.rs:17-17`

- [ ] **Step 1: Modify `src/audio/mod.rs`**
  Change the ring buffer initialization back to 2.0 seconds:
  ```rust
  let rb = SharedRb::<Heap<(f32, u64)>>::new(sample_rate as usize * channels * 2);
  ```

- [ ] **Step 2: Modify `src/bench/mod.rs`**
  Change the simulated ring buffer initialization to 2.0 seconds:
  ```rust
  let rb = SharedRb::<Heap<(f32, u64)>>::new(48000 * 2 * 2);
  ```

- [ ] **Step 3: Modify `src/bin/debug_audio.rs`**
  Change the debug ring buffer initialization to 2.0 seconds:
  ```rust
  let audio_rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(f32, u64)>>::new(48000 * 2 * 2);
  ```

- [ ] **Step 4: Modify `src/bin/test_playback_sync.rs`**
  Change the test ring buffer initialization to 2.0 seconds:
  ```rust
  let rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(f32, u64)>>::new(48000 * 2 * 2);
  ```

- [ ] **Step 5: Run tests to verify compilation**
  Run: `cargo test --lib`
  Expected: PASS

---

### Task 2: Fix Clock Playback Controls in `bench_decode`

**Files:**
- Modify: `src/bin/bench_decode.rs:17-29`
- Modify: `src/bin/bench_decode.rs:77-80`
- Modify: `src/bin/bench_decode.rs:123-126`
- Modify: `src/bin/bench_decode.rs:182-194`
- Modify: `src/bin/bench_decode.rs:227-229`

- [ ] **Step 1: Modify `start_playback` and `reset_to_start` in `src/bin/bench_decode.rs`**
  Modify helper functions to properly control the master clock state:
  ```rust
  fn reset_to_start(rig: &MediaTestRig) -> Result<(), anyhow::Error> {
      rig.send(MediaCommand::Pause)?;
      rig.master_clock.pause();
      rig.timed_seek(0, false, 0, Duration::from_secs(5));
      rig.drain_frames();
      Ok(())
  }

  fn start_playback(rig: &MediaTestRig, start_secs: f64) -> Result<(), anyhow::Error> {
      rig.master_clock.set_position(start_secs);
      rig.send(MediaCommand::Play)?;
      rig.master_clock.play();
      Ok(())
  }
  ```

- [ ] **Step 2: Add clock pause commands to scenario functions in `src/bin/bench_decode.rs`**
  Ensure the clock is paused when commands are paused at the end of each scenario.
  
  In `burst_decode`:
  ```rust
      let (frames, intervals) = collect_frames_for(rig, Duration::from_secs(5));
      let elapsed = wall_start.elapsed();

      rig.send(MediaCommand::Pause)?;
      rig.master_clock.pause();
  ```
  
  In `sustained_decode`:
  ```rust
      let (frames, intervals) = collect_frames_for(rig, Duration::from_secs(15));
      let elapsed = wall_start.elapsed();

      rig.send(MediaCommand::Pause)?;
      rig.master_clock.pause();
  ```
  
  In `decode_after_seek` (both before seek and at the end of the loop iteration):
  ```rust
          // Seek to position
          rig.send(MediaCommand::Pause)?;
          rig.master_clock.pause();
          let seq_id = 100 + i;
  ```
  and
  ```rust
          let elapsed = t0.elapsed();

          rig.send(MediaCommand::Pause)?;
          rig.master_clock.pause();
  ```
  
  In `gop_boundary_spikes`:
  ```rust
      let (frames, intervals) = collect_frames_for(rig, Duration::from_secs(3));

      rig.send(MediaCommand::Pause)?;
      rig.master_clock.pause();
  ```

- [ ] **Step 3: Run the benchmark to verify fix**
  Run: `SNIPPY_BENCH_VIDEO=/Users/dselck/Downloads/bbb_sunflower_2160p_60fps_normal.mp4 cargo run --release --bin bench_decode`
  Expected:
  - Benchmark completes rapidly
  - High FPS on burst decode and sustained decode
  - Decode-after-seek time is normal (e.g. ~10-100ms per seek, not 10.05s timeout)
  - No deadlocks or hangs

---
