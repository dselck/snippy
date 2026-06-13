//! Headless decode throughput profiler.
//!
//! Measures how fast the `MediaEngine` can produce decoded frames across
//! four scenarios: burst decode, sustained decode, decode-after-seek, and
//! GOP boundary spike detection.
//!
//! Usage:
//!   SNIPPY_BENCH_VIDEO=path/to/clip.mp4 cargo run --release --bin bench_decode

use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
use snippy2::media::{DecodedFrame, MediaCommand};
use std::time::{Duration, Instant};

// ── Helpers ────────────────────────────────────────────────────────────────

/// Seek to frame 0 and drain stale frames so the rig is in a clean state.
fn reset_to_start(rig: &MediaTestRig) -> Result<(), anyhow::Error> {
    rig.send(MediaCommand::Pause)?;
    rig.master_clock.pause();
    rig.timed_seek(0, false, 0, Duration::from_secs(5));
    rig.drain_frames();
    Ok(())
}

/// Begin playback from the current position.
fn start_playback(rig: &MediaTestRig, start_secs: f64) -> Result<(), anyhow::Error> {
    rig.master_clock.set_position(start_secs);
    rig.send(MediaCommand::Play)?;
    rig.master_clock.play();
    Ok(())
}

/// Collect frames for `duration`, recording inter-frame intervals.
/// Returns `(total_frames, inter_frame_stats)`.
fn collect_frames_for(
    rig: &MediaTestRig,
    duration: Duration,
) -> (Vec<DecodedFrame>, Stats) {
    let mut frames: Vec<DecodedFrame> = Vec::new();
    let mut intervals = Stats::new();
    let deadline = Instant::now() + duration;
    let mut last_recv = Instant::now();

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let timeout = remaining.min(Duration::from_millis(100));
        if let Some(frame) = rig.recv_frame(timeout) {
            let now = Instant::now();
            if !frames.is_empty() {
                intervals.push(now - last_recv);
            }
            last_recv = now;
            frames.push(frame);
        }
    }
    (frames, intervals)
}

// ── Scenarios ──────────────────────────────────────────────────────────────

/// Scenario 1: Burst decode — measure peak throughput over a short window.
fn burst_decode(
    rig: &MediaTestRig,
    report: &mut BenchReport,
    profile: bool,
) -> Result<(), anyhow::Error> {
    println!("\n▸ Scenario 1: Burst decode (5s)");

    let _guard = if profile {
        Some(snippy2::bench::ProfilingGuard::new("decode_burst", 100))
    } else {
        None
    };

    reset_to_start(rig)?;
    start_playback(rig, 0.0)?;

    let wall_start = Instant::now();
    let (frames, intervals) = collect_frames_for(rig, Duration::from_secs(5));
    let elapsed = wall_start.elapsed();

    rig.send(MediaCommand::Pause)?;
    rig.master_clock.pause();

    drop(_guard);

    let fps = frames.len() as f64 / elapsed.as_secs_f64();
    println!(
        "  {} frames in {:.2}s → {:.1} FPS",
        frames.len(),
        elapsed.as_secs_f64(),
        fps
    );

    if let Some(median) = intervals.median() {
        println!(
            "  Inter-frame: median {}, p99 {}, max {}",
            Stats::fmt_duration(median),
            Stats::fmt_duration(intervals.percentile(99.0).unwrap()),
            Stats::fmt_duration(intervals.max().unwrap()),
        );
    }

    report.add("Burst decode (5s) intervals", intervals);
    Ok(())
}

/// Scenario 2: Sustained decode — detect thermal throttle over a longer window.
fn sustained_decode(
    rig: &MediaTestRig,
    report: &mut BenchReport,
    profile: bool,
) -> Result<(), anyhow::Error> {
    println!("\n▸ Scenario 2: Sustained decode (15s)");

    let _guard = if profile {
        Some(snippy2::bench::ProfilingGuard::new("decode_sustained", 100))
    } else {
        None
    };

    reset_to_start(rig)?;
    start_playback(rig, 0.0)?;

    let wall_start = Instant::now();
    let (frames, intervals) = collect_frames_for(rig, Duration::from_secs(15));
    let elapsed = wall_start.elapsed();

    rig.send(MediaCommand::Pause)?;
    rig.master_clock.pause();

    drop(_guard);

    let fps = frames.len() as f64 / elapsed.as_secs_f64();
    println!(
        "  {} frames in {:.2}s → {:.1} FPS",
        frames.len(),
        elapsed.as_secs_f64(),
        fps
    );

    if let Some(median) = intervals.median() {
        println!(
            "  Inter-frame: median {}, p99 {}, max {}",
            Stats::fmt_duration(median),
            Stats::fmt_duration(intervals.percentile(99.0).unwrap()),
            Stats::fmt_duration(intervals.max().unwrap()),
        );
    }

    report.add("Sustained decode (15s) intervals", intervals);
    Ok(())
}

