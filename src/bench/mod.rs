//! Shared infrastructure for headless performance benchmarking.
//!
//! Provides `MediaTestRig` for driving the MediaEngine without GPU or audio
//! hardware, `Stats` for timing analysis, `BenchReport` for dual console/JSON
//! output, and `TestMediaLocator` for resolving test media files.

use crossbeam_channel::{bounded, unbounded, Receiver};
use ringbuf::{storage::Heap, traits::*, SharedRb};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

use crate::audio::clock::MasterClock;
use crate::media::{DecodedFrame, MediaCommand, MediaEngine, VideoMetadata};

// ── MediaTestRig ────────────────────────────────────────────────────────────

/// Headless wrapper around `MediaEngine` for benchmark use.
///
/// Creates all the plumbing (ring buffers, dummy audio consumer, standalone
/// MasterClock, crossbeam channels) so callers can simply `send()` commands
/// and `recv_frame()` responses.
pub struct MediaTestRig {
    pub engine: MediaEngine,
    pub metadata: VideoMetadata,
    pub frame_rx: Receiver<DecodedFrame>,
    pub master_clock: MasterClock,
    _audio_consumer: thread::JoinHandle<()>,
}

impl MediaTestRig {
    /// Open a video file and spin up the media engine headlessly.
    ///
    /// The `MasterClock` is configured in no-audio fallback mode so the
    /// `VideoSyncEngine` doesn't block waiting for real audio PTS.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, anyhow::Error> {
        let (frame_tx, frame_rx) = bounded::<DecodedFrame>(4);
        let (_cmd_tx, cmd_rx) = unbounded::<MediaCommand>();

        // Dummy audio ring buffer — never consumed by real hardware
        let rb = SharedRb::<Heap<(f32, u64)>>::new(48000 * 2 * 2);
        let (audio_tx, mut audio_cons) = rb.split();

        // Dummy tags ring buffer
        let tags_rb = SharedRb::<Heap<(u64, u64)>>::new(256);
        let (tags_tx, mut tags_cons) = tags_rb.split();

        // Drain audio so the media engine never blocks on backpressure
        let audio_consumer = thread::spawn(move || loop {
            let mut drained = false;
            while audio_cons.try_pop().is_some() {
                drained = true;
            }
            while tags_cons.try_pop().is_some() {
                drained = true;
            }
            if !drained {
                thread::sleep(Duration::from_millis(5));
            }
        });

        // Standalone clock — no-audio fallback mode
        let (master_clock, _ticker) = MasterClock::new(48000);
        master_clock.set_has_audio(false);

        let (engine, metadata) = MediaEngine::new(
            path,
            48000,
            1, // mono
            frame_tx,
            audio_tx,
            tags_tx,
            cmd_rx,
            master_clock.clone(),
        )?;

        Ok(Self {
            engine,
            metadata,
            frame_rx,
            master_clock,
            _audio_consumer: audio_consumer,
        })
    }

    /// Send a command to the media engine.
    pub fn send(&self, cmd: MediaCommand) -> Result<(), anyhow::Error> {
        self.engine.tx.send(cmd)?;
        Ok(())
    }

    /// Block until a frame arrives or timeout expires.
    pub fn recv_frame(&self, timeout: Duration) -> Option<DecodedFrame> {
        self.frame_rx.recv_timeout(timeout).ok()
    }

    /// Drain all currently-buffered frames, returning them.
    pub fn drain_frames(&self) -> Vec<DecodedFrame> {
        let mut frames = Vec::new();
        while let Ok(frame) = self.frame_rx.try_recv() {
            frames.push(frame);
        }
        frames
    }

    /// Seek to a frame and wait for the response, returning elapsed time.
    /// Uses a unique sequence_id so the response can be matched.
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
        self.master_clock.set_position(frame_index as f64 / self.metadata.fps);
        let _ = self.send(MediaCommand::Seek {
            frame_index,
            keyframe_only,
            sequence_id,
            audio_enabled: false,
        });

        // Wait for a frame with the matching sequence_id
        let deadline = start + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match self.frame_rx.recv_timeout(remaining) {
                Ok(frame) if frame.sequence_id() == sequence_id => {
                    return Some((frame, start.elapsed()));
                }
                Ok(_) => continue, // stale frame, keep waiting
                Err(_) => return None,
            }
        }
    }
}

