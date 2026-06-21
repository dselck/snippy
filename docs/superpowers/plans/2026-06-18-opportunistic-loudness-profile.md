# Opportunistic Loudness Profile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fake sine-wave `loudness_profile: Vec<f32>` on `MainApp` with a real `Vec<Option<f32>>` owned by `PlaybackSession`, populated opportunistically via a side channel as audio chunks arrive during playback/scrubbing.

**Architecture:** `AudioDecoderPipeline::receive_samples()` in the Decoder Thread produces `AudioChunk`s. A new pure function `compute_rms_buckets()` splits each chunk across Loudness Bucket boundaries (4 buckets per frame) and returns `(bucket_idx, rms)` pairs. These are sent through a `crossbeam_channel::unbounded()` side channel to `PlaybackSession`, which drains it in `update()` and accumulates into `Vec<Option<f32>>`. The UI reads the profile via `self.playback.loudness_profile() -> &[Option<f32>]`, skipping bars for `None` buckets and rendering a dim tinted overlay over unanalysed regions. The filmstrip "LOADING" placeholder is upgraded to an animated spinning arc.

**Tech Stack:** Rust, crossbeam-channel, egui 0.34.3, epaint, ffmpeg-next 8.1

---

### Task 1: Extract and test `compute_rms_buckets`

**Files:**
- Modify: `src/media/audio_pipeline.rs`

This is a pure function — test first.

- [ ] **Step 1: Add the failing tests**

Append to the bottom of `src/media/audio_pipeline.rs`:

```rust
#[cfg(test)]
mod tests_rms {
    use super::*;

    #[test]
    fn test_rms_no_tag_returns_empty() {
        let chunk = AudioChunk { data: vec![1.0; 1024], tag: None };
        let result = compute_rms_buckets(&chunk, 30.0, 1, 44100);
        assert!(result.is_empty());
    }

    #[test]
    fn test_rms_silence_produces_zero() {
        let samples_per_bucket = (44100.0f64 / (30.0 * 4.0)).ceil() as usize;
        let chunk = AudioChunk {
            data: vec![0.0; samples_per_bucket],
            tag: Some((0, 0)),
        };
        let result = compute_rms_buckets(&chunk, 30.0, 1, 44100);
        assert!(!result.is_empty());
        for (_, rms) in &result {
            assert_eq!(*rms, 0.0, "silent audio should have rms=0.0");
        }
    }

    #[test]
    fn test_rms_full_scale_sine_near_0707() {
        // RMS of a full-scale sine wave is 1/sqrt(2) ≈ 0.707
        let sample_rate = 44100u32;
        let fps = 30.0;
        let samples_per_bucket = (sample_rate as f64 / (fps * 4.0)) as usize;
        let n_samples = samples_per_bucket * 4;
        let data: Vec<f32> = (0..n_samples)
            .map(|i| {
                (i as f32 * 2.0 * std::f32::consts::PI * 1000.0 / sample_rate as f32).sin()
            })
            .collect();
        let chunk = AudioChunk { data, tag: Some((0, 0)) };
        let result = compute_rms_buckets(&chunk, fps, 1, sample_rate);
        assert!(!result.is_empty());
        for (_, rms) in &result {
            assert!((*rms - 0.707).abs() < 0.02, "expected rms≈0.707, got {rms}");
        }
    }

    #[test]
    fn test_rms_bucket_index_at_100ms() {
        // pts=0.1s → bucket = floor(0.1 / (1/(30*4))) = floor(12.0) = 12
        let chunk = AudioChunk {
            data: vec![1.0f32; 100],
            tag: Some((0, 100_000)), // 100 000 µs = 0.1 s
        };
        let result = compute_rms_buckets(&chunk, 30.0, 1, 44100);
        assert!(!result.is_empty());
        assert_eq!(result[0].0, 12, "wrong bucket index");
    }
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cargo test tests_rms -- --nocapture
```

Expected: `error[E0425]: cannot find function 'compute_rms_buckets'`

- [ ] **Step 3: Implement `compute_rms_buckets`**

Add this public function to `src/media/audio_pipeline.rs` **above** the test module:

