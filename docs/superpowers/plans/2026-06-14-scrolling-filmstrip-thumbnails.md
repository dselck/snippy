# Dynamic Scrolling Filmstrip and Background Thumbnail Decoder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a dynamic scrolling filmstrip timeline under a stationary playhead, driven by an asynchronous, preemptible background thumbnail decoder thread and GPU texture caching.

**Architecture:** 
- A dedicated background thread (`ThumbnailDecoder`) opens a separate FFmpeg reader to seek and decode requested frame indices. It outputs scaled RGBA frame buffers over a channel.
- The UI thread polls the channel, uploads the frame buffers to GPU textures using `egui::Context::load_texture` (retaining `egui::TextureHandle`), and caches them in a `HashMap`.
- The filmstrip UI calculates the visible timeline step coordinates based on logarithmic zoom interpolation and the playhead offset, drawing only the visible cached thumbnails or placeholders.

**Tech Stack:** Rust, `egui`, `ffmpeg-next`, `crossbeam-channel`.

---

### Task 1: Create Communication Types and Register Module

**Files:**
- Create: `src/media/thumbnails.rs`
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Create `src/media/thumbnails.rs`**
  Define `ThumbnailCommand` and `ThumbnailResponse` enums/structs.
  ```rust
  use crossbeam_channel::{Receiver, Sender};
  use std::thread;

  #[derive(Debug, Clone)]
  pub enum ThumbnailCommand {
      Request { frames: Vec<usize> },
      Clear,
  }

  #[derive(Debug)]
  pub struct ThumbnailResponse {
      pub frame_idx: usize,
      pub width: u32,
      pub height: u32,
      pub rgba_data: Vec<u8>,
  }
  ```

- [ ] **Step 2: Add module registration to `src/media/mod.rs`**
  Add `pub mod thumbnails;` and `pub use thumbnails::*;` to `src/media/mod.rs` around lines 15-26.
  ```rust
  pub mod thumbnails;
  pub use thumbnails::*;
  ```

