//! Headless arrow-key seeking benchmark.
//!
//! Simulates a user holding down ArrowRight (forward) and ArrowLeft (backward)
//! to measure achieved FPS and frame delivery latency under one-in-flight throttling.

use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
use snippy2::media::MediaCommand;
use std::time::{Duration, Instant};

const SEEK_TIMEOUT: Duration = Duration::from_secs(5);

/// Simulates a raw unthrottled hold where seek commands are sent back-to-back
/// as fast as the decoder thread can return them.
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

/// Simulates the GUI thread environment:
/// 1. Capped at a VSync refresh rate (e.g. 60Hz).
/// 2. Overhead of copying YUV planes to simulate CPU-to-GPU texture upload memory cost.
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
    let mut in_flight: Option<usize> = None;
    let mut last_send_time = Instant::now();

    let tick_interval = Duration::from_secs_f64(1.0 / vsync_hz);
    let mut last_tick = Instant::now();
    let mut frames_displayed = 0;

    let total_start = Instant::now();
    let duration_limit = Duration::from_secs(5);

    while frames_displayed < count && total_start.elapsed() < duration_limit {
        let now = Instant::now();
        let elapsed = now.duration_since(last_tick);

        if elapsed >= tick_interval {
            last_tick = now;

            // 1. UI receives frames from channel
            let mut got_frame = false;
            while let Ok(frame) = rig.frame_rx.try_recv() {
                if Some(frame.sequence_id()) == in_flight {
                    // Record latency
                    stats.push(last_send_time.elapsed());

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

                    in_flight = None;
                    got_frame = true;
                }
            }

            if got_frame {
                frames_displayed += 1;
            }

            // 2. UI sends next seek if idle
            if in_flight.is_none() && frames_displayed < count {
                if direction > 0 {
                    current = (current + 1).min(rig.metadata.total_frames.saturating_sub(1));
                } else {
                    current = current.saturating_sub(1);
                }

                *seq += 1;
                last_send_time = Instant::now();
                let _ = rig.send(MediaCommand::Seek {
                    frame_index: current,
                    keyframe_only: false,
                    sequence_id: *seq,
                    audio_enabled: false,
                });
                in_flight = Some(*seq);
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

    // ── Scenario 1: Hold Right from Paused State ──
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

    // ── Scenario 2: Hold Right from Play state (Buggy UI - no Pause sent) ──
    println!("\n--- Scenario 2: Hold Right from Play (Buggy - No Pause - Raw) ---");
    let timer_start = Instant::now();
    let buggy_stats = simulate_arrow_hold_raw(&rig, 0, 1, count, &mut seq, true, false);
    let buggy_duration = timer_start.elapsed();
    let buggy_fps = count as f64 / buggy_duration.as_secs_f64();
    println!(
        "  Buggy (No Pause): processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        buggy_duration.as_secs_f64(),
        buggy_fps
    );
    report.add("Hold Right (Buggy UI) Latency (Raw)", buggy_stats);

    // ── Scenario 3: Hold Right from Play state (Fixed UI - Pause sent) ──
    println!("\n--- Scenario 3: Hold Right from Play (Fixed - Pause Sent - Raw) ---");
    let timer_start = Instant::now();
    let fixed_stats = simulate_arrow_hold_raw(&rig, 0, 1, count, &mut seq, true, true);
    let fixed_duration = timer_start.elapsed();
    let fixed_fps = count as f64 / fixed_duration.as_secs_f64();
    println!(
        "  Fixed (Pause Sent): processed {} frames in {:.2}s ({:.1} FPS)",
        count,
        fixed_duration.as_secs_f64(),
        fixed_fps
    );
    report.add("Hold Right (Fixed UI) Latency (Raw)", fixed_stats);

    // ── Scenario 4: Simulated GUI 60Hz Hold Seek ──
    println!("\n--- Scenario 4: Simulated GUI 60Hz Hold Seek ---");
    let (gui_stats, gui_fps) = simulate_arrow_hold_gui(&rig, 0, 1, count, &mut seq, 60.0);
    println!(
        "  Simulated 60Hz GUI: processed {} frames ({:.1} FPS)",
        count,
        gui_fps
    );
    report.add("Hold Right (Simulated 60Hz GUI) Latency", gui_stats);

    report.print_table();
    report.write_json("arrow_seek")?;

    let _ = rig.send(MediaCommand::Shutdown);
    Ok(())
}