```rust
/// Splits a decoded `AudioChunk` across Loudness Bucket boundaries and returns
/// `(bucket_idx, rms)` pairs for every bucket the chunk covers.
///
/// Bucket width = 1 frame / 4  →  `bucket_idx = floor(pts_sec × fps × 4)`.
/// All channels are averaged into the RMS reading.
/// Returns an empty vec when the chunk has no `tag` (position unknown).
pub fn compute_rms_buckets(
    chunk: &AudioChunk,
    fps: f64,
    target_channels: usize,
    target_sample_rate: u32,
) -> Vec<(usize, f32)> {
    let Some((_frames_written, pts_us)) = chunk.tag else {
        return vec![];
    };
    let ch = target_channels.max(1);
    let samples_per_channel = chunk.data.len() / ch;
    if samples_per_channel == 0 {
        return vec![];
    }

    const BUCKETS_PER_FRAME: f64 = 4.0;
    let bucket_dur = 1.0 / (fps * BUCKETS_PER_FRAME);
    let pts_sec = pts_us as f64 / 1_000_000.0;
    let chunk_dur = samples_per_channel as f64 / target_sample_rate as f64;

    let first_bucket = (pts_sec / bucket_dur) as usize;
    let last_bucket = ((pts_sec + chunk_dur) / bucket_dur) as usize;

    let mut result = Vec::with_capacity(last_bucket.saturating_sub(first_bucket) + 1);

    for b in first_bucket..=last_bucket {
        let b_start = b as f64 * bucket_dur;
        let b_end = (b + 1) as f64 * bucket_dur;

        // Sample-frame indices relative to the start of this chunk
        let rel_start =
            ((b_start - pts_sec) * target_sample_rate as f64).max(0.0) as usize;
        let rel_end = ((b_end - pts_sec) * target_sample_rate as f64)
            .min(samples_per_channel as f64) as usize;

        if rel_start >= rel_end {
            continue;
        }

        let mut sum_sq = 0.0f64;
        let mut count = 0usize;
        for s in rel_start..rel_end {
            for c in 0..ch {
                let idx = s * ch + c;
                if let Some(&sample) = chunk.data.get(idx) {
                    sum_sq += (sample as f64) * (sample as f64);
                    count += 1;
                }
            }
        }

        if count > 0 {
            let rms = ((sum_sq / count as f64).sqrt() as f32).clamp(0.0, 1.0);
            result.push((b, rms));
        }
    }

    result
}
```

- [ ] **Step 4: Run tests to confirm they pass**

```bash
cargo test tests_rms -- --nocapture
```

Expected: `test result: ok. 4 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src/media/audio_pipeline.rs
git commit -m "feat: add compute_rms_buckets with tests"
```

---

### Task 2: Thread the loudness side-channel through `MediaEngine`

**Files:**
- Modify: `src/media/mod.rs`

The channel flows: `run_decoder_thread` sends `(bucket_idx, f32)` → `PlaybackSession` receives.

- [ ] **Step 1: Add `loudness_tx` parameter to `MediaEngine::new`**

In `src/media/mod.rs`, locate `pub fn new<P: AsRef<Path>>(` (around line 40). Add `loudness_tx: crossbeam_channel::Sender<(usize, f32)>,` as a parameter after `master_clock`:

```rust
pub fn new<P: AsRef<Path>>(
    path: P,
    target_sample_rate: u32,
    target_channels: usize,
    frame_tx: Sender<DecodedFrame>,
    audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
    cmd_rx: Receiver<MediaCommand>,
    master_clock: crate::audio::clock::MasterClock,
    loudness_tx: crossbeam_channel::Sender<(usize, f32)>,  // ← new
    hw_accel: bool,
    lookahead_depth: usize,
) -> Result<(Self, VideoMetadata), anyhow::Error> {
```

- [ ] **Step 2: Thread `loudness_tx` through `run_decoding_loop`**

In `MediaEngine::new`, the `thread::spawn` closure calls `run_decoding_loop(...)` (around line 117). Add `loudness_tx` to the captured variables and the call. It goes last:

```rust
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
        master_clock_clone,
        keyframes,
        keyframes_pts_clone,
        hw_accel,
        lookahead_depth,
        loudness_tx,  // ← new, last
    );
});
```

Update `fn run_decoding_loop(` signature (around line 168) to add as last parameter:

```rust
fn run_decoding_loop(
    path: &str,
    target_sample_rate: u32,
    target_channels: usize,
    frame_tx: Sender<DecodedFrame>,
    audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
    background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
    fps: f64,
    master_clock: crate::audio::clock::MasterClock,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
    hw_accel: bool,
    lookahead_depth: usize,
    loudness_tx: crossbeam_channel::Sender<(usize, f32)>,  // ← new, last
) -> Result<(), anyhow::Error> {
```

- [ ] **Step 3: Thread `loudness_tx` through `run_decoder_thread`**

