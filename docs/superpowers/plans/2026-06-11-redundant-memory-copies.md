# Zero-Copy Video Frame Caching and Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate redundant CPU video memory copies by wrapping `YuvPlanes` in `std::sync::Arc` across the decoder pipeline, GOP cache, and rendering thread.

**Architecture:** Change `DecodedFrame::Cpu` and `CachedFrame` to store `Arc<YuvPlanes>` instead of owned `YuvPlanes`. Update `FfmpegDecoderAdapter` to wrap the decoded frame in `Arc`, and modify the decoder pipeline and UI thread to perform pointer clones (increasing reference counts) rather than deep copies.

**Tech Stack:** Rust standard library (`std::sync::Arc`).

---

### Task 1: Update DecodedFrame enum variant definition

**Files:**
- Modify: [src/media/types.rs](file:///Users/dselck/Documents/Snippy2/src/media/types.rs)

- [ ] **Step 1: Modify DecodedFrame::Cpu variant**

Change the `planes` field of the `Cpu` variant in `DecodedFrame` from `YuvPlanes` to `std::sync::Arc<YuvPlanes>`.

```rust
// In src/media/types.rs around lines 30-37
#[derive(Clone, Debug)]
pub enum DecodedFrame {
    Cpu {
        frame_idx: usize,
        planes: std::sync::Arc<YuvPlanes>,
        sequence_id: usize,
        pts_sec: f64,
    },
    Gpu {
        frame_idx: usize,
        descriptor: GpuFrameDescriptor,
        sequence_id: usize,
        pts_sec: f64,
    },
}
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Fails only with compilation errors in `src/media/cache.rs` and downstream modules.

- [ ] **Step 3: Commit**

```bash
git add src/media/types.rs
git commit -m "refactor: wrap DecodedFrame::Cpu planes in Arc"
```

---

### Task 2: Update CachedFrame definition

**Files:**
- Modify: [src/media/cache.rs](file:///Users/dselck/Documents/Snippy2/src/media/cache.rs)

- [ ] **Step 1: Modify CachedFrame struct fields**

Change `pub planes: YuvPlanes` to `pub planes: std::sync::Arc<YuvPlanes>` in `CachedFrame`.

```rust
// In src/media/cache.rs around lines 4-9
#[derive(Debug, Clone)]
pub struct CachedFrame {
    pub planes: std::sync::Arc<YuvPlanes>,
    pub pts_sec: f64,
}
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Fails only with errors in `src/media/ffmpeg_adapter.rs`, `src/media/decoder_pipeline.rs`, and UI modules.

- [ ] **Step 3: Commit**

```bash
git add src/media/cache.rs
git commit -m "refactor: wrap CachedFrame planes in Arc"
```

---

### Task 3: Wrap decoded frame in Arc in FFmpeg adapter

**Files:**
- Modify: [src/media/ffmpeg_adapter.rs](file:///Users/dselck/Documents/Snippy2/src/media/ffmpeg_adapter.rs)

- [ ] **Step 1: Wrap YuvPlanes construction in Arc**

Modify `FfmpegDecoderAdapter::receive_frame` to wrap the parsed `YuvPlanes` in `std::sync::Arc::new(...)`.

```rust
// In src/media/ffmpeg_adapter.rs around lines 67-91
                let planes = std::sync::Arc::new(YuvPlanes {
                    y: video_frame.data(0).to_vec(),
                    u: video_frame.data(1).to_vec(),
                    v: video_frame.data(2).to_vec(),
                    width: video_frame.width(),
                    height: video_frame.height(),
                    y_stride: video_frame.stride(0) as u32,
                    u_stride: video_frame.stride(1) as u32,
                    v_stride: video_frame.stride(2) as u32,
                });

                let pts_sec = match frame.pts() {
                    Some(pts_val) => pts_val as f64 * self.time_base_f64 - self.video_start_time,
                    None => self.fallback_frame_idx as f64 / self.fps,
                };
                let frame_idx = (pts_sec * self.fps).round() as usize;
                self.fallback_frame_idx = frame_idx + 1;

                Ok(Some(DecodedFrame::Cpu {
                    frame_idx,
                    planes,
                    sequence_id: 0,
                    pts_sec,
                }))
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Fails only with errors in `src/media/decoder_pipeline.rs` and UI modules.

- [ ] **Step 3: Commit**

```bash
git add src/media/ffmpeg_adapter.rs
git commit -m "refactor: wrap video frame planes in Arc in ffmpeg adapter"
```

---

### Task 4: Update VideoDecoderPipeline caching and destructuring

**Files:**
- Modify: [src/media/decoder_pipeline.rs](file:///Users/dselck/Documents/Snippy2/src/media/decoder_pipeline.rs)

- [ ] **Step 1: Consume decoded frame planes in process_packet**

Modify `VideoDecoderPipeline::process_packet` to destructure the decoded frame and bind `planes` directly by value. Since `planes` is an `Arc`, `planes.clone()` only performs reference-counted pointer copies.

```rust
// In src/media/decoder_pipeline.rs around lines 153-177
        while let Some(decoded) = self.adapter.receive_frame()? {
            let (frame_idx, pts_sec, planes) = match decoded {
                DecodedFrame::Cpu { frame_idx, pts_sec, planes, .. } => (frame_idx, pts_sec, planes),
                _ => unreachable!(),
            };

            let final_frame = DecodedFrame::Cpu {
                frame_idx,
                planes: planes.clone(),
                sequence_id: self.current_sequence_id,
                pts_sec,
            };

            // GOP Cache population
            if self.keyframes.binary_search(&frame_idx).is_ok() {
                self.gop_cache.clear_if_different_gop(frame_idx);
            }
            if self.gop_cache.keyframe_idx == usize::MAX {
                let target_kf = Self::get_nearest_keyframe(&self.keyframes, frame_idx);
                self.gop_cache.clear_if_different_gop(target_kf);
            }
            self.gop_cache.insert_frame(frame_idx, CachedFrame { planes: planes.clone(), pts_sec });

            self.last_decoded_frame = Some(frame_idx);
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Fails only with errors in `src/ui/mod.rs`.

- [ ] **Step 3: Commit**

```bash
git add src/media/decoder_pipeline.rs
git commit -m "refactor: avoid deep copy on GOP caching in decoder pipeline"
```

---

### Task 5: Update UI Thread and YuvCallback

**Files:**
- Modify: [src/ui/mod.rs](file:///Users/dselck/Documents/Snippy2/src/ui/mod.rs)

- [ ] **Step 1: Update YuvCallback struct definition**

Modify `YuvCallback` definition to store `std::sync::Arc<media::YuvPlanes>`.

```rust
// In src/ui/mod.rs around lines 1784-1786
struct YuvCallback {
    planes: std::sync::Arc<media::YuvPlanes>,
}
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Compilation passes completely without errors.

- [ ] **Step 3: Run unit tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/mod.rs
git commit -m "refactor: update UI callback to hold Arc of YuvPlanes"
```

---

### Task 6: Verify Performance Gains

**Files:**
- Test: [src/bin/bench_decode.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_decode.rs)

- [ ] **Step 1: Run performance benchmarks with profile output**

Run:
```bash
SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv" cargo run --release --bin bench_decode -- --profile
```

Expected:
- Benchmarks run successfully.
- Correctness passes.
- Output SVG flamegraphs in `target/profile/` show reduced CPU cycles spent in `process_packet` and fewer vector cloning operations.
- FPS throughput in burst and sustained decoding increases compared to baseline.
