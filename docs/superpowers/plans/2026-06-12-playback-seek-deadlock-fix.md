# Playback Seek Deadlock and State Sync Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix playback freezing/pausing when seeking during active playback, ensuring seeking resumes playing if the video was playing prior to the seek interaction.

**Architecture:** 
1. Preserve the `play_after_seek` flag in the presenter thread across multiple back-to-back seeks by performing a logical OR: `play_after_seek = playing || play_after_seek`.
2. Add a `was_playing_before_scrub` state variable to the UI thread (`MainApp`), storing the playback state when a drag or click interaction starts (`drag_started()`), and restoring it when the interaction completes (`drag_stopped()` or `clicked()`) by invoking `set_playing()`.

**Tech Stack:** Rust, egui, crossbeam-channel

---

### Task 1: Presenter Thread Seek State Preservation

**Files:**
- Modify: `src/media/mod.rs`
- Test: `src/bin/test_playback_sync.rs`

- [ ] **Step 1: Modify `play_after_seek` flag logic in `src/media/mod.rs`**
  Modify line 214 to preserve `play_after_seek` state across back-to-back seeks:
  ```rust
  play_after_seek = playing || play_after_seek;
  ```

- [ ] **Step 2: Run the test suite to verify the double seek test now passes**
  Run: `SNIPPY_BENCH_VIDEO=/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv cargo run --bin test_playback_sync`
  Expected: All tests pass, and the output ends with `=== All Tests Passed Successfully ===`.

- [ ] **Step 3: Commit changes**
  Run:
  ```bash
  git add src/media/mod.rs src/bin/test_playback_sync.rs
  git commit -m "fix: preserve play_after_seek state across back-to-back seeks"
  ```

### Task 2: UI Thread Playback State Tracking & Restoration During Seek

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add `was_playing_before_scrub` field to `MainApp`**
  Add `pub was_playing_before_scrub: bool,` to `MainApp` struct:
  ```rust
  pub was_playing_before_scrub: bool,
  ```

- [ ] **Step 2: Initialize the field in `MainApp::new`**
  Initialize `was_playing_before_scrub: false,` in `MainApp::new` struct literal.

- [ ] **Step 3: Record state when scrub starts and restore it when drag stops/clicks finish**
  Inside `MainApp::update` in `src/ui/mod.rs`:
  Record `was_playing_before_scrub` on drag start:
  ```rust
  if progress_response.drag_started() || wave_response.drag_started() {
      self.was_playing_before_scrub = self.is_playing;
  }
  ```
  Restore `is_playing` and call `set_playing` on progress bar drag end / click:
  ```rust
  if progress_response.drag_stopped() || progress_response.clicked() {
      let target = self.latest_frame.as_ref().map(|f| f.frame_idx()).unwrap_or(self.current_frame);
      self.current_frame = target;
      self.is_playing = self.was_playing_before_scrub;
      self.seek_internal(target, false, true);
      self.set_playing(self.is_playing);
  }
  ```
  And similarly on waveform drag end / click:
  ```rust
  if wave_response.drag_stopped() || wave_response.clicked() {
      let target = self.latest_frame.as_ref().map(|f| f.frame_idx()).unwrap_or(self.current_frame);
      self.current_frame = target;
      self.is_playing = self.was_playing_before_scrub;
      self.seek_internal(target, false, true);
      self.set_playing(self.is_playing);
  }
  ```

- [ ] **Step 4: Verify compilation and tests**
  Run: `cargo check` and `cargo test`
  Expected: Success without errors.

- [ ] **Step 5: Commit changes**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "fix: track and restore playback state during seek/scrub in UI"
  ```
