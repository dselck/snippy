//! Playback frame timing and jitter profiler.
//!
//! Simulates active synchronized playback and measures the exact arrival intervals
//! of frames on the UI thread side, computing the jitter relative to the video's
//! nominal frame rate (e.g. 41.67ms for 24fps).

use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
use snippy2::media::{MediaCommand, MasterClockPort};
use std::time::{Duration, Instant};

fn main() -> Result<(), anyhow::Error> {
    let video_path = TestMediaLocator::resolve()?;
    let rig = MediaTestRig::new(&video_path)?;
    let fps = rig.metadata.fps;
    let nominal_interval = Duration::from_secs_f64(1.0 / fps);
    let info = format_video_info(&rig.metadata);

    println!("=== Playback Jitter Profile ===");
    println!("  Video: {}", video_path.display());
    println!("  {info}");
    println!("  Nominal frame interval: {:.2}ms", nominal_interval.as_secs_f64() * 1000.0);

    // Clean start state
    rig.send(MediaCommand::Pause)?;
    std::thread::sleep(Duration::from_millis(50));
    rig.drain_frames();

    // Start playback: play clock and send Play command
    rig.master_clock.set_position(0.0);
    rig.master_clock.play();
    rig.send(MediaCommand::Play)?;

    // Warmup for 1.0s to let the pipeline decode keyframes, fill queues, and stabilize
    println!("  ▸ Warming up playback for 1.0s...");
    let warmup_deadline = Instant::now() + Duration::from_secs(1);
    let mut warmup_progress_time = Instant::now();
    let mut progress_count = 1;
    while Instant::now() < warmup_deadline {
        let _ = rig.drain_frames();
        std::thread::sleep(Duration::from_millis(10));
        
        if Instant::now().duration_since(warmup_progress_time) >= Duration::from_millis(100) && progress_count <= 10 {
            println!("    Warmup progress {}/10: Clock PTS: {:?}", progress_count, rig.master_clock.current_pts());
            warmup_progress_time = Instant::now();
            progress_count += 1;
        }
    }
    let warmup_frames = rig.drain_frames();
    println!("  ▸ Warmup complete (drained {} frames). Starting measurement...", warmup_frames.len());

    let mut frame_arrival_times = Vec::new();
    let start_time = Instant::now();
    // Collect frames for 3.0 seconds
    let run_duration = Duration::from_secs(3);
    let deadline = start_time + run_duration;

    println!("\n  ▸ Collecting playback frame timings for {:.1}s...", run_duration.as_secs_f64());

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if rig.recv_frame(remaining.min(Duration::from_millis(100))).is_some() {
            frame_arrival_times.push(Instant::now());
        }
    }

    // Stop playback
    rig.send(MediaCommand::Pause)?;
    rig.master_clock.pause();

    let count = frame_arrival_times.len();
    println!("  Collected {} frames", count);

    if count < 2 {
        anyhow::bail!("Not enough frames collected to compute jitter (needed at least 2).");
    }

    let mut interval_stats = Stats::new();
    let mut jitter_stats = Stats::new();
    
    for i in 1..count {
        let delta = frame_arrival_times[i].duration_since(frame_arrival_times[i - 1]);
        interval_stats.push(delta);

        let jitter_sec = (delta.as_secs_f64() - nominal_interval.as_secs_f64()).abs();
        jitter_stats.push(Duration::from_secs_f64(jitter_sec));
    }

    let mut report = BenchReport::new("Playback Jitter", &info);
    report.add("Frame Arrival Interval", interval_stats);
    report.add("Absolute Timing Jitter", jitter_stats);

    report.print_table();
    let _ = report.write_json("playback_jitter")?;

    let _ = rig.send(MediaCommand::Shutdown);
    Ok(())
}
