# Audio-Video Sync & Smooth Scrubbing Implementation Plan (v2 - Tagged-Sample Clock)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement absolute microsecond sample tagging, CPAL callback verification, and pause-for-sync logic to guarantee frame-accurate AV synchronization.

**Architecture:** Replace the sample count counter with `current_position_us`. Tag the audio frames with PTS in the decoder thread. Verify and align the clock in the CPAL callback. Pause CPAL audio consumption on seek until the UI displays the seek target video frame.

**Tech Stack:** Rust, FFmpeg, CPAL

---

### Task 1: Update `AudioPlaybackState` in `src/audio/mod.rs`

**Files:**
- Modify: `src/audio/mod.rs` (file:///Users/dselck/Documents/Snippy2/src/audio/mod.rs)

- [x] **Step 1: Update `AudioPlaybackState` struct fields**
  Replace `samples_played` with `current_position_us`, and add `audio_paused_for_sync`:
  ```rust
  pub struct AudioPlaybackState {
      /// Current absolute playback position in microseconds since the start of the file.
      pub current_position_us: Arc<AtomicU64>,
      /// Sample rate of the output device (e.g., 48000).
      pub sample_rate: u32,
      pub has_audio: Arc<AtomicBool>,
      pub fallback_start_time: Arc<std::sync::Mutex<Option<Instant>>>,
      pub fallback_base_time: Arc<std::sync::Mutex<f64>>,
      pub should_flush: Arc<AtomicBool>,
      pub is_playing: Arc<AtomicBool>,
      pub audio_paused_for_sync: Arc<AtomicBool>,
  }
  ```

- [x] **Step 2: Update `current_time` implementation**
  Modify `current_time` to convert microseconds to seconds:
  ```rust
      pub fn current_time(&self) -> f64 {
          if self.has_audio.load(Ordering::Relaxed) {
              let us = self.current_position_us.load(Ordering::Relaxed) as f64;
              us / 1_000_000.0
          } else {
              let lock = self.fallback_start_time.lock().unwrap();
              let base = *self.fallback_base_time.lock().unwrap();
              if let Some(start) = *lock {
                  base + start.elapsed().as_secs_f64()
              } else {
                  base
              }
          }
      }
  ```

- [x] **Step 3: Update `reset` implementation**
  Set `audio_paused_for_sync` to `true`, store microseconds, and clear fallback clock:
  ```rust
      pub fn reset(&self, time_seconds: f64) {
          self.current_position_us.store((time_seconds * 1_000_000.0) as u64, Ordering::Relaxed);
          let mut base = self.fallback_base_time.lock().unwrap();
          *base = time_seconds;
          let mut lock = self.fallback_start_time.lock().unwrap();
          *lock = None; // Always pause fallback clock on seek/reset
          self.audio_paused_for_sync.store(true, Ordering::SeqCst);
          self.should_flush.store(true, Ordering::SeqCst);
      }
  ```

- [x] **Step 4: Run `cargo check` to verify**
  Verify compilation. It should fail on `AudioEngine::new` due to missing struct fields.

---

### Task 2: Create Tags Queue and Implement Tagging in CPAL Callback

**Files:**
- Modify: `src/audio/mod.rs` (file:///Users/dselck/Documents/Snippy2/src/audio/mod.rs)

- [x] **Step 1: Update `AudioEngine` struct**
  Add `tags_tx` to `AudioEngine`:
  ```rust
  pub struct AudioEngine {
      pub stream: cpal::Stream,
      pub tx: Option<CachingProd<Arc<SharedRb<Heap<f32>>>>>,
      pub tags_tx: Option<CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>>,
      pub playback_state: Arc<AudioPlaybackState>,
  }
  ```

- [x] **Step 2: Update `AudioEngine::new` to initialize the tags queue and callback**
  Create `tags_rb`, capture tagging variables in the closure, and verify/update `current_position_us` inside the CPAL callback:
  ```rust
          // Create tags ring buffer: tags hold (frames_written_total, pts_us)
          let tags_rb = SharedRb::<Heap<(u64, u64)>>::new(256);
          let (tags_prod, mut tags_cons) = tags_rb.split();

          let current_position_us = Arc::new(AtomicU64::new(0));
          let current_position_us_callback = Arc::clone(&current_position_us);
          let should_flush = Arc::new(AtomicBool::new(false));
          let should_flush_callback = Arc::clone(&should_flush);
          let is_playing = Arc::new(AtomicBool::new(false));
          let is_playing_callback = Arc::clone(&is_playing);
          let audio_paused_for_sync = Arc::new(AtomicBool::new(false));
          let audio_paused_for_sync_callback = Arc::clone(&audio_paused_for_sync);

          let mut samples_read_total = 0u64; // In frames
          let mut last_tag: Option<(u64, u64)> = None;
          let mut pending_tag: Option<(u64, u64)> = None;

          // CPAL high-priority real-time audio thread callback
          let stream = device.build_output_stream(
              &config.into(),
              move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                  if should_flush_callback.swap(false, Ordering::SeqCst) {
                      while cons.try_pop().is_some() {}
                      while tags_cons.try_pop().is_some() {}
                      samples_read_total = 0;
                      last_tag = None;
                      pending_tag = None;
                  }
                  let mut written = 0;
                  let active = is_playing_callback.load(Ordering::Relaxed)
                      && !audio_paused_for_sync_callback.load(Ordering::Relaxed);
                  for frame in data.chunks_mut(channels) {
                      if active {
                          if let Some(sample) = cons.try_pop() {
                              for channel_sample in frame.iter_mut() {
                                  *channel_sample = sample;
                              }
                              written += 1;
                              samples_read_total += 1;

                              // Process tags
                              loop {
                                  let next_tag = pending_tag.take().or_else(|| tags_cons.try_pop());
                                  if let Some((tag_idx, tag_pts_us)) = next_tag {
                                      if tag_idx <= samples_read_total {
                                          last_tag = Some((tag_idx, tag_pts_us));
                                      } else {
                                          pending_tag = Some((tag_idx, tag_pts_us));
                                          break;
                                      }
                                  } else {
                                      break;
                                  }
                              }
                          } else {
                              for channel_sample in frame.iter_mut() {
                                  *channel_sample = 0.0;
                              }
                          }
                      } else {
                          for channel_sample in frame.iter_mut() {
                              *channel_sample = 0.0;
                          }
                      }
                  }
                  if active {
                      if let Some((tag_idx, tag_pts_us)) = last_tag {
                          let elapsed_samples = samples_read_total.saturating_sub(tag_idx);
                          let elapsed_us = (elapsed_samples * 1_000_000) / (sample_rate as u64);
                          let current_pos_us = tag_pts_us + elapsed_us;
                          current_position_us_callback.store(current_pos_us, Ordering::Relaxed);
                      } else {
                          let elapsed_us = (written as u64 * 1_000_000) / (sample_rate as u64);
                          current_position_us_callback.fetch_add(elapsed_us, Ordering::Relaxed);
                      }
                  }
              },
              |err| eprintln!("CPAL audio stream error: {:?}", err),
              None,
          )?;

          stream.play()?;

          let has_audio = Arc::new(AtomicBool::new(false));
          let fallback_start_time = Arc::new(std::sync::Mutex::new(None));
          let fallback_base_time = Arc::new(std::sync::Mutex::new(0.0));

          Ok(Self {
              stream,
              tx: Some(prod),
              tags_tx: Some(tags_prod),
              playback_state: Arc::new(AudioPlaybackState {
                  current_position_us,
                  sample_rate,
                  has_audio,
                  fallback_start_time,
                  fallback_base_time,
                  should_flush,
                  is_playing,
                  audio_paused_for_sync,
              }),
          })
  ```

- [x] **Step 3: Run `cargo check` to verify**
  Verify compilation. It should fail only on `MediaEngine::new` instantiation in `src/main.rs`.

---

### Task 3: Implement Tagging in Audio Decoder Thread

**Files:**
- Modify: `src/media/mod.rs` (file:///Users/dselck/Documents/Snippy2/src/media/mod.rs)

- [x] **Step 1: Update `MediaEngine::new` signature & caller**
  Update `MediaEngine::new` parameters to receive `tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>`.
  Update `run_decoding_loop` to receive `mut tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>`.

- [x] **Step 2: Update tagging logic in audio decoder thread**
  Initialize `frames_written_total: u64` in the audio thread loop.
  Reset it on `Seek` commands.
  Push a tag `(frames_written_total, pts_us)` before writing samples to `audio_tx`:
  ```rust
              let mut frames_written_total = 0u64;
              // loop ...
                  // Seek ...
                      frames_written_total = 0;
                  // Inside receive_frame loop ...
                                          if is_first_audio_frame {
                                              let target_container_time = video_start_time + skip_until_time;
                                              if pts_sec < target_container_time {
                                                  continue; // Discard whole frame before seek target
                                              }
                                              is_first_audio_frame = false;
                                              let stream_time = pts_sec - video_start_time;
                                              playback_state.current_position_us.store((stream_time * 1_000_000.0) as u64, std::sync::atomic::Ordering::SeqCst);
                                          }
                                          
                                          let pcm_frames = resampled.samples();
                                          let pts_us = (pts_sec * 1_000_000.0) as u64;
                                          let _ = tags_tx.try_push((frames_written_total, pts_us));
                                          
                                          // Push samples ...
                                          for &sample in data { ... }
                                          frames_written_total += pcm_frames as u64;
  ```

- [x] **Step 3: Update `MediaEngine` initialization in `src/main.rs`**
  Modify `src/main.rs` (file:///Users/dselck/Documents/Snippy2/src/main.rs) to extract `tags_tx` from `audio_engine` and pass it to `MediaEngine::new`:
  ```rust
  let tags_tx = audio_engine.tags_tx.take().unwrap();
  // Pass to MediaEngine::new
  ```

- [x] **Step 4: Run `cargo check` to verify**
  Verify compilation. It should compile successfully except for tests and UI thread integrations.

---

### Task 4: Integrate Sync Pause in UI Thread & Verify

**Files:**
- Modify: `src/ui/mod.rs` (file:///Users/dselck/Documents/Snippy2/src/ui/mod.rs)
- Modify: `src/bin/test_playback_sync.rs` (file:///Users/dselck/Documents/Snippy2/src/bin/test_playback_sync.rs)

- [x] **Step 1: Update clock starting logic in `src/ui/mod.rs`**
  Modify lines 733-736 in `src/ui/mod.rs` to only start the clock if `audio_paused_for_sync` is false:
  ```rust
              if !self.playback_state.audio_paused_for_sync.load(std::sync::atomic::Ordering::SeqCst) {
                  let running = self.playback_state.is_playing.swap(true, std::sync::atomic::Ordering::SeqCst);
                  if !running {
                      self.playback_state.start_fallback_clock();
                  }
              }
  ```

- [x] **Step 2: Add catch-up check after receiving frames**
  At the end of background frames processing (around line 777), add catch-up check to clear `audio_paused_for_sync`:
  ```rust
              if self.playback_state.audio_paused_for_sync.load(std::sync::atomic::Ordering::SeqCst) {
                  let mut caught_up = false;
                  if let Some(ref f) = self.latest_frame {
                      if f.sequence_id() == self.latest_sequence_id {
                          caught_up = true;
                      }
                  }
                  if caught_up {
                      self.playback_state.audio_paused_for_sync.store(false, std::sync::atomic::Ordering::SeqCst);
                      if self.is_playing {
                          self.playback_state.start_fallback_clock();
                      }
                  }
              }
  ```

- [x] **Step 3: Update `test_playback_sync.rs`**
  Modify `AudioPlaybackState` instantiation to include `audio_paused_for_sync` and `current_position_us`, and extract `tags_tx` to pass to `MediaEngine::new`.

- [x] **Step 4: Run `cargo check` to verify**
  Verify compilation.

- [x] **Step 5: Run integration test to verify seek behavior**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS

- [x] **Step 6: Commit all changes**
  Run:
  ```bash
  git add src/media/mod.rs src/audio/mod.rs src/ui/mod.rs src/main.rs src/bin/test_playback_sync.rs
  git commit -m "fix: implement microseconds absolute sample tagging and seek-start catchup synchronization"
  ```
