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
    pre_play: bool,
    send_pause: bool,
) -> Stats {
    // Drain any stale frames first
    rig.drain_frames();

    if pre_play {
        // Put the engine in Play state
        let _ = rig.send(MediaCommand::Play);
        // Wait briefly for playback to begin
        std::thread::sleep(Duration::from_millis(50));

        if send_pause {
            // Pause the engine properly before seeking
            let _ = rig.send(MediaCommand::Pause);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

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

    let count = (total_frames / 2).min(60); // Test up to 60 frames

    // ── Scenario 1: Hold Right from Paused State ──
    println!("\n--- Scenario 1: Hold Right from Paused State ---");
    let timer_start = Instant::now();
    let forward_stats = simulate_arrow_hold(&rig, 0, 1, count, &mut seq, false, false);
    let forward_duration = timer_start.elapsed();
    let forward_fps = count as f64 / forward_duration.as_secs_f64();
    println!(
        "  Hold Right: processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        forward_duration.as_secs_f64(),
        forward_fps
    );
    report.add("Hold Right (Paused) Latency", forward_stats);

    // ── Scenario 2: Hold Left from Paused State ──
    println!("\n--- Scenario 2: Hold Left from Paused State ---");
    let timer_start = Instant::now();
    let backward_stats = simulate_arrow_hold(&rig, count, -1, count, &mut seq, false, false);
    let backward_duration = timer_start.elapsed();
    let backward_fps = count as f64 / backward_duration.as_secs_f64();
    println!(
        "  Hold Left: processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        backward_duration.as_secs_f64(),
        backward_fps
    );
    report.add("Hold Left (Paused) Latency", backward_stats);

    // ── Scenario 3: Hold Right from Play state (Buggy UI - no Pause sent) ──
    println!("\n--- Scenario 3: Hold Right from Play (Buggy - No Pause) ---");
    let timer_start = Instant::now();
    let buggy_stats = simulate_arrow_hold(&rig, 0, 1, count, &mut seq, true, false);
    let buggy_duration = timer_start.elapsed();
    let buggy_fps = count as f64 / buggy_duration.as_secs_f64();
    println!(
        "  Buggy (No Pause): processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        buggy_duration.as_secs_f64(),
        buggy_fps
    );
    report.add("Hold Right (Buggy UI) Latency", buggy_stats);

    // ── Scenario 4: Hold Right from Play state (Fixed UI - Pause sent) ──
    println!("\n--- Scenario 4: Hold Right from Play (Fixed - Pause Sent) ---");
    let timer_start = Instant::now();
    let fixed_stats = simulate_arrow_hold(&rig, 0, 1, count, &mut seq, true, true);
    let fixed_duration = timer_start.elapsed();
    let fixed_fps = count as f64 / fixed_duration.as_secs_f64();
    println!(
        "  Fixed (Pause Sent): processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        fixed_duration.as_secs_f64(),
        fixed_fps
    );
    report.add("Hold Right (Fixed UI) Latency", fixed_stats);

    report.print_table();
    report.write_json("arrow_seek")?;

    let _ = rig.send(MediaCommand::Shutdown);
    Ok(())
}
