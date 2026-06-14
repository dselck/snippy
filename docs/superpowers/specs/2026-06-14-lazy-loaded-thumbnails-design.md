# Design Document: Lazy-Loaded Thumbnail Rendering & Preemption

## Problem Statement
The original thumbnail loader had two major issues:
1. **Spotty performance (lost requests)**: During active decoding, if the UI thread sent new/updated thumbnail requests (e.g. due to user scrolling or zooming), the background decoder thread's `try_recv()` loop would consume/pop those requests and discard them. This left the UI in a state where it believed a request was in-flight (the frame index remained in the `in_flight_requests` set), but the background thread had thrown it away. Thus, those thumbnails remained stuck in a "LOADING" state.
2. **Eager/Redundant loading**: The system attempted to load every frame/thumbnail, regardless of whether it was currently visible or about to be visible in the user's active viewport.

---

## Proposed and Implemented Solution

### 1. Non-Discarding Preemption in the Background Decoder Thread
In `src/media/thumbnails.rs`, the thread loop was refactored:
- An `active_request` or `current_cmd` option is maintained.
- Instead of using a simple `while let Ok(cmd) = cmd_rx.recv()` loop that cannot easily handle preemption, we use a `loop` which retrieves the next command from either `current_cmd` (if populated by a preemption) or blocks/waits on `cmd_rx.recv()`.
- During the decoding loop, if a new command is received on `cmd_rx.try_recv()`, the new command is saved into `current_cmd`, the current loop is flagged as aborted, and we break to immediately process the new command without losing it.

### 2. Active Zone & Padded Lazy Loading in the UI
In `src/ui/mod.rs`, we implemented:
- A sliding "active zone" around the visible thumbnails (defined as `[first_k - 2, last_k + 2]`).
- Only the thumbnails falling within this active zone are considered for loading/decoding.
- Any request for a frame that is no longer within this active zone is purged from the `in_flight_requests` map, allowing the resources to focus on current visible areas.

### 3. Request Tracking with Expiration (Timeout)
- `in_flight_requests` was converted from a `HashSet<usize>` to a `HashMap<usize, std::time::Instant>`.
- Every time a request is made, the current timestamp is recorded.
- In every rendering frame, any in-flight request that has exceeded 2 seconds is cleared from `in_flight_requests`. This allows the UI to automatically retry loading any request that might have been dropped or aborted during rapid scrolling.

---

## Verification and Testing
A new unit test `test_thumbnail_decoder_preemption` was added to `src/media/thumbnails.rs` to ensure that:
1. Submitting a new request correctly preempts an ongoing, larger request.
2. The new request is successfully processed and the requested frame is returned.
3. The test and all other 17 existing tests pass.
