# Video Scrubbing and GOP Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement keyframe scrubbing and frame-by-frame repeat stepping, accelerated by a GPU-memory-friendly GOP cache inside the video decoder thread to optimize reverse scrubbing performance.

**Architecture:** We will introduce a `GopCache` struct inside the video decoder thread, caching all decoded frames within the current Group of Pictures (GOP). Seeking backward/forward in the same GOP will bypass demuxer seeks and decode steps on cache hits. In the UI, holding the forward/backward Arrow keys or Prev/Next buttons for >250ms will advance frames as fast as possible on every UI repaint.

**Tech Stack:** Rust, egui, crossbeam-channel, FFmpeg.

---

### Task 1: Define GopCache and Helper in Media Engine

**Files:**
- Modify: `src/media/mod.rs` (around line 30)

- [x] **Step 1: Define GOP Cache structures**
  Add the following struct definitions near the top of `src/media/mod.rs` (under the imports and enum definitions):
  ```rust
  #[derive(Debug, Clone)]
  pub struct CachedFrame {
      pub planes: YuvPlanes,
      pub pts_sec: f64,
  }

  pub struct GopCache {
      pub keyframe_idx: usize,
      pub frames: std::collections::HashMap<usize, CachedFrame>,
  }
  ```

- [x] **Step 2: Add get_nearest_keyframe helper function**
  Add the following helper function at the end of `src/media/mod.rs`:
  ```rust
  fn get_nearest_keyframe(keyframes: &[usize], target: usize) -> usize {
      if keyframes.is_empty() {
          return target;
      }
      match keyframes.binary_search(&target) {
          Ok(i) => keyframes[i],
          Err(0) => keyframes[0],
          Err(i) => keyframes[i - 1],
      }
  }
  ```

