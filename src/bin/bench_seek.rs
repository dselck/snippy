//! Headless seek latency profiler for the Snippy2 video editor.
//!
//! Measures wall-clock time from sending a seek command to receiving the
//! decoded frame across five scenarios: cold keyframe, warm GOP, precise
//! seek at varying distances, random seek storm, and sequential fast-path.

use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
use snippy2::media::MediaCommand;
use std::time::Duration;

const SEEK_TIMEOUT: Duration = Duration::from_secs(5);

fn main() -> Result<(), anyhow::Error> {
    let profile = std::env::args().any(|arg| arg == "--profile");
    let video_path = TestMediaLocator::resolve()?;
    let rig = MediaTestRig::new(&video_path)?;

    let video_info = format_video_info(&rig.metadata);
    println!("=== Seek Latency Benchmark ===");
    println!("  Video: {}", video_path.display());
    println!("  {video_info}");

    let keyframes = &rig.metadata.keyframes;
    let total_frames = rig.metadata.total_frames;
    let mut report = BenchReport::new("Seek Latency", &video_info);
    let mut seq: usize = 1;

    // ── Scenario 1: Keyframe seek (cold) ────────────────────────────────

    println!("\n--- Scenario: Keyframe seek (cold) ---");
    let mut cold_stats = Stats::new();
    {
        let _guard = if profile {
            Some(snippy2::bench::ProfilingGuard::new("seek_cold", 100))
        } else {
            None
        };

        let sampled_keyframes = sample_keyframes(keyframes, 100);
        for &kf in &sampled_keyframes {
            match rig.timed_seek(kf, true, seq, SEEK_TIMEOUT) {
                Some((_frame, elapsed)) => cold_stats.push(elapsed),
                None => eprintln!("  WARN: seek to keyframe {kf} timed out (seq={seq})"),
            }
            seq += 1;
        }
    }
    println!("  Collected {} samples", cold_stats.count());
    report.add("Keyframe seek (cold)", cold_stats);

    // ── Scenario 2: Keyframe seek (warm GOP) ────────────────────────────

    println!("\n--- Scenario: Keyframe seek (warm GOP) ---");
    let mut warm_stats = Stats::new();
    {
        let _guard = if profile {
            Some(snippy2::bench::ProfilingGuard::new("seek_warm", 100))
        } else {
            None
        };

        if let Some(&target_kf) = keyframes.get(keyframes.len() / 2) {
            // Cold seek to populate the GOP cache
            rig.timed_seek(target_kf, true, seq, SEEK_TIMEOUT);
            seq += 1;

            // Warm seeks — should hit the cache
            for _ in 0..10 {
                match rig.timed_seek(target_kf, true, seq, SEEK_TIMEOUT) {
                    Some((_frame, elapsed)) => warm_stats.push(elapsed),
                    None => eprintln!("  WARN: warm seek to keyframe {target_kf} timed out (seq={seq})"),
                }
                seq += 1;
            }
        }
    }
    println!("  Collected {} samples", warm_stats.count());
    report.add("Keyframe seek (warm GOP)", warm_stats);

    // ── Scenario 3: Precise seek (varying distance) ─────────────────────

    println!("\n--- Scenario: Precise seek (varying distance) ---");
    let distances = [1, 5, 15, 30, 60];
    {
        let _guard = if profile {
            Some(snippy2::bench::ProfilingGuard::new("seek_precise", 100))
        } else {
            None
        };

        for distance in distances {
            let mut stats = Stats::new();

            // Find a keyframe with enough room after it
            let anchor = keyframes
                .iter()
                .copied()
                .find(|&kf| kf + distance < total_frames);

            if let Some(kf) = anchor {
                for _ in 0..5 {
                    // Seek to the keyframe first to establish position
                    rig.timed_seek(kf, true, seq, SEEK_TIMEOUT);
                    seq += 1;

                    // Precise seek to keyframe + distance
                    let target = kf + distance;
                    match rig.timed_seek(target, false, seq, SEEK_TIMEOUT) {
                        Some((_frame, elapsed)) => stats.push(elapsed),
                        None => eprintln!("  WARN: precise seek to frame {target} timed out (seq={seq})"),
                    }
                    seq += 1;
                }
            } else {
                eprintln!("  WARN: no keyframe with {distance} frames of headroom, skipping");
            }

            let label = format!("Precise seek (+{distance})");
            println!("  {label}: {} samples", stats.count());
            report.add(&label, stats);
        }
    }

    // ── Scenario 4: Random seek storm ───────────────────────────────────

    println!("\n--- Scenario: Random seek storm ---");
    let mut storm_stats = Stats::new();
    {
        let _guard = if profile {
            Some(snippy2::bench::ProfilingGuard::new("seek_storm", 100))
        } else {
            None
        };

        for i in 0..100u64 {
            let target = ((i.wrapping_mul(7919)) % total_frames as u64) as usize;
            match rig.timed_seek(target, true, seq, SEEK_TIMEOUT) {
                Some((_frame, elapsed)) => storm_stats.push(elapsed),
                None => eprintln!("  WARN: random seek to frame {target} timed out (seq={seq})"),
            }
            seq += 1;
        }
    }
    println!("  Collected {} samples", storm_stats.count());
    report.add("Random seek storm", storm_stats);

    // ── Scenario 5: Sequential fast-path ────────────────────────────────

    println!("\n--- Scenario: Sequential fast-path ---");
    let mut seq_stats = Stats::new();
    {
        let _guard = if profile {
            Some(snippy2::bench::ProfilingGuard::new("seek_sequential", 100))
        } else {
            None
        };

        // Pick a keyframe near the start so we have room for +20
        let anchor_kf = keyframes
            .iter()
            .copied()
            .find(|&kf| kf + 20 < total_frames);

        if let Some(kf) = anchor_kf {
            // Seek to the anchor keyframe first
            rig.timed_seek(kf, true, seq, SEEK_TIMEOUT);
            seq += 1;

            // Sequential seeks: +1, +2, ..., +20
            for offset in 1..=20 {
                let target = kf + offset;
                match rig.timed_seek(target, false, seq, SEEK_TIMEOUT) {
                    Some((_frame, elapsed)) => seq_stats.push(elapsed),
                    None => eprintln!("  WARN: sequential seek to frame {target} timed out (seq={seq})"),
                }
                seq += 1;
            }
        } else {
            eprintln!("  WARN: no keyframe with 20 frames of headroom, skipping");
        }
    }
    println!("  Collected {} samples", seq_stats.count());
    report.add("Sequential fast-path", seq_stats);

    // ── Report ──────────────────────────────────────────────────────────

    report.print_table();
    let json_path = report.write_json("seek_latency")?;
    println!("  Report written to: {}", json_path.display());

    // Shutdown the engine
    let _ = rig.send(MediaCommand::Shutdown);

    Ok(())
}

/// Sample up to `max` keyframes evenly from the full list.
fn sample_keyframes(keyframes: &[usize], max: usize) -> Vec<usize> {
    let n = keyframes.len();
    if n <= max {
        return keyframes.to_vec();
    }

    let step = n as f64 / max as f64;
    (0..max)
        .map(|i| keyframes[(i as f64 * step) as usize])
        .collect()
}
