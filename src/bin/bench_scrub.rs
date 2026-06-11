//! Headless scrub-responsiveness profiler.
//!
//! Simulates realistic timeline-dragging patterns and measures end-to-end seek
//! latency under the one-at-a-time seek protocol used by the Snippy2 UI.

use snippy2::bench::{format_video_info, BenchReport, MediaTestRig, Stats, TestMediaLocator};
use snippy2::media::MediaCommand;
use std::time::{Duration, Instant};

// ── Scrub simulator ────────────────────────────────────────────────────────

/// Faithfully reproduces the one-at-a-time seek protocol:
///
/// * At most one seek is in flight at a time.
/// * When a seek response arrives and the desired position has moved, a new
///   seek is fired immediately.
/// * `step_interval` controls how fast the simulated user drags the playhead.
///
/// Returns `(seek_latency_stats, frame_lag_stats)`.
/// Frame lag is stored as `Duration::from_micros(frame_count)` — the µs value
/// is the frame-distance between the displayed frame and the desired position.
fn simulate_scrub(
    rig: &MediaTestRig,
    positions: &[usize],
    step_interval: Duration,
    seq: &mut usize,
) -> (Stats, Stats) {
    let mut outstanding_seq: Option<usize> = None;
    let mut outstanding_send_time: Option<Instant> = None;
    let mut seek_latency = Stats::new();
    let mut frame_lag = Stats::new();
    let mut desired_frame = positions.first().copied().unwrap_or(0);

    for &pos in positions {
        std::thread::sleep(step_interval);
        desired_frame = pos;

        // Fire a seek if the pipeline is idle
        if outstanding_seq.is_none() {
            *seq += 1;
            let _ = rig.send(MediaCommand::Seek {
                frame_index: desired_frame,
                keyframe_only: true,
                sequence_id: *seq,
                audio_enabled: false,
            });
            outstanding_seq = Some(*seq);
            outstanding_send_time = Some(Instant::now());
        }

        // Non-blocking drain: check for completed seeks
        while let Ok(frame) = rig.frame_rx.try_recv() {
            if Some(frame.sequence_id()) == outstanding_seq {
                let latency = outstanding_send_time.unwrap().elapsed();
                seek_latency.push(latency);

                let lag = desired_frame.abs_diff(frame.frame_idx());
                frame_lag.push(Duration::from_micros(lag as u64));

                outstanding_seq = None;
                outstanding_send_time = None;

                // If the user has moved on, fire next seek immediately
                if desired_frame != frame.frame_idx() {
                    *seq += 1;
                    let _ = rig.send(MediaCommand::Seek {
                        frame_index: desired_frame,
                        keyframe_only: true,
                        sequence_id: *seq,
                        audio_enabled: false,
                    });
                    outstanding_seq = Some(*seq);
                    outstanding_send_time = Some(Instant::now());
                }
            }
            // Stale frames from earlier sequence IDs are silently dropped.
        }
    }

    // Wait for any outstanding seek to complete
    if let Some(expected) = outstanding_seq {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match rig.frame_rx.recv_timeout(remaining) {
                Ok(frame) if frame.sequence_id() == expected => {
                    seek_latency.push(outstanding_send_time.unwrap().elapsed());
                    let lag = desired_frame.abs_diff(frame.frame_idx());
                    frame_lag.push(Duration::from_micros(lag as u64));
                    break;
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    }

    (seek_latency, frame_lag)
}

// ── Position generators ────────────────────────────────────────────────────

fn linear_positions(start: usize, end: usize, step: usize) -> Vec<usize> {
    if start <= end {
        (start..=end).step_by(step).collect()
    } else {
        (end..=start).rev().step_by(step).collect()
    }
}

fn jittery_positions(start: usize, end: usize, step: usize, total_frames: usize) -> Vec<usize> {
    let mut out = Vec::new();
    let mut center = start;
    let mut i: usize = 0;
    while center <= end {
        let jitter = (i.wrapping_mul(17)) % 11;
        let jittered = (center as isize + jitter as isize - 5).clamp(0, total_frames as isize - 1);
        out.push(jittered as usize);
        center += step;
        i += 1;
    }
    out
}

fn jump_positions(count: usize, total_frames: usize) -> Vec<usize> {
    (0..count).map(|i| (i * 7919) % total_frames).collect()
}

// ── Scenario runner ────────────────────────────────────────────────────────

struct Scenario {
    name: &'static str,
    positions: Vec<usize>,
    step_interval: Duration,
}

fn run_scenario(
    rig: &MediaTestRig,
    scenario: &Scenario,
    seq: &mut usize,
    report: &mut BenchReport,
    profile: bool,
) {
    println!("  ▸ {} ({} positions)", scenario.name, scenario.positions.len());

    // Reset state: pause + drain
    let _ = rig.send(MediaCommand::Pause);
    std::thread::sleep(Duration::from_millis(50));
    rig.drain_frames();

    let sanitize_name = scenario.name.to_lowercase().replace(' ', "_");
    let _guard = if profile {
        Some(snippy2::bench::ProfilingGuard::new(&format!("scrub_{}", sanitize_name), 100))
    } else {
        None
    };

    let (latency, lag) = simulate_scrub(rig, &scenario.positions, scenario.step_interval, seq);
    drop(_guard);
    println!(
        "    seeks: {}  median: {}",
        latency.count(),
        latency.median().map_or("—".into(), |d| Stats::fmt_duration(d)),
    );

    report.add(scenario.name, latency);
    report.add(&format!("{} (frame lag)", scenario.name), lag);

    // Settle seek: precise seek to the final desired position
    if let Some(&final_pos) = scenario.positions.last() {
        *seq += 1;
        if let Some((_frame, settle_dur)) =
            rig.timed_seek(final_pos, false, *seq, Duration::from_secs(5))
        {
            let mut settle = Stats::new();
            settle.push(settle_dur);
            report.add(&format!("{} (settle)", scenario.name), settle);
        }
    }
}

// ── Main ───────────────────────────────────────────────────────────────────

fn main() -> Result<(), anyhow::Error> {
    let profile = std::env::args().any(|arg| arg == "--profile");
    let path = TestMediaLocator::resolve()?;
    let rig = MediaTestRig::new(&path)?;
    let total = rig.metadata.total_frames;
    let info = format_video_info(&rig.metadata);

    println!("\n[bench_scrub] {info}");
    let mut report = BenchReport::new("Scrub Responsiveness", &info);
    let mut seq: usize = 0;

    let cap_slow = (total / 4).min(100);
    let cap_fast = (total / 2).min(300);

    let scenarios = vec![
        Scenario {
            name: "Slow fwd",
            positions: linear_positions(0, cap_slow, 2),
            step_interval: Duration::from_millis(100),
        },
        Scenario {
            name: "Slow rev",
            positions: linear_positions(cap_slow, 0, 2),
            step_interval: Duration::from_millis(100),
        },
        Scenario {
            name: "Fast fwd",
            positions: linear_positions(0, cap_fast, 10),
            step_interval: Duration::from_millis(30),
        },
        Scenario {
            name: "Fast rev",
            positions: linear_positions(cap_fast, 0, 10),
            step_interval: Duration::from_millis(30),
        },
        Scenario {
            name: "Jittery",
            positions: jittery_positions(cap_slow, cap_fast, 3, total),
            step_interval: Duration::from_millis(50),
        },
        Scenario {
            name: "Jump",
            positions: jump_positions(20, total),
            step_interval: Duration::from_millis(200),
        },
    ];

    for scenario in &scenarios {
        run_scenario(&rig, scenario, &mut seq, &mut report, profile);
    }

    report.print_table();
    report.write_json("scrub")?;

    let _ = rig.send(MediaCommand::Shutdown);
    Ok(())
}