`run_decoding_loop` spawns the decoder thread by calling `run_decoder_thread(...)` (around line 194). Add `loudness_tx` to that spawn (last argument):

```rust
thread::spawn(move || {
    let _ = run_decoder_thread(
        path_str,
        target_sample_rate,
        target_channels,
        play_queue_tx,
        audio_tx,
        tags_tx,
        decoder_cmd_rx,
        fps,
        master_clock_clone,
        keyframes_clone,
        keyframes_pts_clone,
        hw_accel,
        loudness_tx,  // ← new, last
    );
});
```

Update `fn run_decoder_thread(` signature (around line 445):

```rust
fn run_decoder_thread(
    path: String,
    target_sample_rate: u32,
    target_channels: usize,
    play_queue_tx: Sender<DecodedFrame>,
    audio_tx: CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>,
    tags_tx: CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>,
    decoder_cmd_rx: Receiver<MediaCommand>,
    fps: f64,
    master_clock: crate::audio::clock::MasterClock,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
    hw_accel: bool,
    loudness_tx: crossbeam_channel::Sender<(usize, f32)>,  // ← new, last
) -> Result<(), anyhow::Error> {
```

- [ ] **Step 4: Call `compute_rms_buckets` and send results in the audio packet branch**

Inside `run_decoder_thread`, find the audio chunk processing block (around line 634):

```rust
if audio_pipeline.send_packet(&packet).is_ok()
    && let Ok(chunks) = audio_pipeline.receive_samples()
        && let Some(sink) = audio_sink_opt.as_mut() {
            sink.push(chunks);
        }
```

Replace with:

```rust
if audio_pipeline.send_packet(&packet).is_ok()
    && let Ok(chunks) = audio_pipeline.receive_samples() {
        for chunk in &chunks {
            for (bucket_idx, rms) in crate::media::audio_pipeline::compute_rms_buckets(
                chunk,
                fps,
                target_channels,
                target_sample_rate,
            ) {
                let _ = loudness_tx.try_send((bucket_idx, rms));
            }
        }
        if let Some(sink) = audio_sink_opt.as_mut() {
            sink.push(chunks);
        }
    }
```

- [ ] **Step 5: Verify it compiles**

```bash
cargo check 2>&1 | head -40
```

Expected: errors only in `session.rs` (the two `MediaEngine::new` call sites that now need `loudness_tx`).

- [ ] **Step 6: Commit**

```bash
git add src/media/mod.rs
git commit -m "feat: thread loudness side-channel through MediaEngine"
```

---

### Task 3: Add `LoudnessProfile` to `PlaybackSession`

**Files:**
- Modify: `src/media/session.rs`

- [ ] **Step 1: Add fields to `PlaybackSession`**

In the `pub struct PlaybackSession {` definition, add after `thumbnail_threads`:

```rust
    // Loudness Profile — opportunistically filled from the loudness side-channel
    loudness_profile: Vec<Option<f32>>,
    loudness_rx: Option<crossbeam_channel::Receiver<(usize, f32)>>,
```

- [ ] **Step 2: Initialise in `PlaybackSession::new()`**

In the `Self { ... }` constructor body, add after `thumbnail_threads: Vec::new()`:

```rust
            loudness_profile: Vec::new(),
            loudness_rx: None,
```

- [ ] **Step 3: Wire the channel in `load_video()` — primary path (with AudioEngine)**

Just before the first `MediaEngine::new(` call (around line 143), create the channel pair:

```rust
let (loudness_tx, loudness_rx) = crossbeam_channel::unbounded::<(usize, f32)>();
```

Add `loudness_tx` to the `MediaEngine::new(` invocation after `master_clock.clone()`:

```rust
if let Ok((engine, meta)) = MediaEngine::new(
    path,
    ae.master_clock.sample_rate(),
    channels,
    frame_tx,
    audio_tx,
    tags_tx,
    cmd_rx,
    master_clock.clone(),
    loudness_tx,   // ← new
    settings.hardware.playback_hw_accel,
    settings.advanced.presenter_lookahead_depth,
) {
```

Inside the `if let Ok((engine, meta))` block, after `self.total_frames = meta.total_frames`:

```rust
self.loudness_profile = vec![None; meta.total_frames * 4];
self.loudness_rx = Some(loudness_rx);
```

- [ ] **Step 4: Wire the channel in `load_video()` — fallback path (no audio device)**

The fallback path calls `MediaEngine::new` around line 282. Apply the same pattern:

Just before that call:
```rust
let (loudness_tx, loudness_rx) = crossbeam_channel::unbounded::<(usize, f32)>();
```