- [ ] **Step 3: Run compilation check**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add src/media/thumbnails.rs src/media/mod.rs
  git commit -m "feat(media): define thumbnail communication types and register module"
  ```

---

### Task 2: Implement Background Thumbnail Decoder Thread

**Files:**
- Modify: `src/media/thumbnails.rs`

- [ ] **Step 1: Write helper `get_nearest_keyframe` in `src/media/thumbnails.rs`**
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

- [ ] **Step 2: Implement `spawn_thumbnail_decoder` background loop**
  Create the core decoding loop in `src/media/thumbnails.rs` that reads commands and executes preemption-aware seeks and decodes.
  ```rust
  pub fn spawn_thumbnail_decoder(
      path: String,
      keyframes: Vec<usize>,
      keyframes_pts: Vec<i64>,
      fps: f64,
  ) -> (Sender<ThumbnailCommand>, Receiver<ThumbnailResponse>, thread::JoinHandle<()>) {
      let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
      let (resp_tx, resp_rx) = crossbeam_channel::bounded(100);

      let handle = thread::spawn(move || {
          let mut input = match ffmpeg_next::format::input(&path) {
              Ok(input) => input,
              Err(_) => return,
          };

          let stream = match input.streams().best(ffmpeg_next::media::Type::Video) {
              Some(s) => s,
              None => return,
          };
          let video_stream_index = stream.index();
          let time_base = stream.time_base();
          let time_base_f64 = time_base.numerator() as f64 / time_base.denominator() as f64;
          let start_time = if stream.start_time() > -1_000_000_000_000 {
              stream.start_time() as f64 * time_base_f64
          } else {
              0.0
          };

          let codec_ctx = match ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()) {
              Ok(ctx) => ctx,
              Err(_) => return,
          };
          let mut decoder = match codec_ctx.decoder().video() {
              Ok(dec) => dec,
              Err(_) => return,
          };

          let aspect_ratio = if decoder.width() > 0 && decoder.height() > 0 {
              decoder.width() as f32 / decoder.height() as f32
          } else {
              16.0 / 9.0
          };

          let thumb_height = 72;
          let thumb_width = (thumb_height as f32 * aspect_ratio).round() as u32;

          let mut scaler = match ffmpeg_next::software::scaling::context::Context::get(
              decoder.format(),
              decoder.width(),
              decoder.height(),
              ffmpeg_next::format::Pixel::RGBA,
              thumb_width,
              thumb_height,
              ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
          ) {
              Ok(s) => s,
              Err(_) => return,
          };

          while let Ok(cmd) = cmd_rx.recv() {
              match cmd {
                  ThumbnailCommand::Clear => {
                      // Drain pending commands
                      while cmd_rx.try_recv().is_ok() {}
                  }
                  ThumbnailCommand::Request { frames } => {
                      for target_frame in frames {
                          // Check if a Clear command has arrived
                          let mut aborted = false;
                          loop {
                              match cmd_rx.try_recv() {
                                  Ok(ThumbnailCommand::Clear) => {
                                      aborted = true;
                                      break;
                                  }
                                  Ok(ThumbnailCommand::Request { .. }) => {
                                      // Skip outdated requests
                                  }
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
                                  // Preemption check inside decoding loop
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
                                                  if scaler.run(&frame, &mut scaled_frame).is_ok() {
                                                      let _ = resp_tx.send(ThumbnailResponse {
                                                          frame_idx,
                                                          width: thumb_width,
                                                          height: thumb_height,
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
      });

      (cmd_tx, resp_rx, handle)
  }
  ```

- [ ] **Step 3: Write tests for `ThumbnailDecoder`**
  Add a unit test in `src/media/thumbnails.rs` to verify seeking and decoding using the sunflower test video.
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use std::time::Duration;

      #[test]
      fn test_thumbnail_decoder_lifecycle() {
          let test_file = format!("{}/test-files/bbb_sunflower_2160p_60fps_normal.mp4", env!("CARGO_MANIFEST_DIR"));
          let keyframes = vec![0, 60, 120];
          let keyframes_pts = vec![0, 60 * 1000, 120 * 1000]; // Dummy pts to check seek conversion
          
          let (cmd_tx, resp_rx, handle) = spawn_thumbnail_decoder(test_file, keyframes, keyframes_pts, 60.0);
          
          // Request frames 0 and 60
          cmd_tx.send(ThumbnailCommand::Request { frames: vec![0, 60] }).unwrap();
          
          // We expect to receive two thumbnail responses within 5 seconds
          let resp1 = resp_rx.recv_timeout(Duration::from_secs(5)).unwrap();
          assert_eq!(resp1.height, 72);
          assert!(resp1.rgba_data.len() > 0);
          
          let resp2 = resp_rx.recv_timeout(Duration::from_secs(5)).unwrap();
          assert_eq!(resp2.height, 72);
          
          // Cleanup
          drop(cmd_tx);
          let _ = handle.join();
      }
  }
  ```

- [ ] **Step 4: Run compilation and unit tests**
  Run: `cargo test media::thumbnails::tests`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add src/media/thumbnails.rs
  git commit -m "feat(media): implement spawn_thumbnail_decoder and verify with unit tests"
  ```

---

### Task 3: Expose Channels and Thread Lifecycle in `PlaybackSession`

**Files:**
- Modify: `src/media/session.rs`

- [ ] **Step 1: Add Thumbnail fields to `PlaybackSession`**
  Modify `PlaybackSession` struct in `src/media/session.rs` to hold the channel interfaces and the join handle.
  ```rust
  pub struct PlaybackSession {
      // ... existing fields ...
      pub(crate) thumbnail_tx: Option<Sender<crate::media::ThumbnailCommand>>,
      pub(crate) thumbnail_rx: Option<Receiver<crate::media::ThumbnailResponse>>,
      thumbnail_thread: Option<thread::JoinHandle<()>>,
  }
  ```

- [ ] **Step 2: Initialize fields in `PlaybackSession::new`**
  Set them to `None` in `PlaybackSession::new()` inside `src/media/session.rs`.
  ```rust
  thumbnail_tx: None,
  thumbnail_rx: None,
  thumbnail_thread: None,
  ```

- [ ] **Step 3: Modify `load_video` to spawn the thumbnail thread**
  Find the `load_video` implementation. In the block where `MediaEngine::new` is successfully instantiated, spawn the `ThumbnailDecoder` thread.
  ```rust
  // Inside load_video:
  let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
  if let Ok((engine, meta)) = MediaEngine::new(...) {
      // ... existing engine initialization ...
      
      // Spawn thumbnail decoder using metadata keyframes
      let (t_tx, t_rx, t_handle) = crate::media::spawn_thumbnail_decoder(
          path.to_string(),
          meta.keyframes.clone(),
          meta.keyframes_pts.clone(),
          meta.fps,
      );
      self.thumbnail_tx = Some(t_tx);
      self.thumbnail_rx = Some(t_rx);
      self.thumbnail_thread = Some(t_handle);
  }
  ```

- [ ] **Step 4: Shut down thumbnail thread on close / drop**
  Implement shutdown inside `PlaybackSession`'s teardown / close path:
  ```rust
  // If there's an existing shutdown / close method or Drop impl, join the thread:
  if let Some(tx) = self.thumbnail_tx.take() {
      drop(tx); // Close command channel to exit loop
  }
  if let Some(handle) = self.thumbnail_thread.take() {
      let _ = handle.join();
  }
  ```

- [ ] **Step 5: Run tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 6: Commit**
  ```bash
  git add src/media/session.rs
  git commit -m "feat(media): integrate thumbnail decoder thread lifecycle into PlaybackSession"
  ```

---

### Task 4: Implement Timeline Spacing, Zoom, and Drag Math in UI

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add new state fields to `MainApp`**
  Modify `MainApp` in `src/ui/mod.rs` to add texture handles, visible list tracker, and scroll dragging state.
  ```rust
  pub struct MainApp {
      // ... existing fields ...
      thumbnail_cache: std::collections::HashMap<usize, egui::TextureHandle>,
      in_flight_requests: std::collections::HashSet<usize>,
      is_dragging_filmstrip: bool,
  }
  ```
  Ensure they are initialized to empty/false in `MainApp::new()`.

- [ ] **Step 2: Drain response channel and upload textures**
  In the `App::ui` update function (around lines 658-662), retrieve any decoded RGBA buffers and load them as textures.
  ```rust
  // Polling decoded thumbnails
  if let Some(rx) = self.playback.thumbnail_rx.as_ref() {
      while let Ok(resp) = rx.try_recv() {
          let color_image = egui::ColorImage::from_rgba_unmultiplied(
              [resp.width as usize, resp.height as usize],
              &resp.rgba_data,
          );
          let texture_handle = ctx.load_texture(
              format!("thumb_{}", resp.frame_idx),
              color_image,
              egui::TextureOptions::BILINEAR,
          );
          self.thumbnail_cache.insert(resp.frame_idx, texture_handle);
          self.in_flight_requests.remove(&resp.frame_idx);
      }
  }
  ```

- [ ] **Step 3: Update Sprocket Filmstrip Viewport drawing & logic**
  Replace lines ~780-875 in `src/ui/mod.rs` with the custom scrolling/zooming timeline and request logic.
  ```rust
  // Calculate logarithmic delta step
  let fps = self.playback.video_fps();
  let total_frames = self.playback.total_frames();
  let duration_sec = total_frames as f64 / fps;
  
  // Interpolate time delta represented by a single thumbnail step
  // Zoom level 0.0 -> 30s. Zoom level 1.0 -> 1/fps s (1 frame)
  let min_interval = 1.0 / fps;
  let max_interval = 30.0;
  let interval = max_interval * (min_interval / max_interval).powf(self.zoom_level as f64);
  
  // Step spacing logic
  let spacing_x = 6.0;
  let thumb_height = 72.0;
  let aspect_ratio = if inner_height > 0.0 && thumbnail_width > 0.0 {
      thumbnail_width / inner_height
  } else {
      16.0 / 9.0
  };
  let actual_thumb_width = thumb_height * aspect_ratio;
  let step_width = actual_thumb_width + spacing_x;
  let pixels_per_second = step_width as f64 / interval;

  // Stationary Playhead Center position
  let playhead_x = total_width * 0.5;
  let current_time_sec = self.playback.current_frame() as f64 / fps;
  let scroll_offset_x = playhead_x as f64 - (current_time_sec * pixels_per_second);

  // Compute indices of visible step slots
  let left_bound = -scroll_offset_x - actual_thumb_width as f64;
  let right_bound = -scroll_offset_x + total_width as f64;
  let first_k = (left_bound / step_width as f64).floor() as isize;
  let last_k = (right_bound / step_width as f64).ceil() as isize;
  
  let mut requested_frames = Vec::new();

  for k in first_k..=last_k {
      if k < 0 { continue; }
      let target_time = k as f64 * interval;
      if target_time > duration_sec { break; }
      let target_frame = (target_time * fps).round() as usize;
      if target_frame >= total_frames { continue; }

      let tx = scroll_offset_x + (k as f64 * step_width as f64);
      let thub_rect = egui::Rect::from_min_size(
          egui::pos2(tx as f32, inner_rect.top()),
          egui::vec2(actual_thumb_width, thumb_height),
      );

      // Handle rendering
      if let Some(texture) = self.thumbnail_cache.get(&target_frame) {
          // Draw decoded thumbnail image
          painter.image(
              texture.id(),
              thub_rect,
              egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
              Color32::WHITE,
          );
      } else {
          // Placeholder loading card
          painter.rect_filled(thub_rect, 4.0, Color32::from_rgb(18, 17, 28));
          painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(45, 43, 75)), egui::StrokeKind::Inside);
          if inner_height > 12.0 {
              painter.text(
                  thub_rect.center(),
                  egui::Align2::CENTER_CENTER,
                  "LOADING",
                  egui::FontId::proportional(9.0),
                  Color32::from_rgb(45, 43, 75),
              );
          }
          
          // Request frame if not in flight
          if !self.in_flight_requests.contains(&target_frame) {
              requested_frames.push(target_frame);
              self.in_flight_requests.insert(target_frame);
          }
      }
      
      // Underlay timecode
      if inner_height > 12.0 {
          let time_text = self.frame_to_time(target_frame);
          painter.text(
              egui::pos2(thub_rect.center().x, thub_rect.bottom() + 10.0),
              egui::Align2::CENTER_CENTER,
              time_text,
              egui::FontId::monospace(8.0),
              Color32::from_rgb(148, 163, 184),
          );
      }
  }

  // Send request batch if present
  if !requested_frames.is_empty() {
      if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
          let _ = tx.send(crate::media::ThumbnailCommand::Request { frames: requested_frames });
      }
  }
  ```

- [ ] **Step 4: Abort on Zoom change**
  Add a zoom monitor to send a `Clear` command when zoom level shifts.
  ```rust
  // Inside App::ui, detect zoom change:
  let has_zoomed = (self.zoom_level - self.last_zoom_level).abs() > 0.01;
  if has_zoomed {
      self.last_zoom_level = self.zoom_level;
      self.in_flight_requests.clear();
      if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
          let _ = tx.send(crate::media::ThumbnailCommand::Clear);
      }
  }
  ```

- [ ] **Step 5: Drag-to-Scrub Mouse Interaction**
  Add mouse drag handling on the filmstrip viewport.
  ```rust
  let response_id = ui.make_persistent_id("filmstrip_viewport_interact");
  let filmstrip_response = ui.interact(filmstrip_rect, response_id, egui::Sense::click_and_drag());
  
  if filmstrip_response.dragged() || filmstrip_response.clicked() {
      self.playback.set_playing(false);
      if let Some(pointer_pos) = filmstrip_response.interact_pointer_pos() {
          let dx_pixels = pointer_pos.x - playhead_x as f32;
          let dt_seconds = dx_pixels as f64 / pixels_per_second;
          let target_time = current_time_sec + dt_seconds;
          let target_frame = (target_time * fps).round().max(0.0).min(total_frames as f64 - 1.0) as usize;
          self.playback.seek_internal(target_frame, false, true);
      }
  }
  ```

- [ ] **Step 6: Run compilation check**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 7: Run full test suite**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 8: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat(ui): implement timeline spacing, scrolling filmstrip rendering, zoom tracking, and drag scrubbing"
  ```
