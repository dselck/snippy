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

## 2. Proposed Design: Single-In-Flight Seek with Intelligent Buffer Draining

We will revert the multi-in-flight pipelined seek model to avoid decoder queue contention, complex synchronization, and UI render stalling. Instead, we use a clean **single-in-flight throttled model** combined with **intelligent buffer draining**:

1. **State representation**: We use a single `in_flight_seek_seq: Option<usize>` to track the currently active seek.
2. **Throttling**: The arrow key repeat handler only dispatches a new seek when `in_flight_seek_seq` is `None`. This guarantees that seeks are sequential (+1 frame), consistently hitting the decoder's sequential fast path (or cache) and achieving 40-60 FPS (VSync-capped).
3. **Intelligent Buffer Draining**: In the UI frame receive loop, we discard any received frames whose sequence ID is less than `self.latest_sequence_id`. This performs an intelligent buffer drain: if any intermediate frames are received, they are instantly discarded without uploading textures or rendering, preventing UI thread stalls.
4. **Benchmark Harness**: The benchmark harness will simulate this VSync-capped single-in-flight GUI loop to match the real software behavior.
