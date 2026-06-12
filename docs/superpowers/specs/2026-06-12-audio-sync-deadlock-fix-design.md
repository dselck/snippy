# Design Spec: Audio Synchronization and Deadlock Fix

## 1. Problem Statement
The Snippy2 media player is encountering two critical bugs:
1. **Startup Jitter and Silent Audio**: When playing from frame 0 (both initially and when seeking back to 0), the audio does not start and the video is extremely jittery.
2. **Broken Seek Deadlock**: When seeking after playback, the player hangs, deadlocks, and ignores further seeking commands.

## 2. Root Cause Analysis
- **Startup Jitter**: `MasterClock::reset()` sets `audio_paused_for_sync = true`. When playback starts, `MasterClock::play()` sets `is_playing = true` but fails to clear `audio_paused_for_sync`. As a result, the audio callback remains inactive, outputs silence, and never pops samples. The video engine evaluates frames against a frozen clock, causing severe jitter.
- **Seek Deadlock**: Because the audio callback never pops samples, the `audio_tx` queue remains completely full. During seek/play, the media thread reads interleaved packets. When it encounters an audio packet, it tries to push samples to `audio_tx`. Because the queue is full and we are paused/seeking, it sleeps up to 15 seconds per chunk, locking up the decoder thread and starving the command receiver (`decoder_cmd_rx`).

## 3. Design Changes

### A. Clock Unpausing (`src/audio/clock.rs`)
In `MasterClock::play()`, clear the `audio_paused_for_sync` flag by setting it to `false`. This immediately activates the clock ticker and CPAL callback when playback begins.

### B. Non-Deadlocking Audio Queue Push (`src/media/mod.rs`)
Rewrite the audio chunk push loop:
- Check `audio_tx.vacant_len()` at the chunk level.
- If the queue does not have enough space, sleep briefly (e.g., 2ms) and poll `decoder_cmd_rx`.
- If a command (like `Seek` or `Shutdown`) is received, abort the current chunk push immediately so the media thread can handle the new command.

### C. Simulated Consumer Generation Drop (`src/bench/mod.rs`)
Modify the headless simulated consumer to always check and discard stale samples when the generation changes, even if the clock is paused. This mirrors the real CPAL behavior and prevents the simulation queue from remaining permanently full during paused seeks.

## 4. Verification Plan
1. Compile the code successfully: `cargo build`.
2. Run unit tests: `cargo test`.
3. Run playback sync test: `SNIPPY_BENCH_VIDEO=/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv cargo run --bin test_playback_sync`.
4. Run playback jitter benchmark: `SNIPPY_BENCH_VIDEO=/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv cargo run --bin bench_playback_jitter`.
5. Confirm jitter statistics and check that median absolute timing jitter is below 5ms.
