//! Headless arrow-key seeking benchmark.
//!
//! Simulates a user holding down ArrowRight (forward) and ArrowLeft (backward)
//! to measure achieved FPS and frame delivery latency under one-in-flight throttling.

use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
use snippy2::media::MediaCommand;
use std::time::{Duration, Instant};

const SEEK_TIMEOUT: Duration = Duration::from_secs(5);

fn simulate_arrow_hold(
    rig: &MediaTestRig,
    start_frame: usize,
    direction: isize,
    count: usize,
    seq: &mut usize,
) -> Stats {
    let mut stats = Stats::new();
    let mut current = start_frame;

    // Tap seek (first step)
    *seq += 1;
    if let Some((_, elapsed)) = rig.timed_seek(current, false, *seq, SEEK_TIMEOUT) {
        stats.push(elapsed);
    }

    // Continuous hold seeks
    for _ in 0..count {
        if direction > 0 {
            current = (current + 1).min(rig.metadata.total_frames.saturating_sub(1));
        } else {
            current = current.saturating_sub(1);
        }

        *seq += 1;
        if let Some((_, elapsed)) = rig.timed_seek(current, false, *seq, SEEK_TIMEOUT) {
            stats.push(elapsed);
        }
    }

    stats
}

fn main() -> Result<(), anyhow::Error> {
    let video_path = TestMediaLocator::resolve()?;
    let rig = MediaTestRig::new(&video_path)?;
    let video_info = format_video_info(&rig.metadata);

    println!("=== Arrow-Key Seek Performance Benchmark ===");
    println!("  Video: {}", video_path.display());
    println!("  {video_info}");

    let total_frames = rig.metadata.total_frames;
    let mut report = BenchReport::new("Arrow-Key Seeking", &video_info);
    let mut seq = 0;

    // ── Scenario 1: Hold Right (Forward) ──
    println!("\n--- Scenario: Hold Right (Forward) ---");
    let start_forward = 0;
    let count = (total_frames / 2).min(120); // Test up to 120 frames or half the video
    let timer_start = Instant::now();
    let forward_stats = simulate_arrow_hold(&rig, start_forward, 1, count, &mut seq);
    let forward_duration = timer_start.elapsed();
    let forward_fps = count as f64 / forward_duration.as_secs_f64();
    println!(
        "  Forward Hold: processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        forward_duration.as_secs_f64(),
        forward_fps
    );
    report.add("Hold Right (Forward) Latency", forward_stats);

    // ── Scenario 2: Hold Left (Backward) ──
    println!("\n--- Scenario: Hold Left (Backward) ---");
    let start_backward = count;
    let timer_start = Instant::now();
    let backward_stats = simulate_arrow_hold(&rig, start_backward, -1, count, &mut seq);
    let backward_duration = timer_start.elapsed();
    let backward_fps = count as f64 / backward_duration.as_secs_f64();
    println!(
        "  Backward Hold: processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        backward_duration.as_secs_f64(),
        backward_fps
    );
    report.add("Hold Left (Backward) Latency", backward_stats);

    report.print_table();
    report.write_json("arrow_seek")?;

    let _ = rig.send(MediaCommand::Shutdown);
    Ok(())
}
