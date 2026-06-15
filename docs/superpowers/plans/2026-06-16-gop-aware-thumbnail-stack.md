# GOP-Aware Thumbnail Stack — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-threaded, GOP-blind thumbnail decoder with a coordinator + N-worker pool that groups requested frames by GOP, decodes each GOP in one forward pass, and serves a bounded LRU cache — eliminating redundant seeks and memory leaks.

**Architecture:** A coordinator thread receives `ThumbnailCommand::Request { visible_frames, padding_frames, height }`, groups both frame lists by nearest keyframe into GOP Work Items, and pushes them (visible GOPs first) onto a crossbeam `SegQueue`. N independent worker threads each own a full FFmpeg context; they pull work items, seek once per GOP, decode forward collecting all target frames, and emit a single `ThumbnailResponse { served_frames }` per GOP. The UI holds a single `LruCache<usize, TextureHandle>` (cap 300); on receiving a response it loads one GPU texture and cheaply clones the `Arc`-backed handle for each served frame. A `thumbnails_dirty` flag collapses rapid viewport changes into one request per repaint cycle.

**Tech Stack:** Rust, `ffmpeg-next`, `crossbeam-channel` + `crossbeam-queue` (adding), `lru = "0.12"` (new dep), `egui::TextureHandle`.

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `Cargo.toml` | Modify | Add `lru`, `crossbeam-queue`, `crossbeam-utils` |
| `src/media/thumbnails.rs` | Rewrite | Coordinator + worker pool, new protocol types |
| `src/media/session.rs` | Modify | Store `Vec<JoinHandle>` instead of one; call new spawn fn |
| `src/ui/mod.rs` | Modify | LRU cache, `thumbnails_dirty` flag, split visible/padding frames |
| `CONTEXT.md` | Modify | Add three new domain terms |

---

