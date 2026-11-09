# src/audio

## Overview
Real-time audio playback engine utilizing CPAL and lock-free ring buffers, alongside a thread-safe MasterClock for audio/video synchronization and PTS tracking.

## Interface
- `[mod.rs:AudioEngine]` - CPAL output stream manager providing real-time audio playback
- `[mod.rs:AudioEngine::new]` - Initializes default OS audio output device and starts audio stream
- `[mod.rs:AudioEngine::set_muted]` - Mutes or unmutes stream playback
- `[mod.rs:AudioEngine::play]` - Resumes the CPAL audio playback stream
- `[mod.rs:AudioEngine::pause]` - Pauses the CPAL audio playback stream
- `[clock.rs:MasterClock]` - Thread-safe presentation clock synchronizing audio and video pipelines
- `[clock.rs:MasterClock::new]` - Constructor with target sample rate
- `[clock.rs:MasterClock::current_time]` - Returns current playback position in seconds
- `[clock.rs:MasterClock::current_pts]` - Returns presentation timestamp (PTS) in seconds
- `[clock.rs:MasterClock::sample_rate]` - Returns configured audio sample rate
- `[clock.rs:MasterClock::set_has_audio]` - Configures clock mode (audio-driven vs monotonic wall clock)
- `[clock.rs:MasterClock::has_audio]` - Checks if media contains an audio stream
- `[clock.rs:MasterClock::is_playing]` - Checks if clock is actively running
- `[clock.rs:MasterClock::playback_generation]` - Returns current generation counter for audio buffer flushes
- `[clock.rs:MasterClock::inc_playback_generation]` - Increments generation counter to invalidate stale buffered samples
- `[clock.rs:MasterClock::play]` - Starts or resumes clock progression
- `[clock.rs:MasterClock::pause]` - Pauses clock progression
- `[clock.rs:MasterClock::set_position]` - Sets clock position without incrementing generation
- `[clock.rs:MasterClock::reset]` - Sets position, increments generation counter, and pauses for sync
- `[clock.rs:MasterClock::is_active]` - Checks if clock is active
- `[clock.rs:MasterClock::advance]` - Advances clock position based on samples written and buffer tags
- `[clock.rs:MasterClock::stall_for_catchup]` - Temporarily halts clock progression for video catchup
- `[clock.rs:MasterClock::resume_from_catchup]` - Resumes clock progression after video catchup

## Invariants
- `[mod.rs:AudioEngine::new]` - Real-time CPAL callback discards stale audio samples across ring buffer boundaries whenever `playback_generation` changes
- `[clock.rs:MasterClock::current_pts]` - Derives presentation timestamp from audio sample count and tags when audio is available; falls back to monotonic wall-clock elapsed time when `has_audio` is false
- `[clock.rs:MasterClock::reset]` - Resets position, increments `playback_generation`, and pauses audio for sync until playback resumes

## Side Effects
- `[mod.rs:AudioEngine::new]` - Acquires and starts the default OS audio output device stream using CPAL
- `[clock.rs:MasterClock]` - Performs atomic updates (`AtomicU64`, `AtomicBool`) across the audio rendering thread and main GUI thread

## Verification
- Run: `cargo test`