// ── Stats ───────────────────────────────────────────────────────────────────

/// Accumulates timing samples and computes summary statistics.
#[derive(Clone, Debug)]
pub struct Stats {
    pub samples: Vec<Duration>,
}

impl Stats {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    pub fn push(&mut self, d: Duration) {
        self.samples.push(d);
    }

    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// Returns a sorted copy for percentile computation.
    fn sorted(&self) -> Vec<Duration> {
        let mut s = self.samples.clone();
        s.sort();
        s
    }

    pub fn min(&self) -> Option<Duration> {
        self.sorted().first().copied()
    }

    pub fn max(&self) -> Option<Duration> {
        self.sorted().last().copied()
    }

    pub fn median(&self) -> Option<Duration> {
        let s = self.sorted();
        if s.is_empty() {
            return None;
        }
        Some(s[s.len() / 2])
    }

    pub fn percentile(&self, p: f64) -> Option<Duration> {
        let s = self.sorted();
        if s.is_empty() {
            return None;
        }
        let idx = ((p / 100.0) * (s.len() - 1) as f64).round() as usize;
        Some(s[idx.min(s.len() - 1)])
    }

    pub fn mean(&self) -> Option<Duration> {
        if self.samples.is_empty() {
            return None;
        }
        let total: Duration = self.samples.iter().sum();
        Some(total / self.samples.len() as u32)
    }

    pub fn stddev(&self) -> Option<Duration> {
        let mean = self.mean()?.as_secs_f64();
        let variance = self.samples.iter()
            .map(|d| {
                let diff = d.as_secs_f64() - mean;
                diff * diff
            })
            .sum::<f64>()
            / self.samples.len() as f64;
        Some(Duration::from_secs_f64(variance.sqrt()))
    }

    /// Format a Duration as a human-readable string (e.g. "3.4ms", "1.23s").
    pub fn fmt_duration(d: Duration) -> String {
        let us = d.as_micros();
        if us < 1_000 {
            format!("{us}µs")
        } else if us < 1_000_000 {
            format!("{:.1}ms", us as f64 / 1_000.0)
        } else {
            format!("{:.2}s", us as f64 / 1_000_000.0)
        }
    }
}

// ── BenchReport ─────────────────────────────────────────────────────────────

/// Collects named stat groups and outputs console tables + JSON.
pub struct BenchReport {
    pub title: String,
    pub video_info: String,
    pub entries: Vec<(String, Stats)>,
}

