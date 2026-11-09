# src/bench

## Overview
Headless benchmarking and performance profiling harness for testing the media decoding pipeline, seek latency, statistical sample analysis, and flamegraph capture.

## Interface
- `[mod.rs:MediaTestRig]` - Headless test fixture wrapping `MediaEngine` and channels for automated benchmark scenarios
- `[mod.rs:MediaTestRig::new]` - Initializes test rig with a target media file path
- `[mod.rs:MediaTestRig::send]` - Dispatches a `MediaCommand` to the engine
- `[mod.rs:MediaTestRig::recv_frame]` - Receives the next decoded frame with a timeout
- `[mod.rs:MediaTestRig::drain_frames]` - Drains all pending frames from the decoded frame channel
- `[mod.rs:MediaTestRig::timed_seek]` - Executes a seek command and measures precise wall-clock latency until frame receipt
- `[mod.rs:Stats]` - Accumulator for benchmark latency and throughput measurements
- `[mod.rs:Stats::new]` - Initializes an empty statistics accumulator
- `[mod.rs:Stats::push]` - Appends a duration sample
- `[mod.rs:Stats::count]` - Returns total sample count
- `[mod.rs:Stats::min]` - Returns minimum observed duration
- `[mod.rs:Stats::max]` - Returns maximum observed duration
- `[mod.rs:Stats::median]` - Returns median observed duration
- `[mod.rs:Stats::percentile]` - Returns arbitrary percentile duration (e.g. p95, p99)
- `[mod.rs:Stats::mean]` - Returns mean duration
- `[mod.rs:Stats::stddev]` - Returns sample standard deviation
- `[mod.rs:Stats::fmt_duration]` - Pretty-prints duration in microseconds or milliseconds
- `[mod.rs:BenchReport]` - Summary reporter printing console tables and saving benchmark results
- `[mod.rs:BenchReport::new]` - Initializes report container with title and video details
- `[mod.rs:BenchReport::add]` - Adds named benchmark statistics section
- `[mod.rs:BenchReport::print_table]` - Formats and displays ASCII benchmark results table to stdout
- `[mod.rs:BenchReport::write_json]` - Exports benchmark report JSON to `target/bench/<prefix>-<timestamp>.json`
- `[mod.rs:TestMediaLocator]` - Resolves input test media files from environment or synthetic generators
- `[mod.rs:TestMediaLocator::resolve]` - Returns path from `SNIPPY_BENCH_VIDEO` or synthesizes a 720p 30-GOP test clip via FFmpeg
- `[mod.rs:format_video_info]` - Helper string formatter for video metadata
- `[mod.rs:ProfilingGuard]` - CPU flamegraph profiling guard using `pprof`
- `[mod.rs:ProfilingGuard::new]` - Starts sampling CPU profile guard for the given scope

## Invariants
- `[mod.rs:MediaTestRig::timed_seek]` - Flushes all stale frames from the channel prior to issuing seek command and validates returned frames against matching `sequence_id`
- `[mod.rs:TestMediaLocator::resolve]` - Relies on `SNIPPY_BENCH_VIDEO` environment variable if present; otherwise invokes ffmpeg CLI to produce a deterministic synthetic 720p 30-GOP test video

## Side Effects
- `[mod.rs:TestMediaLocator::resolve]` - Spawns child process ffmpeg to generate synthetic media on disk in `env::temp_dir()/snippy_bench`
- `[mod.rs:BenchReport::write_json]` - Creates directory `target/bench` and writes JSON benchmark output
- `[mod.rs:ProfilingGuard::drop]` - On Unix systems, collects CPU sample profiles and writes flamegraph SVG files to `target/profile/`

## Verification
- Run: `cargo test`
