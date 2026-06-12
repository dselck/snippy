# Design Specification: Arrow Key Seeking Performance & Benchmarking

Ensure that frame-by-frame seeking (triggered by holding ArrowLeft or ArrowRight) plays at real-time speeds or faster without lagging, clogging the decoder command queue, or causing accidental hard seeks.

## 1. Problem Statement

Holding down ArrowLeft or ArrowRight originally dispatched a precise seek command on every single UI event loop iteration (`update()` tick).
This behavior causes:
1. **Command Flooding**: The decoder thread receives hundreds of seek commands per second.
2. **Sequential Fast-Path Misses**: If the user's input/repaint ticks are faster than the decoder, the target frame index jumps beyond the sequential fast-path limit (currently $+5$ frames). This triggers a hard seek, flushing FFmpeg and seeking the file, which is extremely slow.
3. **Queue Clearing and Lag**: The decoder constantly discards work to start new seeks, leading to stuttering playback well below real-time.

## 2. Proposed Design: Option 3 (Scrub-like Fast Playback) for Hold-Seek

Instead of blocking the UI tick behind a synchronous one-in-flight lock or flooding the decoder with seek commands, we implement a **hybrid hold-seeking model**:

### Forward Seek (Holding ArrowRight)
1. **Transition to Unpaced Playback**: When holding ArrowRight past the 250ms threshold, the UI sends `MediaCommand::PlayUnpaced { sequence_id }` once.
2. **Decoder Loop Streams Sequential Frames**: When `PlayUnpaced` is active, the decoder thread sets `play_mode = true` and `current_sequence_id = sequence_id`. It disables audio and master clock pacing. It decodes and pushes frames to the play queue as fast as it can.
3. **Presenter Thread Forwards Immediately**: The presenter thread bypasses the `VideoSyncEngine` clock sync, forwarding all decoded frames directly to the UI frame channel as fast as they arrive.
4. **UI Thread Drains and Renders Latest**: The UI thread drains the frame channel on every tick and displays only the latest frame, skipping intermediate ones without uploading them to the GPU.
5. **Key Release Pauses**: On releasing ArrowRight, the UI sends `MediaCommand::Pause` to halt the decoder stream.

### Backward Seek (Holding ArrowLeft)
Since FFmpeg cannot decode backward sequentially, holding ArrowLeft continues to send VSync-ticked precise seeks (`MediaCommand::Seek`) on every tick. The GOP cache resolves these seeks quickly.

### Scrubbing Safety
Timeline scrubbing (dragging the mouse) continues to use the single-in-flight `in_flight_seek_seq` lock to prevent flooding.