impl BenchReport {
    pub fn new(title: &str, video_info: &str) -> Self {
        Self {
            title: title.to_string(),
            video_info: video_info.to_string(),
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, name: &str, stats: Stats) {
        self.entries.push((name.to_string(), stats));
    }

    /// Print a formatted console table.
    pub fn print_table(&self) {
        let now = chrono_ish_now();
        let bar = "═".repeat(75);
        println!("\n{bar}");
        println!("  {} — {}", self.title, self.video_info);
        println!("  {now}");
        println!("{bar}\n");
        println!(
            "  {:<30} {:>5}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}",
            "Scenario", "N", "Min", "Median", "P95", "P99", "Max"
        );
        println!("  {}", "─".repeat(73));

        for (name, stats) in &self.entries {
            if stats.count() == 0 {
                println!("  {:<30} {:>5}  (no samples)", name, 0);
                continue;
            }
            println!(
                "  {:<30} {:>5}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}",
                name,
                stats.count(),
                Stats::fmt_duration(stats.min().unwrap()),
                Stats::fmt_duration(stats.median().unwrap()),
                Stats::fmt_duration(stats.percentile(95.0).unwrap()),
                Stats::fmt_duration(stats.percentile(99.0).unwrap()),
                Stats::fmt_duration(stats.max().unwrap()),
            );
        }
        println!();
    }

    /// Write a JSON report to `target/bench/{prefix}_{timestamp}.json`.
    pub fn write_json(&self, prefix: &str) -> Result<PathBuf, anyhow::Error> {
        let dir = PathBuf::from("target/bench");
        fs::create_dir_all(&dir)?;

        let timestamp = chrono_ish_now().replace([':', ' '], "_");
        let filename = format!("{prefix}_{timestamp}.json");
        let path = dir.join(&filename);

        let entries_json: Vec<serde_json::Value> = self
            .entries
            .iter()
            .map(|(name, stats)| {
                serde_json::json!({
                    "scenario": name,
                    "count": stats.count(),
                    "min_us": stats.min().map(|d| d.as_micros()),
                    "median_us": stats.median().map(|d| d.as_micros()),
                    "mean_us": stats.mean().map(|d| d.as_micros()),
                    "p95_us": stats.percentile(95.0).map(|d| d.as_micros()),
                    "p99_us": stats.percentile(99.0).map(|d| d.as_micros()),
                    "max_us": stats.max().map(|d| d.as_micros()),
                    "stddev_us": stats.stddev().map(|d| d.as_micros()),
                    "samples_us": stats.samples.iter().map(|d| d.as_micros()).collect::<Vec<_>>(),
                })
            })
            .collect();

        let report = serde_json::json!({
            "title": self.title,
            "video_info": self.video_info,
            "timestamp": timestamp,
            "entries": entries_json,
        });

        let json_str = serde_json::to_string_pretty(&report)?;
        fs::write(&path, &json_str)?;

        println!("  JSON: {}\n", path.display());
        Ok(path)
    }
}

// ── TestMediaLocator ────────────────────────────────────────────────────────

/// Resolves a test video: uses `SNIPPY_BENCH_VIDEO` env var if set,
/// otherwise generates a synthetic 720p H.264 MP4 via `ffmpeg` CLI.
pub struct TestMediaLocator;

impl TestMediaLocator {
    /// Returns the path to a usable test video file.
    pub fn resolve() -> Result<PathBuf, anyhow::Error> {
        // 1. Check env var
        if let Ok(path) = env::var("SNIPPY_BENCH_VIDEO") {
            let p = PathBuf::from(&path);
            if p.exists() {
                println!("[bench] Using SNIPPY_BENCH_VIDEO: {}", p.display());
                return Ok(p);
            }
            anyhow::bail!(
                "SNIPPY_BENCH_VIDEO is set to '{}' but file does not exist",
                path
            );
        }

        // 2. Generate synthetic video
        let cache_dir = env::temp_dir().join("snippy_bench");
        fs::create_dir_all(&cache_dir)?;
        let output = cache_dir.join("synthetic_720p_30gop.mp4");

        if output.exists() {
            println!("[bench] Using cached synthetic video: {}", output.display());
            return Ok(output);
        }

        println!("[bench] Generating synthetic test video...");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f", "lavfi",
                "-i", "testsrc2=duration=5:size=1280x720:rate=24",
                "-f", "lavfi",
                "-i", "sine=frequency=440:duration=5:sample_rate=48000",
                "-c:v", "libx264",
                "-preset", "ultrafast",
                "-g", "30",         // keyframe interval = 30 frames
                "-keyint_min", "30",
                "-bf", "0",         // no B-frames for deterministic GOPs
                "-c:a", "aac",
                "-b:a", "128k",
                "-pix_fmt", "yuv420p",
                "-movflags", "+faststart",
            ])
            .arg(output.as_os_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .status()?;

        if !status.success() {
            anyhow::bail!(
                "ffmpeg failed to generate synthetic video (exit code {:?}). \
                 Make sure ffmpeg CLI is installed, or set SNIPPY_BENCH_VIDEO.",
                status.code()
            );
        }

        println!("[bench] Generated: {}", output.display());
        Ok(output)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Poor-man's timestamp without pulling in chrono.
fn chrono_ish_now() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    // Just use epoch seconds — good enough for filenames
    format!("{secs}")
}

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