Add `loudness_tx` to the `MediaEngine::new(` invocation (after `master_clock.clone()`):

```rust
if let Ok((engine, meta)) = MediaEngine::new(
    path,
    48000,
    1,
    frame_tx,
    audio_tx,
    tags_tx,
    cmd_rx,
    master_clock.clone(),
    loudness_tx,   // ← new
    settings.hardware.playback_hw_accel,
    settings.advanced.presenter_lookahead_depth,
) {
```

Inside that `if let Ok` block, after `self.total_frames = meta.total_frames`:

```rust
self.loudness_profile = vec![None; meta.total_frames * 4];
self.loudness_rx = Some(loudness_rx);
```

- [ ] **Step 5: Drain the channel in `update()`**

In `PlaybackSession::update()` (around line 321), add a drain loop before the return statement:

```rust
        // Drain loudness side-channel — write each bucket at most once
        if let Some(rx) = &self.loudness_rx {
            while let Ok((bucket_idx, rms)) = rx.try_recv() {
                if let Some(slot) = self.loudness_profile.get_mut(bucket_idx) {
                    if slot.is_none() {
                        *slot = Some(rms);
                    }
                }
            }
        }
```

- [ ] **Step 6: Add the public accessor**

Anywhere in `impl PlaybackSession`:

```rust
    pub fn loudness_profile(&self) -> &[Option<f32>] {
        &self.loudness_profile
    }
```

- [ ] **Step 7: Verify it compiles**

```bash
cargo check 2>&1 | head -40
```

Expected: errors only in `src/ui/mod.rs` (references to the removed `MainApp.loudness_profile` field).

- [ ] **Step 8: Commit**

```bash
git add src/media/session.rs
git commit -m "feat: add LoudnessProfile to PlaybackSession with side-channel drain"
```

---

### Task 4: Update the waveform render to use real data

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Remove the mock `loudness_profile` field from `MainApp`**

In the `pub struct MainApp { ... }` definition, delete:

```rust
    // Mock Audio Loudness Profile (for visual layout)
    pub loudness_profile: Vec<f32>,
```

- [ ] **Step 2: Remove the mock generation in `MainApp::new()`**

Delete the block:

```rust
        // Generate a mock loudness profile to show a beautiful timeline waveform
        let mut loudness = Vec::with_capacity(300);
        for i in 0..300 {
            let sine = (i as f32 * 0.15).sin().abs();
            let noise = (i as f32 * 0.45).cos().abs() * 0.3;
            let val = (sine * 0.7 + noise * 0.3) * 0.8;
            loudness.push(val.clamp(0.05, 1.0));
        }
```

Also remove from the `Self { ... }` initializer:

```rust
            loudness_profile: loudness,
```

- [ ] **Step 3: Rewrite the waveform render loop**

Find the section starting with `// Render mock waveform loudness bars zoomed in sync` (around line 1356). Replace from that comment through the closing `}` of the bar loop (ending around line 1409) with:

