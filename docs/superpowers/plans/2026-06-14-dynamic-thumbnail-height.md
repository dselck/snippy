# Dynamic Thumbnail Height Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make thumbnail decoding height dynamic based on the UI filmstrip panel's actual rendered height.

**Architecture:**
- Update `ThumbnailCommand::Request` to accept `height: u32`.
- The background thread will check if the requested height has changed, and if so, dynamically recreate the `swscale` context preserving the aspect ratio.
- The UI thread will track the rendered `inner_height`, clear its texture cache on size change, and pass the new height in its requests.

**Tech Stack:** Rust, `egui`, `ffmpeg-next`.

---

### Task 1: Update Channels and Background Thread Scaler Logic

**Files:**
- Modify: `src/media/thumbnails.rs`

- [ ] **Step 1: Modify `ThumbnailCommand` definition**
  Update the command enum to take a `height` parameter:
  ```rust
  #[derive(Debug, Clone)]
  pub enum ThumbnailCommand {
      Request { frames: Vec<usize>, height: u32 },
      Clear,
  }
  ```

- [ ] **Step 2: Update `spawn_thumbnail_decoder` to dynamically recreate scaler**
  Update the scaler logic inside `spawn_thumbnail_decoder` to recreate the scaler context whenever the requested `height` changes.
  
  Replace lines ~65-79 (scaler initialization) and lines ~81-180 (command processing) to:
  - Keep track of `current_height: Option<u32>` and a mutable `scaler: Option<ffmpeg_next::software::scaling::context::Context>`.
  - When processing a request with a new `height`, check if `Some(height) != current_height`.
  - If it is different, compute the new `width` preserving aspect ratio, recreate the scaler, and update `current_height`.
  
  Specifically, inside the `spawn_thumbnail_decoder` loop:
  ```rust
  // ... decoder initialization ...
  let aspect_ratio = if decoder.width() > 0 && decoder.height() > 0 {
      decoder.width() as f32 / decoder.height() as f32
  } else {
      16.0 / 9.0
  };

  let mut current_height: Option<u32> = None;
  let mut scaler: Option<ffmpeg_next::software::scaling::context::Context> = None;

  while let Ok(cmd) = cmd_rx.recv() {
      match cmd {
          ThumbnailCommand::Clear => {
              while cmd_rx.try_recv().is_ok() {}
          }
          ThumbnailCommand::Request { frames, height } => {
              // Recreate scaler if requested height changes
              if current_height != Some(height) {
                  let thumb_width = (height as f32 * aspect_ratio).round() as u32;
                  if let Ok(s) = ffmpeg_next::software::scaling::context::Context::get(
                      decoder.format(),
                      decoder.width(),
                      decoder.height(),
                      ffmpeg_next::format::Pixel::RGBA,
                      thumb_width,
                      height,
                      ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
                  ) {
                      scaler = Some(s);
                      current_height = Some(height);
                  }
              }

              let current_scaler = match scaler.as_mut() {
                  Some(s) => s,
                  None => continue,
              };
              let active_height = current_height.unwrap_or(72);
              let active_width = (active_height as f32 * aspect_ratio).round() as u32;

              for target_frame in frames {
                  // Preemption check
                  let mut aborted = false;
                  loop {
                      match cmd_rx.try_recv() {
                          Ok(ThumbnailCommand::Clear) => {
                              aborted = true;
                              break;
                          }
                          Ok(ThumbnailCommand::Request { .. }) => {}
                          Err(_) => break,
                      }
                  }
                  if aborted {
                      break;
                  }

                  let target_kf = get_nearest_keyframe(&keyframes, target_frame);
                  let kf_idx = keyframes.binary_search(&target_kf).unwrap_or(0);
                  let pts = if kf_idx < keyframes_pts.len() {
                      keyframes_pts[kf_idx]
                  } else {
                      0
                  };
                  let pts_us = (pts as f64 * time_base_f64 * 1_000_000.0).round() as i64;

                  if input.seek(pts_us, ..pts_us).is_ok() {
                      decoder.flush();
                      let mut fallback_frame_idx = target_kf;
                      let mut packets_iter = input.packets();

                      while let Some(packet_res) = packets_iter.next() {
                          if cmd_rx.try_recv().is_ok() {
                              aborted = true;
                              break;
                          }

                          let (stream_ref, packet) = match packet_res {
                              Ok(p) => p,
                              Err(_) => break,
                          };

                          if stream_ref.index() == video_stream_index {
                              if decoder.send_packet(&packet).is_ok() {
                                  let mut frame = ffmpeg_next::util::frame::Video::empty();
                                  let mut frame_found = false;

                                  while decoder.receive_frame(&mut frame).is_ok() {
                                      let pts_sec = if let Some(timestamp) = frame.timestamp() {
                                          timestamp as f64 * time_base_f64 - start_time
                                      } else if let Some(pts) = frame.pts() {
                                          pts as f64 * time_base_f64 - start_time
                                      } else {
                                          let pkt_dts = unsafe { (*frame.as_ptr()).pkt_dts };
                                          if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                                              pkt_dts as f64 * time_base_f64 - start_time
                                          } else {
                                              fallback_frame_idx as f64 / fps
                                          }
                                      };
                                      let frame_idx = (pts_sec * fps).round() as usize;
                                      fallback_frame_idx = frame_idx + 1;

                                      if frame_idx >= target_frame {
                                          let mut scaled_frame = ffmpeg_next::util::frame::Video::empty();
                                          if current_scaler.run(&frame, &mut scaled_frame).is_ok() {
                                              let _ = resp_tx.send(ThumbnailResponse {
                                                  frame_idx,
                                                  width: active_width,
                                                  height: active_height,
                                                  rgba_data: scaled_frame.data(0).to_vec(),
                                              });
                                              frame_found = true;
                                          }
                                          break;
                                      }
                                  }
                                  if frame_found {
                                      break;
                                  }
                              }
                          }
                      }
                  }
                  if aborted {
                      break;
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 3: Update Unit Tests**
  Update the unit test in `src/media/thumbnails.rs` to pass a height parameter (e.g., `72`).
  ```rust
  cmd_tx.send(ThumbnailCommand::Request { frames: vec![0, 60], height: 72 }).unwrap();
  ```

- [ ] **Step 4: Run test to verify passes**
  Run: `cargo test media::thumbnails::tests`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add src/media/thumbnails.rs
  git commit -m "feat(media): implement dynamic scaler reconfiguration in background thread"
  ```