## Task 1: Add dependencies and update protocol types

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/media/thumbnails.rs` (types only)

- [ ] **Step 1: Add dependencies to `Cargo.toml`**

  After the `crossbeam-channel` line, add:
  ```toml
  crossbeam-queue = "0.3"
  crossbeam-utils = "0.8"
  lru = "0.12"
  ```

- [ ] **Step 2: Replace the two public types at the top of `src/media/thumbnails.rs`**

  Replace lines 1–16 (the `use` statements through the closing `}` of `ThumbnailResponse`) with:

  ```rust
  use crossbeam_channel::{Receiver, Sender};
  use std::sync::atomic::{AtomicU64, Ordering};
  use std::sync::Arc;
  use std::thread;

  #[derive(Debug, Clone)]
  pub enum ThumbnailCommand {
      /// visible_frames: currently on-screen slots (decoded first).
      /// padding_frames: ±2 preload zone (decoded after visible GOPs).
      Request {
          visible_frames: Vec<usize>,
          padding_frames: Vec<usize>,
          height: u32,
      },
      Clear,
  }

  #[derive(Debug)]
  pub struct ThumbnailResponse {
      /// Every target_frame satisfied by this single GOP decode.
      pub served_frames: Vec<usize>,
      pub width: u32,
      pub height: u32,
      pub rgba_data: Vec<u8>,
  }
  ```

- [ ] **Step 3: Verify it type-checks**

  ```bash
  cargo check 2>&1 | grep "thumbnails.rs" | head -20
  ```

  Expected: errors about `frame_idx`/`frames` fields at call sites — those are fixed in Tasks 3–5. Zero errors inside `thumbnails.rs` itself.

- [ ] **Step 4: Commit**

  ```bash
  git add Cargo.toml src/media/thumbnails.rs
  git commit -m "feat: update ThumbnailCommand/Response protocol for GOP pool"
  ```

---

## Task 2: Add `GopWorkItem` struct and `group_frames_by_gop` helper

**Files:**
- Modify: `src/media/thumbnails.rs`

- [ ] **Step 1: Replace `get_nearest_keyframe` and add the new types after it**

  Keep `get_nearest_keyframe` unchanged. Directly after the closing `}` of that function, add:

  ```rust
  /// One GOP's worth of decode work dispatched to a worker thread.
  #[derive(Debug)]
  struct GopWorkItem {
      keyframe_pts: i64,
      target_frames: Vec<usize>, // sorted ascending
      generation: u64,
      height: u32,
  }

  /// Groups `frames` into one `GopWorkItem` per GOP, ordered chronologically.
  /// Multiple frames that share the same nearest keyframe produce a single item.
  fn group_frames_by_gop(
      frames: &[usize],
      keyframes: &[usize],
      keyframes_pts: &[i64],
      generation: u64,
      height: u32,
  ) -> Vec<GopWorkItem> {
      use std::collections::BTreeMap;
      // BTreeMap keyed by keyframe frame-index keeps GOPs in chronological order.
      let mut gops: BTreeMap<usize, (i64, Vec<usize>)> = BTreeMap::new();
      for &target in frames {
          let kf = get_nearest_keyframe(keyframes, target);
          let kf_idx = keyframes.binary_search(&kf).unwrap_or(0);
          let pts = if kf_idx < keyframes_pts.len() { keyframes_pts[kf_idx] } else { 0 };
          gops.entry(kf).or_insert_with(|| (pts, Vec::new())).1.push(target);
      }
      gops.into_values()
          .map(|(keyframe_pts, mut target_frames)| {
              target_frames.sort_unstable();
              GopWorkItem { keyframe_pts, target_frames, generation, height }
          })
          .collect()
  }
  ```

- [ ] **Step 2: Verify helper compiles**

  ```bash
  cargo check 2>&1 | grep "thumbnails.rs" | head -20
  ```

  Expected: same caller errors as Task 1; no new errors inside `thumbnails.rs`.

- [ ] **Step 3: Commit**

  ```bash
  git add src/media/thumbnails.rs
  git commit -m "feat: add GopWorkItem and group_frames_by_gop helper"
  ```

---

## Task 3: Implement `spawn_thumbnail_decoder_pool` (coordinator + workers)

**Files:**
- Modify: `src/media/thumbnails.rs` — remove old `spawn_thumbnail_decoder`, add pool

- [ ] **Step 1: Delete `spawn_thumbnail_decoder` and replace with the pool implementation**

  Remove the entire `spawn_thumbnail_decoder` function (everything from `pub fn spawn_thumbnail_decoder` through its closing `}`). In its place insert the following three functions:

  ```rust
  // TODO(config): expose THUMBNAIL_DECODER_THREADS as a user-configurable setting.
  const THUMBNAIL_DECODER_THREADS: usize = 4;

  /// Spawns a coordinator thread plus N worker threads.
  /// Returns the command sender, response receiver, and all thread handles.
  pub fn spawn_thumbnail_decoder_pool(
      path: String,
      keyframes: Vec<usize>,
      keyframes_pts: Vec<i64>,
      fps: f64,
  ) -> (Sender<ThumbnailCommand>, Receiver<ThumbnailResponse>, Vec<thread::JoinHandle<()>>) {
      let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<ThumbnailCommand>();
      let (resp_tx, resp_rx) = crossbeam_channel::bounded::<ThumbnailResponse>(200);

      let generation: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
      let work_queue: Arc<crossbeam_queue::SegQueue<GopWorkItem>> =
          Arc::new(crossbeam_queue::SegQueue::new());

      let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity(THUMBNAIL_DECODER_THREADS + 1);

      // Spawn worker threads first.
      for _ in 0..THUMBNAIL_DECODER_THREADS {
          let work_queue = Arc::clone(&work_queue);
          let generation = Arc::clone(&generation);
          let resp_tx = resp_tx.clone();
          let path = path.clone();
          handles.push(thread::spawn(move || {
              worker_thread(path, fps, work_queue, generation, resp_tx);
          }));
      }

      // Spawn coordinator last.
      {
          let work_queue = Arc::clone(&work_queue);
          let generation = Arc::clone(&generation);
          let keyframes = keyframes.clone();
          let keyframes_pts = keyframes_pts.clone();
          handles.push(thread::spawn(move || {
              coordinator_thread(cmd_rx, work_queue, generation, keyframes, keyframes_pts);
          }));
      }

      (cmd_tx, resp_rx, handles)
  }

  fn coordinator_thread(
      cmd_rx: Receiver<ThumbnailCommand>,
      work_queue: Arc<crossbeam_queue::SegQueue<GopWorkItem>>,
      generation: Arc<AtomicU64>,
      keyframes: Vec<usize>,
      keyframes_pts: Vec<i64>,
  ) {
      loop {
          let cmd = match cmd_rx.recv() {
              Ok(c) => c,
              Err(_) => break,
          };
          match cmd {
              ThumbnailCommand::Clear => {
                  generation.fetch_add(1, Ordering::SeqCst);
                  while work_queue.pop().is_some() {}
                  while cmd_rx.try_recv().is_ok() {}
              }
              ThumbnailCommand::Request { visible_frames, padding_frames, height } => {
                  let gen = generation.fetch_add(1, Ordering::SeqCst) + 1;
                  while work_queue.pop().is_some() {}
                  // Visible GOPs pushed first → pulled first by workers (FIFO).
                  for item in group_frames_by_gop(&visible_frames, &keyframes, &keyframes_pts, gen, height) {
                      work_queue.push(item);
                  }
                  for item in group_frames_by_gop(&padding_frames, &keyframes, &keyframes_pts, gen, height) {
                      work_queue.push(item);
                  }
              }
          }
      }
  }

  fn worker_thread(
      path: String,
      fps: f64,
      work_queue: Arc<crossbeam_queue::SegQueue<GopWorkItem>>,
      generation: Arc<AtomicU64>,
      resp_tx: Sender<ThumbnailResponse>,
  ) {
      let mut input = match ffmpeg_next::format::input(&path) {
          Ok(i) => i,
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

      // Peek aspect ratio before codec_ctx is consumed by decoder().video().
      let aspect_ratio = {
          let ctx = match ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()) {
              Ok(c) => c,
              Err(_) => return,
          };
          match ctx.decoder().video() {
              Ok(d) if d.width() > 0 && d.height() > 0 => d.width() as f32 / d.height() as f32,
              _ => 16.0 / 9.0,
          }
      };

      // Build the real codec context with HW acceleration.
      let mut codec_ctx = match ffmpeg_next::codec::context::Context::from_parameters(stream.parameters()) {
          Ok(c) => c,
          Err(_) => return,
      };
      unsafe {
          let mut hw_device_ctx: *mut ffmpeg_next::sys::AVBufferRef = std::ptr::null_mut();
          #[cfg(target_os = "macos")]
          let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX;
          #[cfg(target_os = "windows")]
          let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D11VA;
          #[cfg(target_os = "linux")]
          let hw_type = ffmpeg_next::sys::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI;
          let res = ffmpeg_next::sys::av_hwdevice_ctx_create(
              &mut hw_device_ctx, hw_type, std::ptr::null(), std::ptr::null_mut(), 0,
          );
          if res == 0 && !hw_device_ctx.is_null() {
              (*codec_ctx.as_mut_ptr()).hw_device_ctx =
                  ffmpeg_next::sys::av_buffer_ref(hw_device_ctx);
              ffmpeg_next::sys::av_buffer_unref(&mut hw_device_ctx);
          }
      }
      let mut decoder = match codec_ctx.decoder().video() {
          Ok(d) => d,
          Err(_) => return,
      };
      let mut scaler: Option<ffmpeg_next::software::scaling::context::Context> = None;

      loop {
          // Spin-wait for a work item.
          let item = loop {
              if let Some(i) = work_queue.pop() { break i; }
              if resp_tx.is_disconnected() { return; }
              std::thread::sleep(std::time::Duration::from_millis(2));
          };

          if generation.load(Ordering::SeqCst) != item.generation { continue; }

          let pts_us = (item.keyframe_pts as f64 * time_base_f64 * 1_000_000.0).round() as i64;
          if input.seek(pts_us, ..pts_us).is_err() { continue; }
          decoder.flush();

          let height = item.height;
          let thumb_width = (height as f32 * aspect_ratio).round() as u32;
          let mut remaining = item.target_frames.clone(); // already sorted ascending
          let mut served_frames: Vec<usize> = Vec::new();
          let mut collected_rgba: Option<(Vec<u8>, u32, u32)> = None;
          let mut fallback_frame_idx = 0usize;

          'packets: for (stream_ref, packet) in input.packets() {
              if generation.load(Ordering::SeqCst) != item.generation { break 'packets; }
              if stream_ref.index() != video_stream_index { continue; }
              if decoder.send_packet(&packet).is_err() { continue; }

              let mut raw_frame = ffmpeg_next::util::frame::Video::empty();
              while decoder.receive_frame(&mut raw_frame).is_ok() {
                  let pts_sec = if let Some(ts) = raw_frame.timestamp() {
                      ts as f64 * time_base_f64 - start_time
                  } else if let Some(pts) = raw_frame.pts() {
                      pts as f64 * time_base_f64 - start_time
                  } else {
                      let pkt_dts = unsafe { (*raw_frame.as_ptr()).pkt_dts };
                      if pkt_dts != ffmpeg_next::sys::AV_NOPTS_VALUE {
                          pkt_dts as f64 * time_base_f64 - start_time
                      } else {
                          fallback_frame_idx as f64 / fps
                      }
                  };
                  let frame_idx = (pts_sec * fps).round() as usize;
                  fallback_frame_idx = frame_idx + 1;

                  // Collect all targets we have now passed.
                  let mut i = 0;
                  while i < remaining.len() {
                      if remaining[i] <= frame_idx {
                          let target = remaining.remove(i);
                          // Scale once; all GOP-group members share the same RGBA.
                          if collected_rgba.is_none() {
                              let actual = if raw_frame.format() == ffmpeg_next::format::Pixel::VIDEOTOOLBOX
                                  || raw_frame.format() == ffmpeg_next::format::Pixel::D3D11
                              {
                                  let mut sw = ffmpeg_next::util::frame::Video::empty();
                                  let ok = unsafe {
                                      ffmpeg_next::sys::av_hwframe_transfer_data(
                                          sw.as_mut_ptr(), raw_frame.as_ptr(), 0,
                                      )
                                  };
                                  if ok < 0 { raw_frame.clone() } else { sw }
                              } else {
                                  raw_frame.clone()
                              };

                              let sc = match scaler.as_mut() {
                                  Some(s) if s.input().format == actual.format()
                                      && s.input().width == actual.width()
                                      && s.input().height == actual.height()
                                      && s.output().height == height => s,
                                  _ => {
                                      match ffmpeg_next::software::scaling::context::Context::get(
                                          actual.format(), actual.width(), actual.height(),
                                          ffmpeg_next::format::Pixel::RGBA,
                                          thumb_width, height,
                                          ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
                                      ) {
                                          Ok(s) => { scaler = Some(s); scaler.as_mut().unwrap() }
                                          Err(_) => break,
                                      }
                                  }
                              };

                              let mut scaled = ffmpeg_next::util::frame::Video::empty();
                              if sc.run(&actual, &mut scaled).is_ok() {
                                  let w = scaled.width() as usize;
                                  let h = scaled.height() as usize;
                                  let stride = scaled.stride(0) as usize;
                                  let data = scaled.data(0);
                                  let mut rgba = Vec::with_capacity(w * h * 4);
                                  for y in 0..h {
                                      let s = y * stride;
                                      if let Some(row) = data.get(s..s + w * 4) {
                                          rgba.extend_from_slice(row);
                                      }
                                  }
                                  collected_rgba = Some((rgba, w as u32, h as u32));
                              }
                          }
                          served_frames.push(target);
                      } else {
                          i += 1;
                      }
                  }
                  if remaining.is_empty() { break 'packets; }
              }
          }

          // Emit only if not preempted and we have data.
          if generation.load(Ordering::SeqCst) == item.generation {
              if let Some((rgba_data, w, h)) = collected_rgba {
                  if !served_frames.is_empty() {
                      let _ = resp_tx.send(ThumbnailResponse {
                          served_frames,
                          width: w,
                          height: h,
                          rgba_data,
                      });
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 2: Verify `thumbnails.rs` is error-free**

  ```bash
  cargo check 2>&1 | grep "thumbnails.rs" | head -20
  ```

  Expected: zero errors inside `thumbnails.rs`. Errors about `spawn_thumbnail_decoder` at call sites in `session.rs` are expected.

- [ ] **Step 3: Commit**

  ```bash
  git add Cargo.toml src/media/thumbnails.rs
  git commit -m "feat: implement coordinator + worker pool in thumbnails.rs"
  ```

---

## Task 4: Update `PlaybackSession` to use the pool

**Files:**
- Modify: `src/media/session.rs`

- [ ] **Step 1: Change `thumbnail_thread` field type in the `PlaybackSession` struct**

  Find:
  ```rust
  thumbnail_thread: Option<thread::JoinHandle<()>>,
  ```
  Replace with:
  ```rust
  thumbnail_threads: Vec<thread::JoinHandle<()>>,
  ```

- [ ] **Step 2: Update `PlaybackSession::new` initialiser**

  Find:
  ```rust
  thumbnail_thread: None,
  ```
  Replace with:
  ```rust
  thumbnail_threads: Vec::new(),
  ```

- [ ] **Step 3: Update `shutdown_thumbnail_thread`**

  Replace the entire function body:
  ```rust
  pub fn shutdown_thumbnail_thread(&mut self) {
      drop(self.thumbnail_tx.take());
      self.thumbnail_rx = None;
      for handle in self.thumbnail_threads.drain(..) {
          let _ = handle.join();
      }
  }
  ```

- [ ] **Step 4: Update both `spawn_thumbnail_decoder` call sites (search for them)**

  There are two occurrences (~lines 164 and ~299). Replace each:
  ```rust
  let (t_tx, t_rx, t_handle) = crate::media::spawn_thumbnail_decoder(
      path.to_string(),
      meta.keyframes.clone(),
      meta.keyframes_pts.clone(),
      meta.fps,
  );
  self.thumbnail_tx = Some(t_tx);
  self.thumbnail_rx = Some(t_rx);
  self.thumbnail_thread = Some(t_handle);
  ```
  With:
  ```rust
  let (t_tx, t_rx, t_handles) = crate::media::spawn_thumbnail_decoder_pool(
      path.to_string(),
      meta.keyframes.clone(),
      meta.keyframes_pts.clone(),
      meta.fps,
  );
  self.thumbnail_tx = Some(t_tx);
  self.thumbnail_rx = Some(t_rx);
  self.thumbnail_threads = t_handles;
  ```

- [ ] **Step 5: Verify session compiles**

  ```bash
  cargo check 2>&1 | grep "session.rs" | head -20
  ```

  Expected: zero errors in `session.rs`. `ui/mod.rs` errors expected (next task).

- [ ] **Step 6: Commit**

  ```bash
  git add src/media/session.rs
  git commit -m "feat: update PlaybackSession for thumbnail decoder pool"
  ```

---

## Task 5: Update `src/ui/mod.rs` — LRU cache, dirty flag, split request tiers

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add imports at the top of `src/ui/mod.rs`**

  Add after the existing `use` block:
  ```rust
  use lru::LruCache;
  use std::num::NonZeroUsize;
  ```

- [ ] **Step 2: Update `thumbnail_cache` field type and add `thumbnails_dirty`**

  In the `MainApp` struct, change:
  ```rust
  pub thumbnail_cache: std::collections::HashMap<usize, egui::TextureHandle>,
  ```
  To:
  ```rust
  pub thumbnail_cache: LruCache<usize, egui::TextureHandle>,
  pub thumbnails_dirty: bool,
  ```

- [ ] **Step 3: Update `MainApp::new` initialiser**

  Change:
  ```rust
  thumbnail_cache: std::collections::HashMap::new(),
  ```
  To:
  ```rust
  thumbnail_cache: LruCache::new(NonZeroUsize::new(300).unwrap()),
  thumbnails_dirty: true,
  ```

- [ ] **Step 4: Update the thumbnail response polling block**

  Find the block starting with `// Polling decoded thumbnails` and replace:
  ```rust
  if let Some(rx) = self.playback.thumbnail_rx.as_ref() {
      while let Ok(resp) = rx.try_recv() {
          let color_image = egui::ColorImage::from_rgba_unmultiplied(
              [resp.width as usize, resp.height as usize],
              &resp.rgba_data,
          );
          let texture_handle = ctx.load_texture(
              format!("thumb_{}", resp.frame_idx),
              color_image,
              egui::TextureOptions::LINEAR,
          );
          self.thumbnail_cache.insert(resp.frame_idx, texture_handle);
          self.in_flight_requests.remove(&resp.frame_idx);
      }
  }
  ```
  With:
  ```rust
  if let Some(rx) = self.playback.thumbnail_rx.as_ref() {
      while let Ok(resp) = rx.try_recv() {
          let color_image = egui::ColorImage::from_rgba_unmultiplied(
              [resp.width as usize, resp.height as usize],
              &resp.rgba_data,
          );
          // Load once — TextureHandle is Arc-backed; clones share one GPU allocation.
          let texture = ctx.load_texture(
              format!("thumb_{}", resp.served_frames.first().copied().unwrap_or(0)),
              color_image,
              egui::TextureOptions::LINEAR,
          );
          for frame_idx in &resp.served_frames {
              self.thumbnail_cache.put(*frame_idx, texture.clone());
              self.in_flight_requests.remove(frame_idx);
          }
      }
  }
  ```

- [ ] **Step 5: Update the zoom-change block — remove cache wipe, set dirty flag**

  Find the `has_zoomed` block and replace:
  ```rust
  let has_zoomed = (self.zoom_level - self.last_zoom_level).abs() > 0.01;
  if has_zoomed {
      self.last_zoom_level = self.zoom_level;
      self.in_flight_requests.clear();
      if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
          let _ = tx.send(crate::media::ThumbnailCommand::Clear);
      }
  }
  ```
  With:
  ```rust
  let has_zoomed = (self.zoom_level - self.last_zoom_level).abs() > 0.01;
  if has_zoomed {
      self.last_zoom_level = self.zoom_level;
      self.in_flight_requests.clear();
      self.thumbnails_dirty = true;
  }
  ```

- [ ] **Step 6: Update the height-change block — remove cache wipe, set dirty flag**

  Find the `bucketed_height` comparison block and replace:
  ```rust
  if (bucketed_height - self.last_thumbnail_height).abs() > 0.01 {
      self.last_thumbnail_height = bucketed_height;
      self.thumbnail_cache.clear();
      self.in_flight_requests.clear();
      if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
          let _ = tx.send(crate::media::ThumbnailCommand::Clear);
      }
  }
  ```
  With:
  ```rust
  if (bucketed_height - self.last_thumbnail_height).abs() > 0.01 {
      self.last_thumbnail_height = bucketed_height;
      self.in_flight_requests.clear();
      self.thumbnails_dirty = true;
  }
  ```

- [ ] **Step 7: Replace the frame collection and request-sending logic**

  Find the section that starts with `// Collect all frames in the active zone` and ends with the closing `}` of `if !requested_frames.is_empty()`. Replace the entire block (including the `active_zone_frames` HashSet, the `retain` cleanup, and both request loops) with:

  ```rust
  let now = std::time::Instant::now();

  // --- Render visible slots, collect uncached frames ---
  let mut visible_frames: Vec<usize> = Vec::new();
  for k in first_k..=last_k {
      if k < 0 { continue; }
      let target_time = k as f64 * interval;
      if target_time > duration_sec { break; }
      let target_frame = (target_time * fps).round() as usize;
      if target_frame >= total_frames { continue; }

      let tx_px = scroll_offset_x + (k as f64 * step_width as f64);
      let thub_rect = egui::Rect::from_min_size(
          egui::pos2(tx_px as f32, inner_rect.top()),
          egui::vec2(actual_thumb_width, thumb_height),
      );

      if let Some(texture) = self.thumbnail_cache.get(&target_frame) {
          painter.image(
              texture.id(),
              thub_rect,
              egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
              Color32::WHITE,
          );
      } else {
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
          if !self.in_flight_requests.contains_key(&target_frame) {
              visible_frames.push(target_frame);
              self.in_flight_requests.insert(target_frame, now);
          }
      }

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

  // --- Collect padding frame slots (±2 beyond visible) ---
  let padding = 2isize;
  let mut padding_frames: Vec<usize> = Vec::new();
  for k in ((first_k - padding).max(0)..first_k)
      .chain((last_k + 1)..=(last_k + padding))
  {
      if k < 0 { continue; }
      let target_time = k as f64 * interval;
      if target_time > duration_sec { continue; }
      let target_frame = (target_time * fps).round() as usize;
      if target_frame >= total_frames { continue; }
      if self.thumbnail_cache.peek(&target_frame).is_some() { continue; }
      if self.in_flight_requests.contains_key(&target_frame) { continue; }
      padding_frames.push(target_frame);
      self.in_flight_requests.insert(target_frame, now);
  }

  // --- Expire timed-out in-flight requests (allow retry after 2s) ---
  self.in_flight_requests.retain(|_, t| now.duration_since(*t) < std::time::Duration::from_secs(2));

  // --- Dirty flag: set if there is uncached work to do ---
  if !visible_frames.is_empty() || !padding_frames.is_empty() {
      self.thumbnails_dirty = true;
  }

  // --- Send ONE request batch per dirty repaint ---
  if self.thumbnails_dirty {
      if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
          let _ = tx.send(crate::media::ThumbnailCommand::Request {
              visible_frames,
              padding_frames,
              height: bucketed_height as u32,
          });
      }
      self.thumbnails_dirty = false;
  }
  ```

- [ ] **Step 8: Full compile check**

  ```bash
  cargo check 2>&1 | head -50
  ```

  Expected: zero errors. Possible warnings about unused variables are acceptable.

- [ ] **Step 9: Commit**

  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: LRU cache, dirty flag, and split visible/padding request tiers in UI"
  ```

---

## Task 6: Update tests

**Files:**
- Modify: `src/media/thumbnails.rs` (test module only)

- [ ] **Step 1: Replace the existing tests and add the GOP grouping test**

  Replace the entire `#[cfg(test)] mod tests { ... }` block at the bottom of `thumbnails.rs` with:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use std::time::Duration;

      #[test]
      fn test_group_frames_by_gop_deduplicates_seeks() {
          let keyframes = vec![0usize, 60, 120];
          let keyframes_pts = vec![0i64, 60_000, 120_000];

          // Five frames all in GOP-0 → one work item.
          let items = group_frames_by_gop(&[0, 10, 20, 30, 40], &keyframes, &keyframes_pts, 1, 64);
          assert_eq!(items.len(), 1, "all frames in same GOP → one work item");
          assert_eq!(items[0].keyframe_pts, 0);
          assert_eq!(items[0].target_frames.len(), 5);

          // Frames spanning two GOPs → two work items.
          let items2 = group_frames_by_gop(&[0, 60], &keyframes, &keyframes_pts, 1, 64);
          assert_eq!(items2.len(), 2, "frames in two GOPs → two work items");
          // Items are chronologically ordered (BTreeMap).
          assert!(items2[0].keyframe_pts <= items2[1].keyframe_pts);
      }

      #[test]
      fn test_thumbnail_decoder_lifecycle() {
          let test_file = format!("{}/test-files/bbb_sunflower_2160p_60fps_normal.mp4", env!("CARGO_MANIFEST_DIR"));
          let keyframes = vec![0, 60, 120];
          let keyframes_pts = vec![0, 60 * 1000, 120 * 1000];

          let (cmd_tx, resp_rx, _handles) = spawn_thumbnail_decoder_pool(
              test_file, keyframes, keyframes_pts, 60.0,
          );

          cmd_tx.send(ThumbnailCommand::Request {
              visible_frames: vec![0, 60],
              padding_frames: vec![],
              height: 31,
          }).unwrap();

          let mut got_frames = std::collections::HashSet::new();
          let start = std::time::Instant::now();
          while start.elapsed() < Duration::from_secs(10) {
              if let Ok(resp) = resp_rx.recv_timeout(Duration::from_millis(500)) {
                  assert_eq!(resp.height, 31);
                  assert_eq!(resp.rgba_data.len(), (resp.width * resp.height * 4) as usize);
                  for f in resp.served_frames { got_frames.insert(f); }
              }
              if got_frames.contains(&0) && got_frames.contains(&60) { break; }
          }
          assert!(got_frames.contains(&0), "frame 0 not received");
          assert!(got_frames.contains(&60), "frame 60 not received");

          drop(cmd_tx);
      }

      #[test]
      fn test_thumbnail_decoder_preemption() {
          let test_file = format!("{}/test-files/bbb_sunflower_2160p_60fps_normal.mp4", env!("CARGO_MANIFEST_DIR"));
          let keyframes = vec![0, 60, 120];
          let keyframes_pts = vec![0, 60 * 1000, 120 * 1000];

          let (cmd_tx, resp_rx, _handles) = spawn_thumbnail_decoder_pool(
              test_file, keyframes, keyframes_pts, 60.0,
          );

          cmd_tx.send(ThumbnailCommand::Request {
              visible_frames: vec![0, 60, 120],
              padding_frames: vec![],
              height: 31,
          }).unwrap();
          cmd_tx.send(ThumbnailCommand::Request {
              visible_frames: vec![120],
              padding_frames: vec![],
              height: 31,
          }).unwrap();

          let mut got_120 = false;
          let start = std::time::Instant::now();
          while start.elapsed() < Duration::from_secs(10) {
              if let Ok(resp) = resp_rx.recv_timeout(Duration::from_millis(100)) {
                  if resp.served_frames.contains(&120) {
                      got_120 = true;
                      break;
                  }
              }
          }
          assert!(got_120, "frame 120 not received after preemption");
          drop(cmd_tx);
      }
  }
  ```

- [ ] **Step 2: Run thumbnail tests**

  ```bash
  cargo test --lib -- thumbnails 2>&1
  ```

  Expected:
  ```
  test media::thumbnails::tests::test_group_frames_by_gop_deduplicates_seeks ... ok
  test media::thumbnails::tests::test_thumbnail_decoder_lifecycle ... ok
  test media::thumbnails::tests::test_thumbnail_decoder_preemption ... ok

  test result: ok. 3 passed; 0 failed
  ```

- [ ] **Step 3: Run the full test suite**

  ```bash
  cargo test 2>&1 | tail -10
  ```

  Expected: all tests pass.

- [ ] **Step 4: Commit**

  ```bash
  git add src/media/thumbnails.rs
  git commit -m "test: update thumbnail tests for pool API and add GOP grouping test"
  ```

---

## Task 7: Update CONTEXT.md

**Files:**
- Modify: `CONTEXT.md`

- [ ] **Step 1: Append the three new domain terms**

  Add to the end of `CONTEXT.md`:

  ```markdown
  **Thumbnail Decoder Pool**:
  The set of N independent background threads that decode thumbnail GOPs in parallel, each
  owning its own FFmpeg input context and decoder state.
  _Avoid_: Shared decoder, single thumbnail thread

  **GOP Work Item**:
  A unit of work dispatched to a Thumbnail Decoder Pool worker: one keyframe seek target
  plus the list of `target_frames` to harvest in a single forward decode pass.
  _Avoid_: Thumbnail task, decode job

  **Thumbnail LRU Cache**:
  The bounded `LruCache<usize, TextureHandle>` in the UI thread that evicts
  least-recently-accessed thumbnails when the entry count exceeds 300.
  _Avoid_: Thumbnail HashMap, texture store
  ```

- [ ] **Step 2: Commit**

  ```bash
  git add CONTEXT.md
  git commit -m "docs: add Thumbnail Decoder Pool, GOP Work Item, Thumbnail LRU Cache to CONTEXT.md"
  ```

---

## Self-Review

| Spec requirement | Task |
|---|---|
| Protocol: `visible_frames` + `padding_frames` in Request | 1 |
| Protocol: `served_frames` in Response | 1 |
| `group_frames_by_gop` helper | 2 |
| Coordinator thread (generation, FIFO push order) | 3 |
| N independent worker threads | 3 |
| `THUMBNAIL_DECODER_THREADS = 4` + TODO(config) | 3 |
| Abort without partial response on preemption | 3 |
| `PlaybackSession` uses `Vec<JoinHandle>` | 4 |
| LRU cache cap 300 | 5 |
| `thumbnails_dirty` flag | 5 |
| Zoom no longer wipes cache | 5 |
| Visible/padding split in render loop | 5 |
| `in_flight_requests` cleared on viewport change | 5 |
| Existing tests updated for new API | 6 |
| GOP grouping regression test (no FFmpeg) | 6 |
| CONTEXT.md domain terms | 7 |
