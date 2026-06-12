# Audio Sync and Deadlock Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve the startup audio sync/silence issue and the media thread seek deadlock.

**Architecture:** 
1. Clear the `audio_paused_for_sync` flag when `MasterClock::play()` is called so the audio thread immediately activates.
2. Rewrite the media engine's audio push loop to check vacant queue space at the chunk level, and poll for decoder commands while waiting to prevent deadlocks.
3. Update the simulated audio consumer in the benchmark rig to discard stale samples on generation changes even when paused.

**Tech Stack:** Rust, `cpal`, `ringbuf`, `crossbeam_channel`, `ffmpeg-next`.

---

### Task 1: Master Clock Play Unpause

**Files:**
- Modify: `src/audio/clock.rs`

- [ ] **Step 1: Write a failing unit test**
  Add `test_play_clears_sync_pause` inside the `mod tests` block in `src/audio/clock.rs`:
  ```rust
      #[test]
      fn test_play_clears_sync_pause() {
          let (clock, ticker) = MasterClock::new(48000);
          clock.set_has_audio(true);
          clock.reset(0.0);
          assert!(!ticker.is_active(), "Ticker should be inactive after reset");
          clock.play();
          assert!(ticker.is_active(), "Ticker should be active after play");
      }
  ```

- [ ] **Step 2: Run the test to verify it fails**
  Run: `cargo test audio::clock::tests::test_play_clears_sync_pause`
  Expected: FAIL (assertion `ticker.is_active()` is false because `audio_paused_for_sync` is still true).

- [ ] **Step 3: Implement the fix in MasterClock::play()**
  Modify `play()` in `src/audio/clock.rs` to clear the `audio_paused_for_sync` flag:
  ```rust
      pub fn play(&self) {
          self.state.is_playing.store(true, Ordering::Relaxed);
          self.state.audio_paused_for_sync.store(false, Ordering::SeqCst);
          let start_us = self.state.fallback_start_us.load(Ordering::Relaxed);
          if start_us == 0 {
              let now_us = std::cmp::max(1, self.state.fallback_epoch.elapsed().as_micros() as u64);
              self.state.fallback_start_us.store(now_us, Ordering::Relaxed);
          }
      }
  ```

- [ ] **Step 4: Run the test to verify it passes**
  Run: `cargo test audio::clock::tests::test_play_clears_sync_pause`
  Expected: PASS.

- [ ] **Step 5: Commit changes**
  Run:
  ```bash
  git add src/audio/clock.rs
  git commit -m "fix(clock): clear audio_paused_for_sync on MasterClock::play"
  ```

---

### Task 2: Simulated Consumer Generation Drop in Benchmark

**Files:**
- Modify: `src/bench/mod.rs`

- [ ] **Step 1: Update simulated consumer thread**
  Modify the simulated consumer thread in `MediaTestRig::new` at `src/bench/mod.rs` to pop and discard stale samples when the generation changes, even if `active` is false:
  ```rust
              let audio_consumer = thread::spawn(move || {
                  let sample_rate = 48000;
                  let mut start_time = Instant::now();
                  let mut samples_read_total = 0u64;
                  let mut last_tag = None;
                  let mut pending_tag: Option<(u64, u64, u64)> = None;
                  let mut last_active = false;
                  let mut current_consumer_generation = 0;

                  loop {
                      let active = ticker_clone.is_active();
                      
                      // Always drop stale samples if the generation changed, even when inactive
                      let producer_gen = ticker_clone.playback_generation();
                      if producer_gen != current_consumer_generation {
                          while let Some((_, generation)) = audio_cons.try_pop() {
                              if generation == producer_gen {
                                  break;
                              }
                          }
                          samples_read_total = 0;
                          last_tag = None;
                          pending_tag = None;
                          current_consumer_generation = producer_gen;
                      }

                      if !active {
                          last_active = false;
                          thread::sleep(Duration::from_millis(5));
                          continue;
                      }

                      let elapsed = start_time.elapsed();
  ```

- [ ] **Step 2: Verify compilation and tests**
  Run: `cargo test`
  Expected: PASS.

- [ ] **Step 3: Commit changes**
  Run:
  ```bash
  git add src/bench/mod.rs
  git commit -m "fix(bench): simulated audio consumer drops stale samples when inactive"
  ```

---

### Task 3: Non-Deadlocking Audio Queue Push Loop

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Rewrite audio sample push loop**
  Replace lines 500-514 in `src/media/mod.rs` with the non-deadlocking chunk-level check and command poller:
  ```rust
                                          let mut chunk_pushed = false;
                                          while !chunk_pushed {
                                              if audio_tx.vacant_len() >= chunk.data.len() {
                                                  for &sample in &chunk.data {
                                                      let _ = audio_tx.try_push((sample, generation));
                                                  }
                                                  chunk_pushed = true;
                                              } else {
                                                  // Queue is full. Poll command channel to prevent deadlock.
                                                  if let Ok(cmd) = decoder_cmd_rx.try_recv() {
                                                      match cmd {
                                                          MediaCommand::Shutdown => return Ok(()),
                                                          MediaCommand::Play => {
                                                              video_pipeline.play();
                                                          }
                                                          MediaCommand::Pause => {
                                                              video_pipeline.pause();
                                                          }
                                                          MediaCommand::Seek { .. } => {
                                                              pending_cmd = Some(cmd);
                                                              break;
                                                          }
                                                      }
                                                  }
                                                  if pending_cmd.is_some() {
                                                      break;
                                                  }
                                                  std::thread::sleep(std::time::Duration::from_millis(2));
                                              }
                                          }
                                          if pending_cmd.is_some() {
                                              break;
                                          }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo build`
  Expected: Successful compilation.

- [ ] **Step 3: Commit changes**
  Run:
  ```bash
  git add src/media/mod.rs
  git commit -m "fix(media): rewrite audio push loop to check vacant space and poll commands to prevent deadlock"
  ```

---

### Task 4: Integration Verification

- [ ] **Step 1: Run integration sync test with real video**
  Run: `SNIPPY_BENCH_VIDEO=/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv cargo run --bin test_playback_sync`
  Expected: PASS, exact seeks succeed, sequential play decodes correctly.

- [ ] **Step 2: Run playback jitter bench with real video**
  Run: `SNIPPY_BENCH_VIDEO=/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv cargo run --bin bench_playback_jitter`
  Expected: PASS, frame arrival statistics print, and median absolute timing jitter is < 5ms.
