# Design Spec: Zero-Copy Video Frame Caching and Rendering

To eliminate the redundant CPU copies of raw pixel data (`YuvPlanes`) in the Snippy2 decoding pipeline.

## Problem Statement
A single 1080p YUV420p video frame is approximately `1920 * 1080 * 1.5 ≈ 3.11 MB`. In the current implementation, every decoded frame is cloned multiple times:
1. When extracted from `DecodedFrame::Cpu` in the decoder pipeline loop.
2. When wrapped into the final `DecodedFrame` returned to the pipeline's caller.
3. When cached inside `GopCache`.
4. When cloned to pass to the UI rendering thread.

These redundant clones copy about 9.3 MB of pixel data per frame, leading to high CPU usage, memory allocation thrashing, and memory bandwidth bottlenecking.

## Proposed Solution (Approach A)
Wrap the large, read-only `YuvPlanes` struct in a reference-counted pointer (`std::sync::Arc`) in the primary frame structures:
- `DecodedFrame::Cpu`
- `CachedFrame`
- `YuvCallback`

By using `Arc<YuvPlanes>`, cloning a frame will only copy the pointer and increment the reference count, completely avoiding deep copies of video memory. Downstream GPU upload code will access the underlying fields via `Arc` dereferencing.

---

## Detailed Changes

### `src/media/types.rs`
Update `DecodedFrame::Cpu` to hold `Arc<YuvPlanes>`.
```rust
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum DecodedFrame {
    Cpu {
        frame_idx: usize,
        planes: Arc<YuvPlanes>,
        sequence_id: usize,
        pts_sec: f64,
    },
    // ...
}
```

### `src/media/cache.rs`
Update `CachedFrame` to hold `Arc<YuvPlanes>`.
```rust
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct CachedFrame {
    pub planes: Arc<YuvPlanes>,
    pub pts_sec: f64,
}
```

### `src/media/ffmpeg_adapter.rs`
Wrap `YuvPlanes` in `Arc::new(...)` when creating `DecodedFrame::Cpu`.
```rust
use std::sync::Arc;

// Inside receive_frame():
let planes = Arc::new(YuvPlanes {
    y: video_frame.data(0).to_vec(),
    u: video_frame.data(1).to_vec(),
    v: video_frame.data(2).to_vec(),
    width: video_frame.width(),
    height: video_frame.height(),
    y_stride: video_frame.stride(0) as u32,
    u_stride: video_frame.stride(1) as u32,
    v_stride: video_frame.stride(2) as u32,
});
```

### `src/media/decoder_pipeline.rs`
Update caching and retrieval to clone only `Arc` pointers.
- In `get_cached_frame`:
  ```rust
  Some(DecodedFrame::Cpu {
      frame_idx: frame_index,
      planes: cached.planes.clone(), // Pointer clone
      sequence_id: self.current_sequence_id,
      pts_sec: cached.pts_sec,
  })
  ```
- In `process_packet`:
  Destructure the returned `DecodedFrame::Cpu` directly to consume the `Arc` without clone/ref.
  ```rust
  while let Some(decoded) = self.adapter.receive_frame()? {
      let (frame_idx, pts_sec, planes) = match decoded {
          DecodedFrame::Cpu { frame_idx, pts_sec, planes, .. } => (frame_idx, pts_sec, planes),
          _ => unreachable!(),
      };

      let final_frame = DecodedFrame::Cpu {
          frame_idx,
          planes: planes.clone(), // Pointer clone
          sequence_id: self.current_sequence_id,
          pts_sec,
      };

      self.gop_cache.insert_frame(frame_idx, CachedFrame { planes: planes.clone(), pts_sec }); // Pointer clone
      // ...
  }
  ```

### `src/ui/mod.rs`
Update `YuvCallback` to hold `Arc<media::YuvPlanes>` and update paint callback construction.
```rust
struct YuvCallback {
    planes: std::sync::Arc<media::YuvPlanes>,
}

// Inside paint_callback construction:
YuvCallback { planes: planes.clone() }
```

---

## Verification Plan

### Automated Tests
Run unit tests to ensure functional correctness:
```bash
cargo test
```

### Profiling and Benchmarking
Re-run benchmarks to verify the optimization results:
```bash
SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv" cargo run --release --bin bench_decode -- --profile
```
Confirm that:
1. Decoding throughput (FPS) increases.
2. SVG flamegraphs generated in `target/profile/` show a significant reduction in the CPU proportion of `process_packet` and fewer vector allocation/memcpy operations.
