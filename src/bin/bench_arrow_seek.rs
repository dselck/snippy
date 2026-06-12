//! Headless arrow-key seeking benchmark.
//!
//! Simulates a user holding down ArrowRight (forward) and ArrowLeft (backward)
//! to measure achieved FPS and frame delivery latency under VSync-ticked seeking and buffer draining.

use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
use snippy2::media::MediaCommand;
use std::time::{Duration, Instant};

const SEEK_TIMEOUT: Duration = Duration::from_secs(5);

fn simulate_arrow_hold_raw(
    rig: &MediaTestRig,
    start_frame: usize,
    direction: isize,
    count: usize,
    seq: &mut usize,
    pre_play: bool,
    send_pause: bool,
) -> Stats {
    rig.drain_frames();

    if pre_play {
        let _ = rig.send(MediaCommand::Play);
        std::thread::sleep(Duration::from_millis(50));
        if send_pause {
            let _ = rig.send(MediaCommand::Pause);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let mut stats = Stats::new();
    let mut current = start_frame;

    *seq += 1;
    if let Some((_, elapsed)) = rig.timed_seek(current, false, *seq, SEEK_TIMEOUT) {
        stats.push(elapsed);
    }

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

fn simulate_arrow_hold_gui(
    rig: &MediaTestRig,
    start_frame: usize,
    direction: isize,
    count: usize,
    seq: &mut usize,
    vsync_hz: f64,
) -> (Stats, f64) {
    rig.drain_frames();

    let mut stats = Stats::new();
    let mut current = start_frame;
    let mut send_times = std::collections::HashMap::<usize, Instant>::new();

    let tick_interval = Duration::from_secs_f64(1.0 / vsync_hz);
    let mut next_tick = Instant::now() + tick_interval;
    let mut frames_displayed = 0;

    let total_start = Instant::now();
    let duration_limit = Duration::from_secs(5);

    while frames_displayed < count && total_start.elapsed() < duration_limit {
        let now = Instant::now();

        if now >= next_tick {
            // Align next tick to avoid drift
            next_tick += tick_interval;

            // 1. UI receives frames from channel (draining channel)
            let mut latest_frame_received = None;

            while let Ok(frame) = rig.frame_rx.try_recv() {
                if frame.sequence_id() < *seq {
                    continue;
                }
                latest_frame_received = Some(frame);
            }

            if let Some(frame) = latest_frame_received {
                let frame_seq = frame.sequence_id();
                if let Some(send_time) = send_times.remove(&frame_seq) {
                    stats.push(send_time.elapsed());
                }

                // Simulate texture upload overhead (copy YUV planes)
                if let snippy2::media::DecodedFrame::Cpu { planes, .. } = &frame {
                    let mut dummy_y = vec![0u8; planes.y.len()];
                    let mut dummy_u = vec![0u8; planes.u.len()];
                    let mut dummy_v = vec![0u8; planes.v.len()];
                    dummy_y.copy_from_slice(&planes.y);
                    dummy_u.copy_from_slice(&planes.u);
                    dummy_v.copy_from_slice(&planes.v);
                    std::hint::black_box((dummy_y, dummy_u, dummy_v));
                }

                frames_displayed += 1;
            }

            // 2. UI sends next seek unconditionally on every tick (no single-in-flight lock)
            if frames_displayed < count {
                if direction > 0 {
                    current = (current + 1).min(rig.metadata.total_frames.saturating_sub(1));
                } else {
                    current = current.saturating_sub(1);
                }

                *seq += 1;
                send_times.insert(*seq, Instant::now());
                let _ = rig.send(MediaCommand::Seek {
                    frame_index: current,
                    keyframe_only: false,
                    sequence_id: *seq,
                    audio_enabled: false,
                });
            }
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    let total_elapsed = total_start.elapsed().as_secs_f64();
    let actual_fps = frames_displayed as f64 / total_elapsed;

    (stats, actual_fps)
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

    let count = (total_frames / 2).min(60);

    // ── Scenario 1: Hold Right from Paused State (Raw) ──
    println!("\n--- Scenario 1: Hold Right from Paused State (Raw) ---");
    let timer_start = Instant::now();
    let forward_stats = simulate_arrow_hold_raw(&rig, 0, 1, count, &mut seq, false, false);
    let forward_duration = timer_start.elapsed();
    let forward_fps = count as f64 / forward_duration.as_secs_f64();
    println!(
        "  Hold Right: processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        forward_duration.as_secs_f64(),
        forward_fps
    );
    report.add("Hold Right (Paused) Latency (Raw)", forward_stats);

    // ── Scenario 2: Simulated GUI 60Hz Hold Seek (VSync-Ticked with Buffer Drain) ──
    println!("\n--- Scenario 2: Simulated GUI 60Hz Hold Seek (VSync-Ticked with Buffer Drain) ---");
    let (gui_stats_1, gui_fps_1) = simulate_arrow_hold_gui(&rig, 0, 1, count, &mut seq, 60.0);
    println!(
        "  Simulated 60Hz GUI: processed {} frames ({:.1} FPS)",
        count,
        gui_fps_1
    );
    report.add("Hold Right (Simulated 60Hz GUI) Latency", gui_stats_1);

    report.print_table();
    report.write_json("arrow_seek")?;

    let _ = rig.send(MediaCommand::Shutdown);
    Ok(())
}
