# Programmatic CPU Profiling Harness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish a user-space CPU profiling harness integrated into the benchmark suite to generate targeted SVG flamegraphs of the video decoder pipeline, sync engine, and cache under active workloads.

**Architecture:** We will integrate `pprof` under Unix target dependencies in Cargo.toml. We will implement a `ProfilingGuard` helper in `src/bench/mod.rs` which starts profiling on instantiating and saves SVG flamegraphs when dropped. We will add a `--profile` flag to the benchmark binaries to conditionally wrap key workload phases with this guard.

**Tech Stack:** Rust, `pprof-rs` with `flamegraph` feature enabled.

---

### Task 1: Fix Seek Timeout Bug

**Files:**
- Modify: [src/bench/mod.rs](file:///Users/dselck/Documents/Snippy2/src/bench/mod.rs)

- [ ] **Step 1: Update `timed_seek` to position the master clock**
  Update `timed_seek` around lines 114-124 to sync the clock with the seek target.

  Target content:
  ```rust
      pub fn timed_seek(
          &self,
          frame_index: usize,
          keyframe_only: bool,
          sequence_id: usize,
          timeout: Duration,
      ) -> Option<(DecodedFrame, Duration)> {
          // Drain any stale frames first
          self.drain_frames();

          let start = Instant::now();
          let _ = self.send(MediaCommand::Seek {
              frame_index,
              keyframe_only,
              sequence_id,
              audio_enabled: false,
          });
  ```

  Replacement content:
  ```rust
      pub fn timed_seek(
          &self,
          frame_index: usize,
          keyframe_only: bool,
          sequence_id: usize,
          timeout: Duration,
      ) -> Option<(DecodedFrame, Duration)> {
          // Drain any stale frames first
          self.drain_frames();

          // Sync the master clock time with the seek target so the VideoSyncEngine
          // doesn't loop forever waiting for early frames.
          self.master_clock.set_position(frame_index as f64 / self.metadata.fps);

          let start = Instant::now();
          let _ = self.send(MediaCommand::Seek {
              frame_index,
              keyframe_only,
              sequence_id,
              audio_enabled: false,
          });
  ```

- [ ] **Step 2: Run Cargo Check**
  Verify the changes compile cleanly.
  Run: `cargo check`
  Expected: PASS with no compilation errors.

- [ ] **Step 3: Run Seek Benchmark to verify fix**
  Run the seek benchmark using synthetic video to make sure seeking doesn't time out.
  Run: `cargo run --release --bin bench_seek`
  Expected: Execution completes successfully without "WARN: seek to keyframe ... timed out" messages.

- [ ] **Step 4: Commit**
  Run:
  ```bash
  git add src/bench/mod.rs
  git commit -m "fix(bench): align MasterClock position before seek to prevent sync timeouts"
  ```

---

### Task 2: Configure target-specific dependency for `pprof`

**Files:**
- Modify: [Cargo.toml](file:///Users/dselck/Documents/Snippy2/Cargo.toml)

- [ ] **Step 1: Add target dependency to Cargo.toml**
  Append `pprof` under a target configuration block at the end of the file.

  Target content (bottom of file):
  ```toml
  [profile.dev.package."*"]
  opt-level = 3
  ```

  Replacement content:
  ```toml
  [profile.dev.package."*"]
  opt-level = 3

  [target.'cfg(unix)'.dependencies]
  pprof = { version = "0.13.0", features = ["flamegraph"] }
  ```

- [ ] **Step 2: Run Cargo Check**
  Run: `cargo check`
  Expected: Cargo successfully downloads/resolves and compiles dependencies.

- [ ] **Step 3: Commit**
  Run:
  ```bash
  git add Cargo.toml
  git commit -m "deps: add target-specific pprof dependency for unix platforms"
  ```

---

### Task 3: Implement `ProfilingGuard` in `src/bench/mod.rs`

**Files:**
- Modify: [src/bench/mod.rs](file:///Users/dselck/Documents/Snippy2/src/bench/mod.rs)

- [ ] **Step 1: Add ProfilingGuard implementation at the bottom of the file**
  Implement the struct and helper logic using conditional compilation gates.

  Target content (bottom of file):
  ```rust
  /// Format video metadata into a concise info string.
  pub fn format_video_info(meta: &VideoMetadata) -> String {
      format!(
          "{}x{} {:.1}fps • {} frames • {} keyframes",
          meta.width,
          meta.height,
          meta.fps,
          meta.total_frames,
          meta.keyframes.len()
      )
  }
  ```

  Replacement content:
  ```rust
  /// Format video metadata into a concise info string.
  pub fn format_video_info(meta: &VideoMetadata) -> String {
      format!(
          "{}x{} {:.1}fps • {} frames • {} keyframes",
          meta.width,
          meta.height,
          meta.fps,
          meta.total_frames,
          meta.keyframes.len()
      )
  }

  // ── Profiling Infrastructure ───────────────────────────────────────────────

  #[cfg(unix)]
  use pprof::ProfilerGuard;

  pub struct ProfilingGuard {
      #[cfg(unix)]
      guard: Option<ProfilerGuard<'static>>,
      #[allow(dead_code)]
      name: String,
  }

  impl ProfilingGuard {
      pub fn new(name: &str, frequency: i32) -> Self {
          #[cfg(unix)]
          {
              match ProfilerGuard::new(frequency) {
                Ok(g) => Self {
                    guard: Some(g),
                    name: name.to_string(),
                },
                Err(e) => {
                    eprintln!("  [profiler] Warn: Failed to start profiler: {:?}", e);
                    Self {
                        guard: None,
                        name: name.to_string(),
                    }
                }
              }
          }
          #[cfg(not(unix))]
          {
              Self {
                  name: name.to_string(),
              }
          }
      }
  }

  impl Drop for ProfilingGuard {
      fn drop(&mut self) {
          #[cfg(unix)]
          if let Some(guard) = self.guard.take() {
              if let Ok(report) = guard.report().build() {
                  let dir = std::path::Path::new("target/profile");
                  let _ = std::fs::create_dir_all(dir);
                  let file_name = format!("{}_flamegraph.svg", self.name);
                  let file_path = dir.join(file_name);
                  if let Ok(file) = std::fs::File::create(&file_path) {
                      if let Err(e) = report.flamegraph(file) {
                          eprintln!("  [profiler] Warn: Failed to generate flamegraph: {:?}", e);
                      } else {
                          println!("  [profiler] Flamegraph saved to: {}", file_path.display());
                      }
                  }
              }
          }
      }
  }
  ```

- [ ] **Step 2: Run Cargo Check**
  Verify everything compiles.
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  Run:
  ```bash
  git add src/bench/mod.rs
  git commit -m "feat(bench): add ProfilingGuard helper for programmatic flamegraphs"
  ```

---

### Task 4: Integrate Profiling into `bench_seek`

**Files:**
- Modify: [src/bin/bench_seek.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_seek.rs)

- [ ] **Step 1: Check for `--profile` argument and wrap cold keyframe seeks**
  Wrap Cold Keyframe Seek scenario with a targeted `ProfilingGuard`.

  Target content:
  ```rust
      // ── Scenario 1: Keyframe seek (cold) ────────────────────────────────

      println!("\n--- Scenario: Keyframe seek (cold) ---");
      let mut cold_stats = Stats::new();

      let sampled_keyframes = sample_keyframes(keyframes, 100);
  ```

  Replacement content:
  ```rust
      let profile = std::env::args().any(|arg| arg == "--profile");

      // ── Scenario 1: Keyframe seek (cold) ────────────────────────────────

      println!("\n--- Scenario: Keyframe seek (cold) ---");
      let mut cold_stats = Stats::new();

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("seek_cold", 100))
      } else {
          None
      };

      let sampled_keyframes = sample_keyframes(keyframes, 100);
  ```

- [ ] **Step 2: Wrap warm GOP seek scenario**
  Wrap Warm GOP Seek with `ProfilingGuard`.

  Target content:
  ```rust
      // ── Scenario 2: Keyframe seek (warm GOP) ────────────────────────────

      println!("\n--- Scenario: Keyframe seek (warm GOP) ---");
      let mut warm_stats = Stats::new();

      if let Some(&target_kf) = keyframes.get(keyframes.len() / 2) {
  ```

  Replacement content:
  ```rust
      // ── Scenario 2: Keyframe seek (warm GOP) ────────────────────────────

      println!("\n--- Scenario: Keyframe seek (warm GOP) ---");
      let mut warm_stats = Stats::new();

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("seek_warm", 100))
      } else {
          None
      };

      if let Some(&target_kf) = keyframes.get(keyframes.len() / 2) {
  ```

- [ ] **Step 3: Wrap precise seek scenario**
  Wrap Precise Seek with `ProfilingGuard`.

  Target content:
  ```rust
      // ── Scenario 3: Precise seek (varying distance) ─────────────────────

      println!("\n--- Scenario: Precise seek (varying distance) ---");
      let distances = [1, 5, 15, 30, 60];
  ```

  Replacement content:
  ```rust
      // ── Scenario 3: Precise seek (varying distance) ─────────────────────

      println!("\n--- Scenario: Precise seek (varying distance) ---");
      let distances = [1, 5, 15, 30, 60];

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("seek_precise", 100))
      } else {
          None
      };
  ```

- [ ] **Step 4: Wrap random seek storm scenario**
  Wrap Random Seek Storm with `ProfilingGuard`.

  Target content:
  ```rust
      // ── Scenario 4: Random seek storm ───────────────────────────────────

      println!("\n--- Scenario: Random seek storm ---");
      let mut storm_stats = Stats::new();
  ```

  Replacement content:
  ```rust
      // ── Scenario 4: Random seek storm ───────────────────────────────────

      println!("\n--- Scenario: Random seek storm ---");
      let mut storm_stats = Stats::new();

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("seek_storm", 100))
      } else {
          None
      };
  ```

- [ ] **Step 5: Wrap sequential fast-path scenario**
  Wrap Sequential Fast-path with `ProfilingGuard`.

  Target content:
  ```rust
      // ── Scenario 5: Sequential fast-path ────────────────────────────────

      println!("\n--- Scenario: Sequential fast-path ---");
      let mut seq_stats = Stats::new();
  ```

  Replacement content:
  ```rust
      // ── Scenario 5: Sequential fast-path ────────────────────────────────

      println!("\n--- Scenario: Sequential fast-path ---");
      let mut seq_stats = Stats::new();

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("seek_sequential", 100))
      } else {
          None
      };
  ```

- [ ] **Step 6: Run Cargo Check**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 7: Commit**
  Run:
  ```bash
  git add src/bin/bench_seek.rs
  git commit -m "feat(bench): integrate ProfilingGuard in bench_seek scenarios"
  ```

---

### Task 5: Integrate Profiling into `bench_decode`

**Files:**
- Modify: [src/bin/bench_decode.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_decode.rs)

- [ ] **Step 1: Check for `--profile` argument and pass it to scenarios**
  Modify scenario signatures to accept `profile: bool`.

  Target content (main function):
  ```rust
  fn main() -> Result<(), anyhow::Error> {
      let video_path = TestMediaLocator::resolve()?;
      let rig = MediaTestRig::new(&video_path)?;

      let info = format_video_info(&rig.metadata);
      println!("[bench_decode] {info}");

      let mut report = BenchReport::new("Decode Throughput", &info);

      burst_decode(&rig, &mut report)?;
      sustained_decode(&rig, &mut report)?;
      decode_after_seek(&rig, &mut report)?;
      gop_boundary_spikes(&rig, &mut report)?;
  ```

  Replacement content:
  ```rust
  fn main() -> Result<(), anyhow::Error> {
      let profile = std::env::args().any(|arg| arg == "--profile");
      let video_path = TestMediaLocator::resolve()?;
      let rig = MediaTestRig::new(&video_path)?;

      let info = format_video_info(&rig.metadata);
      println!("[bench_decode] {info}");

      let mut report = BenchReport::new("Decode Throughput", &info);

      burst_decode(&rig, &mut report, profile)?;
      sustained_decode(&rig, &mut report, profile)?;
      decode_after_seek(&rig, &mut report, profile)?;
      gop_boundary_spikes(&rig, &mut report, profile)?;
  ```

- [ ] **Step 2: Update scenario signatures and add profiling guards**
  Modify all four scenario functions in `bench_decode.rs` to accept `profile: bool` and use `ProfilingGuard`.

  Target content (burst/sustained decode signatures):
  ```rust
  /// Scenario 1: Burst decode — measure peak throughput over a short window.
  fn burst_decode(rig: &MediaTestRig, report: &mut BenchReport) -> Result<(), anyhow::Error> {
      println!("\n▸ Scenario 1: Burst decode (5s)");

      reset_to_start(rig)?;
  ```

  Replacement content:
  ```rust
  /// Scenario 1: Burst decode — measure peak throughput over a short window.
  fn burst_decode(rig: &MediaTestRig, report: &mut BenchReport, profile: bool) -> Result<(), anyhow::Error> {
      println!("\n▸ Scenario 1: Burst decode (5s)");

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("decode_burst", 100))
      } else {
          None
      };

      reset_to_start(rig)?;
  ```

  Target content (sustained decode signature):
  ```rust
  /// Scenario 2: Sustained decode — detect thermal throttle over a longer window.
  fn sustained_decode(rig: &MediaTestRig, report: &mut BenchReport) -> Result<(), anyhow::Error> {
      println!("\n▸ Scenario 2: Sustained decode (15s)");

      reset_to_start(rig)?;
  ```

  Replacement content:
  ```rust
  /// Scenario 2: Sustained decode — detect thermal throttle over a longer window.
  fn sustained_decode(rig: &MediaTestRig, report: &mut BenchReport, profile: bool) -> Result<(), anyhow::Error> {
      println!("\n▸ Scenario 2: Sustained decode (15s)");

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("decode_sustained", 100))
      } else {
          None
      };

      reset_to_start(rig)?;
  ```

  Target content (decode after seek signature):
  ```rust
  /// Scenario 3: Decode latency after seeking to various positions.
  fn decode_after_seek(rig: &MediaTestRig, report: &mut BenchReport) -> Result<(), anyhow::Error> {
      const FRAMES_TO_COLLECT: usize = 60;
  ```

  Replacement content:
  ```rust
  /// Scenario 3: Decode latency after seeking to various positions.
  fn decode_after_seek(rig: &MediaTestRig, report: &mut BenchReport, profile: bool) -> Result<(), anyhow::Error> {
      const FRAMES_TO_COLLECT: usize = 60;

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("decode_after_seek", 100))
      } else {
          None
      };
  ```

  Target content (gop boundary signature):
  ```rust
  /// Scenario 4: Detect GOP boundary spikes in inter-frame decode time.
  fn gop_boundary_spikes(
      rig: &MediaTestRig,
      report: &mut BenchReport,
  ) -> Result<(), anyhow::Error> {
      println!("\n▸ Scenario 4: GOP boundary spikes (3s)");

      reset_to_start(rig)?;
  ```

  Replacement content:
  ```rust
  /// Scenario 4: Detect GOP boundary spikes in inter-frame decode time.
  fn gop_boundary_spikes(
      rig: &MediaTestRig,
      report: &mut BenchReport,
      profile: bool,
  ) -> Result<(), anyhow::Error> {
      println!("\n▸ Scenario 4: GOP boundary spikes (3s)");

      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new("decode_gop_spikes", 100))
      } else {
          None
      };

      reset_to_start(rig)?;
  ```

- [ ] **Step 3: Run Cargo Check**
  Verify compile correctness.
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  Run:
  ```bash
  git add src/bin/bench_decode.rs
  git commit -m "feat(bench): integrate ProfilingGuard in bench_decode scenarios"
  ```

---

### Task 6: Integrate Profiling into `bench_scrub`

**Files:**
- Modify: [src/bin/bench_scrub.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_scrub.rs)

- [ ] **Step 1: Check for `--profile` argument and pass it to `run_scenario`**
  Modify scenario signature to accept `profile: bool`.

  Target content (main loop):
  ```rust
      for scenario in &scenarios {
          run_scenario(&rig, scenario, &mut seq, &mut report);
      }
  ```

  Replacement content:
  ```rust
      let profile = std::env::args().any(|arg| arg == "--profile");

      for scenario in &scenarios {
          run_scenario(&rig, scenario, &mut seq, &mut report, profile);
      }
  ```

- [ ] **Step 2: Update `run_scenario` signature and add profiling guards**
  Implement the profiling wrapper inside `run_scenario`.

  Target content:
  ```rust
  fn run_scenario(
      rig: &MediaTestRig,
      scenario: &Scenario,
      seq: &mut usize,
      report: &mut BenchReport,
  ) {
      println!("  ▸ {} ({} positions)", scenario.name, scenario.positions.len());
  ```

  Replacement content:
  ```rust
  fn run_scenario(
      rig: &MediaTestRig,
      scenario: &Scenario,
      seq: &mut usize,
      report: &mut BenchReport,
      profile: bool,
  ) {
      println!("  ▸ {} ({} positions)", scenario.name, scenario.positions.len());

      let sanitize_name = scenario.name.to_lowercase().replace(' ', "_");
      let _guard = if profile {
          Some(snippy2::bench::ProfilingGuard::new(&format!("scrub_{sanitize_name}"), 100))
      } else {
          None
      };
  ```

- [ ] **Step 3: Run Cargo Check**
  Verify compilation.
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  Run:
  ```bash
  git add src/bin/bench_scrub.rs
  git commit -m "feat(bench): integrate ProfilingGuard in bench_scrub scenarios"
  ```

---

### Task 7: Run profiling and verify output

- [ ] **Step 1: Run the seek profiling workload**
  Generate flamegraphs for seeking behavior against the real video file.
  Run: `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv" cargo run --release --bin bench_seek -- --profile`
  Expected: Completes successfully and outputs flamegraph SVG paths.

- [ ] **Step 2: Run the decode profiling workload**
  Generate flamegraphs for decoding.
  Run: `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv" cargo run --release --bin bench_decode -- --profile`
  Expected: Completes successfully and outputs flamegraph SVG paths.

- [ ] **Step 3: Run the scrub profiling workload**
  Generate flamegraphs for scrubbing.
  Run: `SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv" cargo run --release --bin bench_scrub -- --profile`
  Expected: Completes successfully and outputs flamegraph SVG paths.

- [ ] **Step 4: List the generated flamegraphs**
  Verify that all flamegraphs exist in `target/profile/`.
  Run: `ls -la target/profile`
  Expected: Contains files like `seek_cold_flamegraph.svg`, `decode_burst_flamegraph.svg`, `scrub_slow_fwd_flamegraph.svg`, etc.