- [x] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [x] **Step 4: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat: define GopCache structs and get_nearest_keyframe helper"
  ```

---

### Task 2: Update VideoThreadCommand and run_decoding_loop Signature

**Files:**
- Modify: `src/media/mod.rs` (around line 94 and 196)

- [x] **Step 1: Add audio_enabled to VideoThreadCommand::Seek**
  Update `VideoThreadCommand` in `src/media/mod.rs`:
  ```rust
  pub enum VideoThreadCommand {
      Seek {
          frame_index: usize,
          keyframe_only: bool,
          sequence_id: usize,
          audio_enabled: bool,
          resp_tx: Sender<bool>,
      },
      Play,
  }
  ```

- [x] **Step 2: Update run_decoding_loop Signature to accept Keyframes**
  Update `run_decoding_loop` signature and calls:
  ```rust
  fn run_decoding_loop(
      path: &str,
      target_sample_rate: u32,
      target_channels: usize,
      frame_tx: Sender<DecodedFrame>,
      mut audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
      mut tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>,
      background_rx: Receiver<MediaCommand>,
      _cmd_rx: Receiver<MediaCommand>,
      fps: f64,
      playback_state: Arc<crate::audio::AudioPlaybackState>,
      keyframes: Vec<usize>,
  ) -> Result<(), anyhow::Error> {
  ```

- [x] **Step 3: Update run_decoding_loop call in MediaEngine::new**
  Update the spawn block inside `MediaEngine::new` (around line 195):
  ```rust
          let keyframes_clone = keyframes.clone();
          let thread_handle = thread::spawn(move || {
              let _ = run_decoding_loop(
                  &path_str,
                  target_sample_rate,
                  target_channels,
                  frame_tx,
                  audio_tx,
                  tags_tx,
                  background_rx,
                  cmd_rx,
                  fps_clone,
                  playback_state_clone,
                  keyframes_clone,
              );
          });
  ```

- [x] **Step 4: Update VideoThreadCommand::Seek send in run_decoding_loop**
  Update the two places where the demuxer sends `VideoThreadCommand::Seek` inside `run_decoding_loop` (around lines 657 and 717):
  ```rust
              let _ = video_cmd_tx.send(VideoThreadCommand::Seek {
                  frame_index,
                  keyframe_only,
                  sequence_id,
                  audio_enabled,
                  resp_tx: video_resp_tx,
              });
  ```

- [x] **Step 5: Verify Compilation**
  Run: `cargo check`
  Expected: Fails only in `src/media/mod.rs` inside the video thread seek matches.

- [x] **Step 6: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat: add audio_enabled to VideoThreadCommand and pass keyframes to decoding loop"
  ```

---

### Task 3: Integrate GopCache in Video Thread Seek and Decode Loop

**Files:**
- Modify: `src/media/mod.rs` (around lines 265 to 425)

- [x] **Step 1: Initialize gop_cache inside Video Thread**
  Add the variable initialization near the top of the video thread `thread::spawn` (around line 265):
  ```rust
          let mut gop_cache = GopCache {
              keyframe_idx: usize::MAX,
              frames: std::collections::HashMap::new(),
          };
  ```

- [x] **Step 2: Update the first Seek command match in receive_frame loop**
  Update lines 349-357 to use `gop_cache`:
  ```rust
                                                      VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae, resp_tx } => {
                                                          let target_kf = get_nearest_keyframe(&keyframes, frame_index);
                                                          if gop_cache.keyframe_idx != target_kf {
                                                              gop_cache.frames.clear();
                                                              gop_cache.keyframe_idx = target_kf;
                                                          }

                                                          let mut cache_hit = false;
                                                          if !ae {
                                                              if let Some(cached) = gop_cache.frames.get(&frame_index) {
                                                                  let decoded = DecodedFrame::Cpu {
                                                                      frame_idx: frame_index,
                                                                      planes: cached.planes.clone(),
                                                                      sequence_id,
                                                                      pts_sec: cached.pts_sec,
                                                                  };
                                                                  if frame_tx_clone.send(decoded).is_ok() {
                                                                      let _ = resp_tx.send(true);
                                                                      fallback_frame_idx = frame_index;
                                                                      keyframe_only_mode = keyframe_only;
                                                                      current_sequence_id = sequence_id;
                                                                      decode_more = false;
                                                                      cache_hit = true;
                                                                  }
                                                              }
                                                          }

                                                          if !cache_hit {
                                                              fallback_frame_idx = frame_index;
                                                              keyframe_only_mode = keyframe_only;
                                                              current_sequence_id = sequence_id;
                                                              while video_packet_rx.try_recv().is_ok() {}
                                                              video_decoder.flush();
                                                              let _ = resp_tx.send(false);
                                                              decode_more = true;
                                                          }
                                                      }
  ```

- [x] **Step 3: Update the second and third Seek command matches in the outer loops**
  Update the match blocks around lines 386-395 and lines 406-414 with the same logic:
  ```rust
                              Ok(VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae, resp_tx }) => {
                                  let target_kf = get_nearest_keyframe(&keyframes, frame_index);
                                  if gop_cache.keyframe_idx != target_kf {
                                      gop_cache.frames.clear();
                                      gop_cache.keyframe_idx = target_kf;
                                  }

                                  let mut cache_hit = false;
                                  if !ae {
                                      if let Some(cached) = gop_cache.frames.get(&frame_index) {
                                          let decoded = DecodedFrame::Cpu {
                                              frame_idx: frame_index,
                                              planes: cached.planes.clone(),
                                              sequence_id,
                                              pts_sec: cached.pts_sec,
                                          };
                                          if frame_tx_clone.send(decoded).is_ok() {
                                              let _ = resp_tx.send(true);
                                              fallback_frame_idx = frame_index;
                                              keyframe_only_mode = keyframe_only;
                                              current_sequence_id = sequence_id;
                                              decode_more = false;
                                              cache_hit = true;
                                          }
                                      }
                                  }

                                  if !cache_hit {
                                      fallback_frame_idx = frame_index;
                                      keyframe_only_mode = keyframe_only;
                                      current_sequence_id = sequence_id;
                                      while video_packet_rx.try_recv().is_ok() {}
                                      video_decoder.flush();
                                      let _ = resp_tx.send(false);
                                      decode_more = true;
                                  }
                              }
  ```

- [x] **Step 4: Insert decoded frames into GOP Cache**
  Inside the frame decoding block (around line 320, right after `let frame_idx = (pts_sec * fps).round() as usize;`), insert the frame into the cache:
  ```rust
                                     if keyframes.binary_search(&frame_idx).is_ok() {
                                         if gop_cache.keyframe_idx != frame_idx {
                                             gop_cache.frames.clear();
                                             gop_cache.keyframe_idx = frame_idx;
                                         }
                                     }
                                     if gop_cache.keyframe_idx == usize::MAX {
                                         gop_cache.keyframe_idx = get_nearest_keyframe(&keyframes, frame_idx);
                                     }
                                     if frame_idx >= gop_cache.keyframe_idx {
                                         gop_cache.frames.insert(frame_idx, CachedFrame {
                                             planes: planes.clone(),
                                             pts_sec,
                                         });
                                     }
  ```

- [x] **Step 5: Verify Compilation**
  Run: `cargo check`
  Expected: PASS

- [x] **Step 6: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "feat: integrate GopCache into video decoder thread seek and decode loops"
  ```

---

### Task 4: Modify playback_control_button and add MainApp hold states

**Files:**
- Modify: `src/ui/mod.rs`

- [x] **Step 1: Add hold timer fields to MainApp**
  Add the following fields to `MainApp` struct:
  ```rust
      pub left_arrow_held_since: Option<std::time::Instant>,
      pub right_arrow_held_since: Option<std::time::Instant>,
  ```

- [x] **Step 2: Initialize fields in MainApp::new**
  Add initializations in `MainApp::new`:
  ```rust
              left_arrow_held_since: None,
              right_arrow_held_since: None,
  ```

- [x] **Step 3: Modify playback_control_button signature to return Response**
  Change signature and return value in `playback_control_button` (around lines 583 to 653):
  ```rust
  fn playback_control_button(
      ui: &mut egui::Ui,
      icon: PlaybackIcon,
      is_play_btn: bool,
      tooltip: &str,
  ) -> egui::Response {
      // ... (existing body) ...
      draw_playback_icon(painter, rect, &icon, icon_color);

      response
  }
  ```

- [x] **Step 4: Update all button call sites to use .clicked()**
  Modify all occurrences of `playback_control_button(...)` to `playback_control_button(...).clicked()`, except for the `PrevFrame` and `NextFrame` buttons which we will capture responses for:
  - Line 1432:
    ```rust
                                  if playback_control_button(
                                      ui,
                                      PlaybackIcon::ExportVideo,
                                      false,
                                      "export video",
                                  ).clicked() {
    ```
  - Line 1445:
    ```rust
                              if playback_control_button(
                                  ui,
                                  PlaybackIcon::LoadVideo,
                                  false,
                                  "Load video file",
                              ).clicked() {
    ```
  - Line 1792:
    ```rust
                                  if playback_control_button(ui, PlaybackIcon::Start, false, "Go to start of video").clicked() {
    ```
  - Line 1798:
    ```rust
                                  if playback_control_button(ui, PlaybackIcon::SetIn, false, "Set In marker at current frame [").clicked() {
    ```
  - Line 1805:
    ```rust
                                  if playback_control_button(ui, play_icon, true, play_tooltip).clicked() {
    ```
  - Line 1809:
    ```rust
                                  if playback_control_button(ui, PlaybackIcon::SetOut, false, "Set Out marker at current frame ]").clicked() {
    ```
  - Line 1815:
    ```rust
                                  if playback_control_button(ui, PlaybackIcon::End, false, "Go to end of video").clicked() {
    ```
  - Line 1826:
    ```rust
                                  if playback_control_button(ui, PlaybackIcon::AddSnippet, false, "Add current selection as a new snippet").clicked() {
    ```

- [x] **Step 5: Verify Compilation**
  Run: `cargo check`
  Expected: PASS (with minor warnings about unused buttons)

- [x] **Step 6: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "refactor: return egui::Response from playback_control_button and add MainApp hold states"
  ```

---

### Task 5: Implement hold-to-repeat and arrow keys step logic in UI

**Files:**
- Modify: `src/ui/mod.rs`

- [x] **Step 1: Capture PrevFrame and NextFrame button events**
  Around lines 1795 and 1812, replace button handles with capturing the responses and merging them with key events:
  ```rust
                                  let prev_resp = playback_control_button(ui, PlaybackIcon::PrevFrame, false, "Step backward one frame");
                                  if prev_resp.clicked() {
                                      prev_pressed = true;
                                  }
                                  if prev_resp.is_pointer_button_down_on() {
                                      prev_down = true;
                                  }
  ```
  and:
  ```rust
                                  let next_resp = playback_control_button(ui, PlaybackIcon::NextFrame, false, "Step forward one frame");
                                  if next_resp.clicked() {
                                      next_pressed = true;
                                  }
                                  if next_resp.is_pointer_button_down_on() {
                                      next_down = true;
                                  }
  ```

- [x] **Step 2: Initialize local input tracking variables at start of update**
  Near the top of `update` (around line 755):
  ```rust
          let mut prev_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft));
          let mut next_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));

          let mut prev_down = ctx.input(|i| i.key_down(egui::Key::ArrowLeft));
          let mut next_down = ctx.input(|i| i.key_down(egui::Key::ArrowRight));
  ```

- [x] **Step 3: Implement step hold-to-repeat logic at the end of update**
  At the end of `update` (around line 874), add the repeat logic:
  ```rust
          // Hold-to-repeat frame stepping
          if prev_pressed {
              self.current_frame = self.current_frame.saturating_sub(1);
              self.seek_to_frame(self.current_frame, false);
              self.left_arrow_held_since = Some(std::time::Instant::now());
              self.right_arrow_held_since = None;
          } else if next_pressed {
              self.current_frame = (self.current_frame + 1).min(self.total_frames);
              self.seek_to_frame(self.current_frame, false);
              self.right_arrow_held_since = Some(std::time::Instant::now());
              self.left_arrow_held_since = None;
          } else {
              if prev_down {
                  self.right_arrow_held_since = None;
                  if self.left_arrow_held_since.is_none() {
                      self.left_arrow_held_since = Some(std::time::Instant::now());
                  }
                  if let Some(held_since) = self.left_arrow_held_since {
                      if held_since.elapsed() >= std::time::Duration::from_millis(250) {
                          self.current_frame = self.current_frame.saturating_sub(1);
                          self.seek_to_frame(self.current_frame, false);
                          ctx.request_repaint();
                      }
                  }
              } else if next_down {
                  self.left_arrow_held_since = None;
                  if self.right_arrow_held_since.is_none() {
                      self.right_arrow_held_since = Some(std::time::Instant::now());
                  }
                  if let Some(held_since) = self.right_arrow_held_since {
                      if held_since.elapsed() >= std::time::Duration::from_millis(250) {
                          self.current_frame = (self.current_frame + 1).min(self.total_frames);
                          self.seek_to_frame(self.current_frame, false);
                          ctx.request_repaint();
                      }
                  }
              } else {
                  self.left_arrow_held_since = None;
                  self.right_arrow_held_since = None;
              }
          }
  ```

- [x] **Step 4: Verify Compilation**
  Run: `cargo check`
  Expected: PASS

- [x] **Step 5: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: implement hold-to-repeat frame stepping for arrow keys and UI buttons"
  ```

---

### Task 6: Run and Verify Integration Tests

**Files:**
- Test: `src/bin/test_playback_sync.rs`

- [x] **Step 1: Run integration test**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS (showing "All Tests Passed Successfully")