```rust
                    // Render real loudness profile bars — one bar per pixel column
                    let loudness = self.playback.loudness_profile();
                    let total_buckets = loudness.len();
                    let bar_count = wave_rect.width() as usize;
                    let bar_width = 1.0_f32;
                    let middle_y = wave_rect.center().y;

                    // Track contiguous unloaded x-spans for the loading overlay
                    let mut loading_span_start: Option<f32> = None;

                    for i in 0..bar_count {
                        let pct = i as f32 / bar_count as f32;
                        let frame_idx_isize =
                            start_frame + (pct * visible_range_width) as isize;
                        let out_of_bounds = frame_idx_isize < 0
                            || frame_idx_isize > self.playback.total_frames() as isize;

                        let bucket_idx = if out_of_bounds {
                            None
                        } else {
                            let fi = frame_idx_isize as usize;
                            let bi = fi * 4;
                            if bi < total_buckets { Some(bi) } else { None }
                        };

                        let amplitude_opt = bucket_idx.and_then(|bi| loudness[bi]);

                        match amplitude_opt {
                            None => {
                                if loading_span_start.is_none() {
                                    loading_span_start =
                                        Some(wave_rect.left() + i as f32 * bar_width);
                                }
                                continue;
                            }
                            Some(amplitude) => {
                                // Flush any accumulated loading span
                                if let Some(lx) = loading_span_start.take() {
                                    let rx = wave_rect.left() + i as f32 * bar_width;
                                    painter.rect_filled(
                                        egui::Rect::from_min_max(
                                            egui::pos2(lx, wave_rect.top() + 1.0),
                                            egui::pos2(rx, wave_rect.bottom() - 1.0),
                                        ),
                                        0.0,
                                        Color32::from_rgba_premultiplied(55, 35, 75, 45),
                                    );
                                }

                                let frame_idx = frame_idx_isize.clamp(
                                    0,
                                    self.playback.total_frames() as isize,
                                ) as usize;
                                let bar_h = amplitude * (wave_rect.height() - 6.0);
                                let x = wave_rect.left() + i as f32 * bar_width;

                                let inside_active_selection =
                                    match (self.in_marker, self.out_marker) {
                                        (Some(inf), Some(outf)) => {
                                            frame_idx >= inf && frame_idx <= outf
                                        }
                                        _ => false,
                                    };
                                let inside_queued_range = self
                                    .ranges
                                    .iter()
                                    .any(|r| frame_idx >= r.in_frame && frame_idx <= r.out_frame);

                                let bar_color = if out_of_bounds {
                                    Color32::from_rgb(20, 19, 32)
                                } else if inside_active_selection {
                                    Color32::from_rgb(59, 130, 246)
                                } else if inside_queued_range {
                                    if self.cut_mode {
                                        Color32::from_rgb(239, 68, 68)
                                    } else {
                                        Color32::from_rgb(16, 185, 129)
                                    }
                                } else if self.cut_mode {
                                    Color32::from_rgb(12, 55, 35)
                                } else {
                                    Color32::from_rgb(85, 30, 30)
                                };

                                painter.line_segment(
                                    [
                                        egui::pos2(x + bar_width * 0.5, middle_y - bar_h * 0.5),
                                        egui::pos2(x + bar_width * 0.5, middle_y + bar_h * 0.5),
                                    ],
                                    egui::Stroke::new(bar_width, bar_color),
                                );
                            }
                        }
                    }

                    // Flush any trailing loading span at the right edge
                    if let Some(lx) = loading_span_start.take() {
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(lx, wave_rect.top() + 1.0),
                                egui::pos2(wave_rect.right(), wave_rect.bottom() - 1.0),
                            ),
                            0.0,
                            Color32::from_rgba_premultiplied(55, 35, 75, 45),
                        );
                    }
```

- [ ] **Step 4: Verify it compiles cleanly**

```bash
cargo check 2>&1 | head -40
```

Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: wire waveform render to real LoudnessProfile with loading overlay"
```

---

### Task 5: Filmstrip animated spinner

**Files:**
- Modify: `src/ui/mod.rs`

Replace the nearly-invisible "LOADING" text placeholder with a small animated spinning arc.

- [ ] **Step 1: Replace the placeholder block**

Find the filmstrip `else` branch (around line 1229):

```rust
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
```

Replace with:

```rust
                        } else {
                            // Placeholder loading card with animated spinner
                            painter.rect_filled(thub_rect, 4.0, Color32::from_rgb(18, 17, 28));
                            painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(45, 43, 75)), egui::StrokeKind::Inside);
                            if inner_height > 16.0 {
                                // Spinning arc: one full revolution every 1.2 s
                                let t = ui.ctx().input(|i| i.time) as f32;
                                let angle = t * (2.0 * std::f32::consts::PI / 1.2);
                                let center = thub_rect.center();
                                let r = (thub_rect.size().min_elem() * 0.22).max(4.0);
                                let arc_fraction = 0.65_f32;
                                let n_segments = 20usize;
                                let arc_color = Color32::from_rgb(110, 90, 160);
                                let points: Vec<egui::Pos2> = (0..=n_segments)
                                    .map(|k| {
                                        let a = angle
                                            + k as f32 / n_segments as f32
                                                * arc_fraction
                                                * std::f32::consts::TAU;
                                        egui::pos2(
                                            center.x + r * a.cos(),
                                            center.y + r * a.sin(),
                                        )
                                    })
                                    .collect();
                                for seg in points.windows(2) {
                                    painter.line_segment(
                                        [seg[0], seg[1]],
                                        egui::Stroke::new(1.5, arc_color),
                                    );
                                }
                                ui.ctx().request_repaint();
                            }
```

- [ ] **Step 2: Build and visually verify**

```bash
cargo run 2>&1 | head -20
```

Expected: app launches, filmstrip shows spinning purple arcs for unloaded thumbnails, waveform fills in with real RMS bars as audio plays, unanalysed regions show a dim purple tint.

- [ ] **Step 3: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: animated spinner for filmstrip loading placeholders"
```
