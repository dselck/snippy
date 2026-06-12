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

## 2. Proposed Design: VSync-Ticked Seeking with Intelligent Buffer Draining

Instead of blocking the UI tick behind a synchronous one-in-flight lock (which results in frame rate degradation due to scheduling jitter), we will use a **VSync-ticked seeking model** with **intelligent buffer draining** for arrow keys:

1. **Unblocked Tick Seeking**: In the arrow key hold handlers, we do NOT enforce the `in_flight_seek_seq` lock. Instead, we send exactly one seek command on every UI tick (usually 60Hz/VSync). Since seek targets are sequential (+1 frame), this still hits the decoder's sequential fast-path or cache.
2. **Intelligent Buffer Draining**: In the UI frame receive loop, we drain the entire channel of received frames:
   - We discard any stale frames whose sequence ID is less than `self.latest_sequence_id`.
   - Out of the remaining frames, we only keep and render the **latest frame** (the one with the largest sequence ID), discarding intermediate frames without uploading their textures or invoking shader rendering.
   - This prevents rendering overhead from stalling the UI thread.
3. **Scrubbing Safety**: The timeline scrub loop (mouse dragging) will continue to use the single-in-flight `in_flight_seek_seq` lock, as mouse drag events generate updates at a rate much higher than VSync.
4. **Benchmark updates**: The benchmark harness will simulate this unblocked ticked seeking loop with a 60Hz tick rate and buffer draining.
