# src/bin

## Overview
Headless benchmark executables and test drivers for the media engine, decode throughput, seek latency, audio/video playback synchronization, and smart export.

## Interface
- `[bench_arrow_seek.rs:main]` - Benchmarks arrow-key stepping and hold responsiveness under simulated VSync ticks
- `[bench_decode.rs:main]` - Profiles decode throughput across burst, sustained, after-seek, and GOP boundary scenarios
- `[bench_playback_jitter.rs:main]` - Measures frame presentation arrival intervals and timing jitter relative to video frame rate
- `[bench_scrub.rs:main]` - Simulates rapid scrub interactions under the one-at-a-time seek protocol and reports latency/frame lag
- `[bench_seek.rs:main]` - Measures seek latency across cold keyframe, warm GOP, random seek storm, and sequential fast-path
- `[run_export_goat.rs:main]` - Standalone runner testing smart cut export and GOP boundary re-encoding on a test clip
- `[test_playback_sync.rs:main]` - Standalone integration driver testing multi-threaded video decoding, audio ring buffers, and master clock synchronization

## Invariants
- `[bench_arrow_seek.rs:SEEK_TIMEOUT]` - Enforces a 5-second maximum frame delivery deadline during seek operations
- `[bench_decode.rs:reset_to_start]` - Pauses playback, seeks to frame 0, and drains all stale frames before executing throughput benchmarks
- `[bench_scrub.rs:simulate_scrub]` - Dispatches next seek command only after the previous frame response arrives, matching the UI's one-in-flight seek protocol
- `[bench_seek.rs:main]` - Supports `--profile` flag to generate CPU flamegraphs in `target/profile`

## Side Effects
- `[bench_*.rs:main]` - Reads media files from disk, generates benchmark JSON files in `target/bench`, and prints performance tables to stdout
- `[run_export_goat.rs:main]` - Writes exported MP4 video files to disk using smart cut encoding
- `[test_playback_sync.rs:main]` - Decodes media frames and writes audio samples to real-time ring buffers

## Verification
- Run: `cargo test`