---

### Task 2: Integrate Dynamic Height and Cache Invalidation in UI

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Track current viewer height**
  Add a `last_thumbnail_height` field to `MainApp` in `src/ui/mod.rs`.
  ```rust
  pub struct MainApp {
      // ... existing fields ...
      last_thumbnail_height: f32,
  }
  ```
  Initialize it to `0.0` in `MainApp::new()`.

- [ ] **Step 2: Invalidate cache on significant height changes**
  In `App::ui` (inside `src/ui/mod.rs`), check if `inner_height` has changed significantly (e.g., by more than `4.0` pixels) from `last_thumbnail_height`.
  If it has:
  - Clear `thumbnail_cache`.
  - Clear `in_flight_requests`.
  - Send `ThumbnailCommand::Clear` to background decoder.
  - Update `last_thumbnail_height` to `inner_height`.

  ```rust
  // Inside App::ui, around where zoom changes are checked:
  let height_diff = (inner_height - self.last_thumbnail_height).abs();
  if height_diff > 4.0 {
      self.last_thumbnail_height = inner_height;
      self.thumbnail_cache.clear();
      self.in_flight_requests.clear();
      if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
          let _ = tx.send(crate::media::ThumbnailCommand::Clear);
      }
  }
  ```

- [ ] **Step 3: Update `ThumbnailCommand::Request` dispatch**
  Pass the clamped height (e.g., `inner_height.round() as u32` clamped between `30` and `256`) in the request dispatch in `src/ui/mod.rs`.
  ```rust
  let clamped_height = (inner_height.round() as u32).clamp(30, 256);
  if !requested_frames.is_empty() {
      if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
          let _ = tx.send(crate::media::ThumbnailCommand::Request { 
              frames: requested_frames, 
              height: clamped_height 
          });
      }
  }
  ```

- [ ] **Step 4: Verify Compilation & Full Tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): invalidate cache and request thumbnails dynamically on filmstrip height changes"
  ```
