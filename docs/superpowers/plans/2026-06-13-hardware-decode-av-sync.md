# Hardware Decoding Audio-Video Synchronization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Standardize timing and synchronization across both CPU and hardware (VideoToolbox) decoders by using a robust, ground-truth fallback chain for extracted presentation timestamps.

**Architecture:** We are updating the video frame and audio sample receiver logic to read the best-effort presentation timestamp computed by FFmpeg (`best_effort_timestamp`) as the primary timing reference. When that is missing (e.g., under some hardware decoders or seek states), we fall back to raw PTS, then to the packet's Decoding Time Stamp (`pkt_dts`), and only as a final resort do we use nominal calculated approximations.

**Tech Stack:** Rust, ffmpeg-next.

---

### Task 1: Implement Timestamp Fallback Chain in Video Decoder Adapter

**Files:**
- Modify: `src/media/ffmpeg_adapter.rs`

- [ ] **Step 1: Modify `FfmpegDecoderAdapter::receive_frame`**
  Modify `receive_frame` in `src/media/ffmpeg_adapter.rs` to extract timestamps using the fallback chain: `frame.timestamp()` -> `frame.pts()` -> `pkt_dts` -> nominal fallback.

  Replace lines ~41-44:
  ```rust
                  let pts_sec = match frame.pts() {
                      Some(pts_val) => pts_val as f64 * self.time_base_f64 - self.video_start_time,
                      None => self.fallback_frame_idx as f64 / self.fps,
                  };
  ```
  with:
  ```rust
                  let pts_sec = if let Some(timestamp) = frame.timestamp() {
                      timestamp as f64 * self.time_base_f64 - self.video_start_time
                  } else if let Some(pts) = frame.pts() {
                      pts as f64 * self.time_base_f64 - self.video_start_time
                  } else {
                      let pkt_dts = unsafe { (*frame.as_ptr()).pkt_dts };
                      if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                          pkt_dts as f64 * self.time_base_f64 - self.video_start_time
                      } else {
                          self.fallback_frame_idx as f64 / self.fps
                      }
                  };
  ```

- [ ] **Step 2: Run compilation check**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/media/ffmpeg_adapter.rs
  git commit -m "refactor: use robust fallback timestamp chain in video decoder adapter"
  ```

---

### Task 2: Implement Timestamp Fallback Chain in Audio Pipeline

**Files:**
- Modify: `src/media/audio_pipeline.rs`

- [ ] **Step 1: Modify `AudioPipeline::receive_samples`**
  Modify the `receive_samples` loop in `src/media/audio_pipeline.rs` to extract audio frame timestamps using the same fallback chain.

  Replace lines ~71-74:
  ```rust
              let pts_sec = match frame.pts() {
                  Some(pts) => pts as f64 * self.audio_tb_f64,
                  None => self.current_audio_time,
              };
  ```
  with:
  ```rust
              let pts_sec = if let Some(timestamp) = frame.timestamp() {
                  timestamp as f64 * self.audio_tb_f64
              } else if let Some(pts) = frame.pts() {
                  pts as f64 * self.audio_tb_f64
              } else {
                  let pkt_dts = unsafe { (*frame.as_ptr()).pkt_dts };
                  if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                      pkt_dts as f64 * self.audio_tb_f64
                  } else {
                      self.current_audio_time
                  }
              };
  ```

- [ ] **Step 2: Run compilation check**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/media/audio_pipeline.rs
  git commit -m "refactor: use robust fallback timestamp chain in audio pipeline"
  ```

---

### Task 3: Verify the Changes Against Test Suite

**Files:**
- Run tests only.

- [ ] **Step 1: Run the full test suite**
  Run: `cargo test`
  Expected: PASS. All 12 unit tests should continue to pass seamlessly.

- [ ] **Step 2: Run existing benchmarks to verify no performance regression**
  Run: `cargo check --bins`
  Expected: PASS.

- [ ] **Step 3: Final Git Check**
  Run: `git status`
  Expected: Clean status except for modified files and new plan/docs.
