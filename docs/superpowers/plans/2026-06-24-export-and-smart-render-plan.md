# Export and Smart Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the video export functionality with Smart Rendering (splicing video GOP-by-GOP, stream-copying Fully Kept GOPs, and decoding/re-encoding Partially Kept GOPs). Perform export on a background thread with progress feedback and cancellation.

**Architecture:**
1. Define `ExportStatus` in `src/media/types.rs`.
2. Implement the core export logic in `src/media/export.rs` using `ffmpeg-next`.
3. Wire the export function to the egui frontend in `src/ui/mod.rs` using a background thread and an interactive progress/cancellation modal.

**Tech Stack:** Rust, ffmpeg-next, egui, eframe

---

### Task 1: Define Export Status and Setup Export Module

**Files:**
* Modify: `src/media/types.rs`
* Modify: `src/media/mod.rs`
* Create: `src/media/export.rs`

- [ ] **Step 1: Define `ExportStatus`**
  Add the following enum to `src/media/types.rs`:
  ```rust
  #[derive(Debug, Clone, PartialEq)]
  pub enum ExportStatus {
      Idle,
      Exporting { progress: f32, message: String },
      Completed,
      Failed(String),
      Cancelled,
  }
  ```

- [ ] **Step 2: Add `export` module declaration**
  Expose the `export` module in `src/media/mod.rs`:
  ```rust
  pub mod export;
  pub use export::export_video;
  ```

- [ ] **Step 3: Create `src/media/export.rs` boilerplate**
  Write a stub for `export_video` in `src/media/export.rs`:
  ```rust
  use std::sync::Arc;
  use std::sync::atomic::AtomicBool;
  use std::sync::Mutex;
  use crate::media::types::{ExportStatus, VideoMetadata};
  use crate::ui::ExportRange;

  pub fn export_video(
      input_path: &str,
      output_path: &str,
      ranges: Vec<ExportRange>,
      cut_mode: bool,
      metadata: VideoMetadata,
      status: Arc<Mutex<ExportStatus>>,
      cancel_flag: Arc<AtomicBool>,
  ) -> Result<(), anyhow::Error> {
      // Implementation stub
      Ok(())
  }
  ```

- [ ] **Step 4: Verify compilation**
  Run `cargo check` to verify imports and syntax.

---

### Task 2: Implement Smart Render Core in `src/media/export.rs`

**Files:**
* Modify: `src/media/export.rs`

- [ ] **Step 1: Classify GOPs**
  Map GOP boundaries to keep/cut states. A GOP starts at `metadata.keyframes[i]` and ends before `metadata.keyframes[i+1]` (or `metadata.total_frames`).
  Define `GopType`:
  ```rust
  #[derive(Clone, Copy, Debug, PartialEq)]
  enum GopType {
      FullyKept,
      FullyCut,
      PartiallyKept,
  }
  ```
  Implement classification:
  ```rust
  let mut gop_types = Vec::new();
  for i in 0..metadata.keyframes.len() {
      let start_frame = metadata.keyframes[i];
      let end_frame = if i + 1 < metadata.keyframes.len() {
          metadata.keyframes[i + 1]
      } else {
          metadata.total_frames
      };
      
      let mut kept_count = 0;
      let mut cut_count = 0;
      for f in start_frame..end_frame {
          if crate::ui::is_frame_cut_helper(f, &ranges, cut_mode) {
              cut_count += 1;
          } else {
              kept_count += 1;
          }
      }
      
      let gop_type = if kept_count == (end_frame - start_frame) {
          GopType::FullyKept
      } else if cut_count == (end_frame - start_frame) {
          GopType::FullyCut
      } else {
          GopType::PartiallyKept
      };
      gop_types.push(gop_type);
  }
  ```

- [ ] **Step 2: Initialize Input and Output Containers**
  Open the input media file and setup the output container. Set up video and audio streams by duplicating codec parameters:
  ```rust
  let mut input = ffmpeg_next::format::input(&input_path)?;
  let mut output = ffmpeg_next::format::output(&output_path)?;

  // Add streams using input stream templates
  // Track mapped stream indexes
  ```

- [ ] **Step 3: Setup Re-Encoding Video Pipeline**
  If any GOP is `PartiallyKept`, we need a decoder context and encoder context.
  - Locate the native encoder matching the input video's codec ID.
  - Configure the encoder context to match the input's original pixel format (8-bit or 10-bit), resolution, colorspace, color range, and frame rate.
  - Use high-quality CRF settings (e.g. CRF 21 for H.264, 24 for H.265).

- [ ] **Step 4: Muxing Loop**
  Loop through input packets:
  - Calculate `frame_index` for video packets using `pts` and stream `time_base`.
  - Find the matching `gop_index` by locating the nearest keyframe less than or equal to `frame_index`.
  - Process video packets:
    - **FullyKept:** Copy packet, adjust PTS/DTS relative to output segment, and mux.
    - **FullyCut:** Ignore.
    - **PartiallyKept:** Send to decoder, check if decoded frames are kept. If kept, send to encoder and mux output packets.
  - Process audio packets:
    - Keep if the audio packet PTS falls within a kept video frame's time range.
    - Adjust PTS/DTS and mux.
  - Constantly check `cancel_flag`. If set, clean up and delete output file.
  - Update `status` progress state periodically.

- [ ] **Step 5: Flush and Finalize**
  Flush any cached packets from the video encoder, write output trailer, and close containers.

- [ ] **Step 6: Verify compilation**
  Run `cargo check`.

---

### Task 3: UI Integration, Background Thread, and Progress Modal

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add Export fields to `MainApp`**
  Add state fields to track background progress:
  ```rust
  pub export_status: std::sync::Arc<std::sync::Mutex<crate::media::types::ExportStatus>>,
  pub export_cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
  ```
  Initialize these fields in `MainApp::new()`.

- [ ] **Step 2: Add Export button and file dialog trigger**
  Add an "Export" button next to "Add Snippet" or in the header action panel.
  When clicked:
  ```rust
  if let Some(path) = rfd::FileDialog::new()
      .add_filter("Video File", &["mp4", "mkv", "ts", "mov"])
      .save_file()
  {
      // Set status to Exporting, reset cancel flag
      // Spawn background thread to run export_video
  }
  ```

- [ ] **Step 3: Render Progress Overlay Modal**
  In the `update()` loop of `MainApp`, check if the status is `Exporting`:
  ```rust
  let status_guard = self.export_status.lock().unwrap();
  if let crate::media::types::ExportStatus::Exporting { progress, message } = &*status_guard {
      // Show modal window with a ProgressBar, message text, and Cancel button
  }
  ```
  If "Cancel" is clicked:
  ```rust
  self.export_cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
  ```

- [ ] **Step 4: Handle Export Completion/Failure**
  In `update()`, check if the status is `Completed` or `Failed`:
  - If `Completed`: Show a success notification and set status to `Idle`.
  - If `Failed(msg)`: Show an error notification with the failure reason and set status to `Idle`.
  - If `Cancelled`: Show a cancellation notification and set status to `Idle`.

---

### Task 4: Testing & Validation

- [ ] **Step 1: Write Unit/Integration Tests**
  Create a test in `src/media/export.rs` or `tests/export_tests.rs` to verify that slicing works correctly with simulated or small test files.
- [ ] **Step 2: Run the test suite**
  Run `cargo test` to ensure all tests compile and pass.
- [ ] **Step 3: Manual Testing**
  Run Snippy2, load a video, make a few cuts, and verify exporting to `.mp4` or `.mkv` results in a correct, playable, and frame-aligned output video.
