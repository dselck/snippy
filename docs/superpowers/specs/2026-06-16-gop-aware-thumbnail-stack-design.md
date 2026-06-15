# Design Document: GOP-Aware Thumbnail Stack

## Problem Statement

The current thumbnail decoder is GOP-blind. For every requested `target_frame` it seeks
to the nearest keyframe and decodes forward until it reaches that frame. When multiple
requested frames share the same GOP (common at low zoom levels, where the interval between
visible slots is smaller than the GOP size), the decoder seeks to the same keyframe
repeatedly — one full seek + decode-forward pass per frame. This is the dominant
performance bottleneck in the filmstrip at low zoom.

Secondary issues:
- The thumbnail cache grows without bound (a memory leak over long sessions).
- A single decoder thread serialises all GOP work, so the initial fill of a wide filmstrip
  is slow even after the redundant-seek problem is fixed.
- Every repaint that finds an uncached frame immediately fires a new command, flooding the
  channel and causing unnecessary thrashing during fast scroll or zoom.
- Zoom changes nuke the entire cache even though previously decoded thumbnails remain valid.

---

## Design

### 1. Protocol Changes

#### `ThumbnailCommand::Request`

Split the single `frames: Vec<usize>` into two priority tiers so the coordinator can
schedule visible work before prefetch work:

```rust
pub enum ThumbnailCommand {
    Request {
        visible_frames: Vec<usize>, // slots currently on screen
        padding_frames: Vec<usize>, // ±2 preload zone
        height: u32,
    },
    Clear,
}
```

#### `ThumbnailResponse`

Replace the single `frame_idx: usize` with `served_frames: Vec<usize>` — the complete list
of every `target_frame` that this one decode satisfies. One response now covers an entire
GOP group.

```rust
pub struct ThumbnailResponse {
    pub served_frames: Vec<usize>,
    pub width: u32,
    pub height: u32,
    pub rgba_data: Vec<u8>,
}
```

---

### 2. Decoder Thread Pool (`src/media/thumbnails.rs`)

`spawn_thumbnail_decoder` is replaced by `spawn_thumbnail_decoder_pool`.

#### Constants

```rust
// TODO(config): expose THUMBNAIL_DECODER_THREADS as a user-configurable setting.
const THUMBNAIL_DECODER_THREADS: usize = 4;
```

#### Coordinator Thread

A single coordinator thread owns the inbound command channel and is responsible for all
scheduling logic. It does **no decoding itself**.

On receiving a `Request`:

1. Increment a shared `generation: Arc<AtomicU64>`. Workers observe this to detect
   preemption.
2. Group `visible_frames` by nearest keyframe → list of visible-GOP work items.
3. Group `padding_frames` by nearest keyframe → list of padding-GOP work items.
4. Push all work items into a shared lock-free FIFO queue (crossbeam `SegQueue`).
   Priority is achieved by push order: all visible-GOP items are pushed before any
   padding-GOP items. Workers pull in FIFO order, so visible GOPs are always consumed
   first.

On receiving `Clear`: increment generation and drain the work queue.

#### Worker Threads (×N)

Each worker thread is fully independent: it owns its own `ffmpeg` input context, codec
context, and scaler. No shared decoder state between workers.

Per work item `(keyframe_pts, target_frames: Vec<usize>, generation_at_dispatch)`:

1. Check `current_generation == generation_at_dispatch`; if stale, discard and loop.
2. Seek to `keyframe_pts`.
3. Decode forward in a single pass, collecting frames for every index in `target_frames`
   as they arrive.
4. After each packet, re-check the generation; if stale, **abort without emitting a
   response**. The next batch will re-request any uncovered frames; emitting a partial
   response would leave `in_flight_requests` entries dangling.
5. Emit one `ThumbnailResponse { served_frames: target_frames, ... }` only when the
   entire GOP pass completes successfully.

#### GOP Priority Ordering

Within each priority tier (visible / padding), GOPs are ordered chronologically. This
ensures the worker pool fills the left-to-right visible area in temporal order, which
matches the user's mental model.

---

### 3. LRU Thumbnail Cache (`src/ui/mod.rs`)

Replace:
```rust
pub thumbnail_cache: std::collections::HashMap<usize, egui::TextureHandle>,
```

With:
```rust
pub thumbnail_cache: lru::LruCache<usize, egui::TextureHandle>,
```