/// Scenario 3: Decode latency after seeking to various positions.
fn decode_after_seek(
    rig: &MediaTestRig,
    report: &mut BenchReport,
    profile: bool,
) -> Result<(), anyhow::Error> {
    const FRAMES_TO_COLLECT: usize = 60;
    const POSITION_FRACTIONS: [f64; 5] = [0.0, 0.2, 0.4, 0.6, 0.8];

    println!("\n▸ Scenario 3: Decode after seek (60 frames × 5 positions)");

    let total = rig.metadata.total_frames;
    let mut stats = Stats::new();

    let _guard = if profile {
        Some(snippy2::bench::ProfilingGuard::new("decode_after_seek", 100))
    } else {
        None
    };

    for (i, &frac) in POSITION_FRACTIONS.iter().enumerate() {
        let target_frame = ((total as f64) * frac) as usize;
        let target_secs = target_frame as f64 / rig.metadata.fps;

        // Seek to position
        rig.send(MediaCommand::Pause)?;
        rig.master_clock.pause();
        let seq_id = 100 + i;
        rig.timed_seek(target_frame, false, seq_id, Duration::from_secs(5));
        rig.drain_frames();

        // Start playback and time how long until we get 60 frames
        start_playback(rig, target_secs)?;
        let t0 = Instant::now();

        let mut count = 0usize;
        let deadline = t0 + Duration::from_secs(10);
        while count < FRAMES_TO_COLLECT && Instant::now() < deadline {
            if rig.recv_frame(Duration::from_millis(100)).is_some() {
                count += 1;
            }
        }
        let elapsed = t0.elapsed();

        rig.send(MediaCommand::Pause)?;
        rig.master_clock.pause();

        println!(
            "  Position {:.0}% (frame {target_frame}): {count} frames in {}",
            frac * 100.0,
            Stats::fmt_duration(elapsed),
        );
        stats.push(elapsed);
    }

    drop(_guard);

    report.add("Decode-after-seek (60 frames)", stats);
    Ok(())
}

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
    start_playback(rig, 0.0)?;

    let (frames, intervals) = collect_frames_for(rig, Duration::from_secs(3));

    rig.send(MediaCommand::Pause)?;
    rig.master_clock.pause();

    drop(_guard);

    // Identify spikes: frames whose inter-frame time exceeds 2× the median.
    let median = match intervals.median() {
        Some(m) => m,
        None => {
            println!("  Not enough frames to analyze spikes.");
            report.add("GOP boundary intervals", intervals);
            return Ok(());
        }
    };
    let threshold = median * 2;

    // intervals[i] corresponds to the gap before frames[i+1] (since the first
    // frame has no preceding interval).
    let mut spike_count = 0usize;
    let mut spike_locations: Vec<(usize, Duration)> = Vec::new();

    for (i, sample) in intervals.samples.iter().enumerate() {
        if *sample > threshold {
            // The frame that was slow to arrive is frames[i+1].
            let frame_idx = frames[i + 1].frame_idx();
            spike_locations.push((frame_idx, *sample));
            spike_count += 1;
        }
    }

    println!(
        "  {} frames, median inter-frame {}, threshold (2×) {}",
        frames.len(),
        Stats::fmt_duration(median),
        Stats::fmt_duration(threshold),
    );
    println!(
        "  {} spikes detected (>{}):",
        spike_count,
        Stats::fmt_duration(threshold),
    );
    for (frame_idx, dur) in &spike_locations {
        println!("    frame {frame_idx}: {}", Stats::fmt_duration(*dur));
    }

    report.add("GOP boundary intervals", intervals);
    Ok(())
}

// ── Main ───────────────────────────────────────────────────────────────────

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

    // Final report
    report.print_table();
    report.write_json("decode")?;

    // Clean shutdown
    let _ = rig.send(MediaCommand::Shutdown);

    Ok(())
}
