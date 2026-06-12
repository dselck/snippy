# UI Scrub State Reset Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure that the UI's in-flight seek sequence ID and desired scrub frame are reset when dragging stops or a click is completed, allowing subsequent seeks to function.

**Architecture:** 
Reset `in_flight_seek_seq` and `desired_scrub_frame` to `None` in the drag stop / click completion handlers for both the progress bar and the waveform in `src/ui/mod.rs`.

**Tech Stack:** Rust, egui

---

### Task 1: UI Scrub State Reset

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Modify progress bar drag stop / click handler**
  In `src/ui/mod.rs`, update the progress bar handler to reset `in_flight_seek_seq` and `desired_scrub_frame`:
  ```rust
  if progress_response.drag_stopped() || progress_response.clicked() {
      let target = self.latest_frame.as_ref().map(|f| f.frame_idx()).unwrap_or(self.current_frame);
      self.current_frame = target;
      self.is_playing = self.was_playing_before_scrub;
      self.in_flight_seek_seq = None;
      self.desired_scrub_frame = None;
      self.seek_internal(target, false, true);
      self.set_playing(self.is_playing);
  }
  ```

- [ ] **Step 2: Modify waveform drag stop / click handler**
  In `src/ui/mod.rs`, update the waveform handler to reset `in_flight_seek_seq` and `desired_scrub_frame`:
  ```rust
  if wave_response.drag_stopped() || wave_response.clicked() {
      let target = self.latest_frame.as_ref().map(|f| f.frame_idx()).unwrap_or(self.current_frame);
      self.current_frame = target;
      self.is_playing = self.was_playing_before_scrub;
      self.in_flight_seek_seq = None;
      self.desired_scrub_frame = None;
      self.seek_internal(target, false, true);
      self.set_playing(self.is_playing);
  }
  ```

- [ ] **Step 3: Verify compilation and run test suite**
  Run: `SNIPPY_BENCH_VIDEO=/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv cargo run --bin test_playback_sync`
  Expected: Success.

- [ ] **Step 4: Commit changes**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "fix: reset scrub state when drag stops or click finishes"
  ```