**Cap:** 300 entries. At typical zoom levels (10–20 visible thumbnails + 4 padding) this
covers roughly 12 screens of scroll history before eviction begins.

**Receiving a response:**

```rust
let texture = ctx.load_texture(
    format!("thumb_{}", resp.served_frames[0]),
    color_image,
    egui::TextureOptions::LINEAR,
);
for frame_idx in resp.served_frames {
    self.thumbnail_cache.put(frame_idx, texture.clone()); // Arc clone, one GPU allocation
    self.in_flight_requests.remove(&frame_idx);
}
```

`egui::TextureHandle` is an `Arc` over the GPU allocation. Cloning it is O(1) and does
not duplicate texture memory. All entries in the same GOP group share one GPU texture.

**Zoom and resize changes no longer wipe the cache.** Previously decoded thumbnails remain
valid at any zoom level. LRU eviction handles natural turnover as new frames are requested.
`in_flight_requests` is still cleared on zoom/resize to cancel stale work; generation
increment in the coordinator preempts any in-progress decoding.

**Cache lookup in the render loop is unchanged:** `thumbnail_cache.get(&target_frame)`.
The LRU `.get()` call automatically bumps the entry to most-recently-used.

---

### 4. Dirty-Flag Debounce (`src/ui/mod.rs`)

Add `thumbnails_dirty: bool` to `MainApp` (initialised `true` to trigger the first fill
on load).

**Setting the flag:** Any event that changes the visible frame set sets
`thumbnails_dirty = true` and clears `in_flight_requests` (stale in-flight entries from
the previous viewport are no longer relevant):
- Scroll offset changes (playhead moves, user drags filmstrip)
- Zoom level changes
- Panel resize causes `bucketed_height` to change

When building the request batch, every frame in both `visible_frames` and `padding_frames`
that is not already cached is inserted into `in_flight_requests` before the command is
sent — identical to the existing per-frame insertion, just now covering all GOP-grouped
frames in both tiers at once.

**Sending the request:** At the end of each repaint, after the render loop has identified
all `visible_frames` and `padding_frames`:

```rust
if self.thumbnails_dirty {
    if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
        let _ = tx.send(ThumbnailCommand::Request {
            visible_frames,
            padding_frames,
            height: bucketed_height as u32,
        });
    }
    self.thumbnails_dirty = false;
}
```

This collapses any number of rapid viewport changes within one frame into a single
well-timed request. No more per-repaint flooding of the command channel.

---

## Data Flow Summary

```
UI Thread (mod.rs)
  │  sets thumbnails_dirty on any viewport change
  │  on repaint: collects visible_frames + padding_frames
  │  sends ONE ThumbnailCommand::Request per dirty repaint
  ▼
Coordinator Thread (thumbnails.rs)
  │  increments generation (preempts workers)
  │  groups frames by nearest keyframe → GOP work items
  │  visible GOPs → queue front; padding GOPs → queue back
  ▼
Worker Threads ×4 (thumbnails.rs)
  │  each owns independent FFmpeg context
  │  seek once per GOP, decode forward, collect all target frames
  │  emit ThumbnailResponse { served_frames, rgba_data }
  ▼
UI Thread (mod.rs)
  │  load_texture once per response
  │  thumbnail_cache.put(frame_idx, texture.clone()) for each served_frame
  └  LruCache evicts beyond 300 entries automatically
```

---

## Dependency Addition

Add to `Cargo.toml`:
```toml
lru = "0.12"
```

---

## Domain Terms (CONTEXT.md additions)

- **Thumbnail Decoder Pool**: The set of N independent background threads that decode
  thumbnail GOPs in parallel, each owning its own FFmpeg context.
- **GOP Work Item**: A unit of work dispatched to a worker thread: one keyframe seek target
  plus the list of `target_frames` to harvest in a single forward decode pass.
- **Thumbnail LRU Cache**: The bounded `LruCache<usize, TextureHandle>` in the UI thread
  that evicts least-recently-accessed thumbnails when the entry count exceeds the cap.

---

## What Is Not Changed

- The UI rendering loop — `thumbnail_cache.get(&target_frame)` is identical.
- The `in_flight_requests: HashMap<usize, Instant>` tracking structure and timeout logic.
- The `get_nearest_keyframe` helper — reused by the coordinator for GOP grouping.
- The `bucketed_height` height quantisation to prevent churn on resize.
- The exponential backoff on request timeout (already noted as a follow-up from the
  previous lazy-loading design).
