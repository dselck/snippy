# Profiling Harness Design Specification

Establish a programmatic CPU profiling harness in the Snippy2 performance benchmark suite using `pprof-rs`. This harness enables direct, user-space collection of flamegraphs on macOS/Linux without root/sudo permissions, allowing us to pinpoint the exact code paths causing GOP boundary decode spikes and precise seek scaling bottlenecks.

## Proposed Changes

### 1. Master Clock Position Sync (Prerequisite Bug Fix)

#### [MODIFY] [mod.rs](file:///Users/dselck/Documents/Snippy2/src/bench/mod.rs)
- Modify `MediaTestRig::timed_seek` to position the master clock before sending the seek command:
  ```rust
  self.master_clock.set_position(frame_index as f64 / self.metadata.fps);
  ```
- This ensures the `VideoSyncEngine` recognizes the incoming frames as on-time/late rather than early, avoiding infinite wait loops and harness timeouts.

### 2. Dependency Configuration

#### [MODIFY] [Cargo.toml](file:///Users/dselck/Documents/Snippy2/Cargo.toml)
- Add target-specific dependency configuration for `pprof`:
  ```toml
  [target.'cfg(unix)'.dependencies]
  pprof = { version = "0.13.0", features = ["flamegraph"] }
  ```

### 3. Profiling Library Helper

#### [MODIFY] [mod.rs](file:///Users/dselck/Documents/Snippy2/src/bench/mod.rs)
- Introduce `ProfilingGuard` which:
  - Initializes `pprof::ProfilerGuard` on Unix platforms.
  - Automatically writes a SVG flamegraph to `target/profile/<name>_flamegraph.svg` when dropped.
  - Safe-gates compilation for non-Unix platforms via conditional compilation (`#[cfg(unix)]`).

### 4. Benchmark CLI Integration

#### [MODIFY] [bench_seek.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_seek.rs)
- Detect `--profile` command-line argument.
- If present, wrap each scenario loop (e.g., Cold Keyframe Seek, Warm GOP Seek, Precise Seek, Random Seek Storm, Sequential Fast-path) with `ProfilingGuard`.

#### [MODIFY] [bench_decode.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_decode.rs)
- Detect `--profile` command-line argument.
- Wrap throughput scenarios (e.g., Burst Decode, Sustained Decode, GOP Boundary Spikes) with `ProfilingGuard`.

#### [MODIFY] [bench_scrub.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_scrub.rs)
- Detect `--profile` command-line argument.
- Wrap scrub simulation scenarios (e.g., Slow Scrub, Fast Scrub, Jittery/Jump Scrub) with `ProfilingGuard`.

## Verification Plan

### Automated Execution
- Run each benchmark with the `--profile` flag and verify that flamegraphs are generated:
  ```bash
  cargo run --release --bin bench_seek -- --profile
  cargo run --release --bin bench_decode -- --profile
  cargo run --release --bin bench_scrub -- --profile
  ```

### Manual Verification
- Confirm that `target/profile/` contains the generated `.svg` flamegraphs (e.g., `seek_cold_flamegraph.svg`).
- Open a flamegraph in a browser or viewer to verify call stack details trace into `ffmpeg` and the Snippy2 `decoder_pipeline`.
