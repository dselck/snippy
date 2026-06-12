# Design Specification: Arrow Key Seeking Performance & Benchmarking

Ensure that frame-by-frame seeking (triggered by holding ArrowLeft or ArrowRight) plays at real-time speeds without lagging, clogging the decoder command queue, or causing accidental hard seeks.

## 1. Problem Statement

Holding down ArrowLeft or ArrowRight currently dispatches a precise seek command on every single UI event loop iteration (`update()` tick). 
This behavior causes:
1. **Command Flooding**: The decoder thread receives hundreds of seek commands per second.
2. **Sequential Fast-Path Misses**: If the user's input/repaint ticks are faster than the decoder, the target frame index jumps beyond the sequential fast-path limit (currently $+5$ frames). This triggers a hard seek, flushing FFmpeg and seeking the file, which is extremely slow.
3. **Queue Clearing and Lag**: The decoder constantly discards work to start new seeks, leading to stuttering playback well below real-time.

### Play-to-Pause Transition Bug
When the user is playing the video and then holds down an arrow key, the UI sets `self.is_playing = false`. However, because the play/pause state synchronization check is run at the *top* of the UI `update()` loop before the key handlers update `self.is_playing`, the transition is missed. Consequently:
- `self.set_playing(false)` is never called.
- The audio engine remains unpaused.
- The Media Engine does not receive a `Pause` command and still believes it is in playback mode.
- When seek frames arrive, the Media Engine toggles back to `playing = true` and commands the decoder pipeline to play, causing play/seek contention and massive decoding overhead.

### OS Auto-Repeat State-Bypass Bug
OS keyboard auto-repeat registers in egui as repeated `key_pressed` events (usually at a rate of 15Hz to 30Hz). 
When holding down an arrow key:
- The repeated `next_pressed`/`prev_pressed` events trigger the initial transition block in `update()`.
- This resets `held_since` to `Instant::now()` on every tick, meaning `held_since.elapsed()` never reaches the 250ms entrance guard.
- The high-performance throttled loop is bypassed, and seeks are sent at the OS key-repeat rate (~15-30 FPS) without enforcing the `in_flight_seek_seq` lock.
- We must guard the `pressed` check by checking that we are not already in a stepping state.

### Broken Debug Overlay FPS Display
The `decode_fps_frame_count` is defined and initialized in `MainApp` but never incremented or reset in `src/ui/mod.rs`. The `decode_fps` is therefore stuck at 0.0. We need to increment the counter on every received frame and update the FPS value once per second.

## 2. Proposed Design

### A. Arrow Seek State Machine with OS Repeat Guard

We will replace the ad-hoc `left_arrow_held_since` and `right_arrow_held_since` variables in `MainApp` with an explicit state machine:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowSeekState {
    Idle,
    SteppingLeft {
        held_since: std::time::Instant,
    },
    SteppingRight {
        held_since: std::time::Instant,
    },
}
```

State transitions are driven by keyboard inputs in `MainApp::update()`:
* **Key Down Transition**: 
  * Only transition to stepping states if `!is_currently_stepping` (to prevent OS auto-repeat from constantly resetting `held_since`):
    * If `prev_pressed && !is_currently_stepping`: set state to `SteppingLeft`, call `self.set_playing(false)`, and dispatch the first seek.
    * If `next_pressed && !is_currently_stepping`: set state to `SteppingRight`, call `self.set_playing(false)`, and dispatch the first seek.
* **Key Up Transition**:
  * If in state `SteppingLeft` and `ArrowLeft` is released (`!prev_down`): set state to `Idle`.
  * If in state `SteppingRight` and `ArrowRight` is released (`!next_down`): set state to `Idle`.

### B. One-In-Flight Throttling

To prevent queue flooding, we apply the one-in-flight protocol:
1. When a seek is sent, we set `in_flight_seek_seq = Some(latest_sequence_id)`.
2. While `in_flight_seek_seq` is `Some`, we block dispatching any new seek commands from the arrow keys.
3. When the frame with the matching sequence ID arrives, `in_flight_seek_seq` is set back to `None`.
4. While the arrow key is held down (and the entrance guard of 250ms has elapsed):
   * If `in_flight_seek_seq` is `None`, we advance `current_frame` by 1 and dispatch the seek immediately.
   * We call `ctx.request_repaint()` to keep the egui loop spinning, allowing the next seek to fire the moment the frame arrives.

This ensures sequential frames are requested one at a time. Seeking forward will consistently hit the decoder's sequential fast path, yielding real-time playback.

### C. Benchmark Test Harness

We will update the headless benchmark binary `src/bin/bench_arrow_seek.rs` to measure the performance of this hold-seeking loop under two conditions:
1. **Clean State**: Starts from a paused state (ideal).
2. **Transition State**: Starts from an active play state.
   - **Scenario 3 (Buggy)**: Starts playing, then starts seeking without sending `Pause`.
   - **Scenario 4 (Fixed)**: Starts playing, sends `Pause`, then starts seeking.

We will verify that the fixed scenario runs at real-time speeds on `bbb_sunflower_2160p_60fps_normal.mp4`.
