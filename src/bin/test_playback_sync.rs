use crossbeam_channel::{bounded, unbounded};
use ringbuf::traits::*;
use std::thread;
use std::time::Duration;
use snippy2::media::{DecodedFrame, MediaCommand, MediaEngine};

fn main() -> Result<(), anyhow::Error> {
    let video_path_buf = snippy2::bench::TestMediaLocator::resolve()?;
    let video_path = video_path_buf.to_string_lossy();
    println!("=== Testing Playback & Sync with: {} ===", video_path);

    // 1. Create channels
    let (frame_tx, frame_rx) = bounded::<DecodedFrame>(4);
    let (_cmd_tx, cmd_rx) = unbounded::<MediaCommand>();

    // Set up dummy audio producer ring buffer
    let rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(f32, u64)>>::new(48000 * 2 * 2);
    let (audio_tx, mut audio_cons) = rb.split();

    let tags_rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(u64, u64, u64)>>::new(256);
    let (tags_tx, mut tags_cons) = tags_rb.split();

    let (master_clock, ticker) = snippy2::audio::clock::MasterClock::new(48000);
    master_clock.set_has_audio(true);
    master_clock.play();

    // Spawn a dummy audio consumer thread to drain audio buffer so media engine doesn't block on backpressure
    let ticker_clone = ticker.clone();
    let _audio_consumer = thread::spawn(move || {
        let mut count = 0;
        let mut current_consumer_generation = 0;
        let mut last_tag = None;
        let mut pending_tag = None;
        loop {
            let active = ticker_clone.is_active();
            let producer_gen = ticker_clone.playback_generation();
            if producer_gen != current_consumer_generation {
                while let Some((_, generation)) = audio_cons.try_pop() {
                    if generation == producer_gen {
                        break;
                    }
                }
                count = 0;
                last_tag = None;
                pending_tag = None;
                current_consumer_generation = producer_gen;
            }

            if active {
                let mut written = 0;
                while let Some((_, generation)) = audio_cons.try_pop() {
                    if generation != current_consumer_generation {
                        continue;
                    }
                    written += 1;
                    count += 1;

                    loop {
                        let next_tag = pending_tag.take().or_else(|| tags_cons.try_pop());
                        if let Some((tag_generation, tag_idx, tag_pts_us)) = next_tag {
                            if tag_generation != current_consumer_generation {
                                continue;
                            }
                            if tag_idx <= count {
                                last_tag = Some((tag_idx, tag_pts_us));
                            } else {
                                pending_tag = Some((tag_generation, tag_idx, tag_pts_us));
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                if written > 0 {
                    ticker_clone.advance(written, last_tag, count);
                }
            }
            thread::sleep(Duration::from_millis(10));
            if count > 0 && count % 50000 == 0 {
                println!("[Test Audio Consumer] Consumed {} samples so far", count);
            }
        }
    });

    let (me, metadata) = MediaEngine::new(
        &video_path_buf,
        48000,
        1, // Mono
        frame_tx,
        audio_tx,
        tags_tx,
        cmd_rx,
        master_clock.clone(),
    )?;

    println!("Loaded metadata: total_frames={}, fps={}, width={}, height={}, has_audio={}, keyframes count={}",
             metadata.total_frames, metadata.fps, metadata.width, metadata.height, metadata.has_audio, metadata.keyframes.len());
    println!("First 10 keyframes: {:?}", &metadata.keyframes[..10.min(metadata.keyframes.len())]);
    println!("First 10 keyframe PTS: {:?}", &metadata.keyframes_pts[..10.min(metadata.keyframes_pts.len())]);

    println!("\n--- Test Scrubbing Seek to 0 ---");
    me.tx.send(MediaCommand::Seek { frame_index: 0, keyframe_only: false, sequence_id: 1, audio_enabled: false })?;

    // We expect to receive exactly frame 0
    match frame_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(frame) => {
            println!("Received frame: index={}, seq={}", frame.frame_idx(), frame.sequence_id());
        }
        Err(e) => panic!("Did not receive frame after seek: {:?}", e),
    }

    // Wait 500ms and check that no further frames are received (confirming decoder thread is paused)
    println!("Waiting 500ms to verify no extra frames are received...");
    thread::sleep(Duration::from_millis(500));
    let mut extra_frames = vec![];
    while let Ok(frame) = frame_rx.try_recv() {
        extra_frames.push(frame.frame_idx());
    }
    if !extra_frames.is_empty() {
        println!("WARNING: Received extra frames in keyframe-only mode: {:?}", extra_frames);
    } else {
        println!("Success: Decoder thread correctly stopped after decoding the keyframe.");
    }

    // 4. Test Play Command
    println!("\n--- Test Play Command ---");
    me.tx.send(MediaCommand::Play)?;

    // We expect frames to start decoding sequentially
    let mut play_frames = vec![];
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < Duration::from_secs(1) {
        if let Ok(frame) = frame_rx.recv_timeout(Duration::from_millis(100)) {
            play_frames.push(frame.frame_idx());
        }
    }
    println!("Received sequential play frames (first 15): {:?}", &play_frames[..15.min(play_frames.len())]);
    assert!(!play_frames.is_empty(), "Should decode frames in Play mode!");
    assert!(play_frames[0] > 0, "Play should decode from keyframe onwards!");

    // 5. Test Exact Seek directly after Play (without Pause command)
    let exact_target = 340;
    println!("\n--- Test Exact Seek directly after Play: target_frame={} ---", exact_target);
    master_clock.reset(exact_target as f64 / metadata.fps);
    me.tx.send(MediaCommand::Seek { frame_index: exact_target, keyframe_only: false, sequence_id: 2, audio_enabled: true })?;



    // We expect to receive multiple frames as the decoder catches up, ending at exact_target or greater.
    // Discard any stale frames with sequence_id < 2.
    let mut seek_frames = vec![];
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < Duration::from_secs(10) {
        if let Ok(frame) = frame_rx.recv_timeout(Duration::from_millis(200)) {
            if frame.sequence_id() < 2 {
                println!("Discarding stale frame: index={}, seq={}", frame.frame_idx(), frame.sequence_id());
                continue;
            }
            seek_frames.push(frame.frame_idx());
            if frame.frame_idx() >= exact_target {
                break;
            }
        }
    }
    println!("Seek frames received: {:?}", seek_frames);
    assert!(seek_frames.contains(&exact_target), "Should decode and receive target frame {}", exact_target);

    // Verify that playback resumed automatically from the target frame!
    println!("Verifying that playback resumes automatically after seek...");
    let mut post_seek_frames = vec![];
    let post_start_time = std::time::Instant::now();
    while post_start_time.elapsed() < Duration::from_secs(4) {
        if let Ok(frame) = frame_rx.recv_timeout(Duration::from_millis(200)) {
            if frame.sequence_id() == 2 {
                post_seek_frames.push(frame.frame_idx());
                if post_seek_frames.len() >= 3 {
                    break;
                }
            }
        }
    }
    println!("Post-seek frames received: {:?}", post_seek_frames);
    assert!(post_seek_frames.len() >= 3, "Playback should automatically resume, decoding subsequent frames!");
    assert!(post_seek_frames.iter().all(|&f| f > exact_target), "Subsequent frames should be greater than seek target!");

    // 6. Test egui UI timeline click seek scenario (two rapid seeks during playback)
    println!("\n--- Test egui UI timeline click seek scenario (rapid Seek commands during playback) ---");
    // Resume playback first
    me.tx.send(MediaCommand::Play)?;
    
    // Wait for a few frames to decode
    thread::sleep(Duration::from_millis(200));
    let mut temp = vec![];
    while let Ok(frame) = frame_rx.try_recv() {
        temp.push(frame.frame_idx());
    }
    println!("Resumed playback frames: {:?}", temp);

    // Now send rapid double seek (first audio_enabled: false, second audio_enabled: true)
    let double_seek_target = 400;
    master_clock.reset(double_seek_target as f64 / metadata.fps);
    
    println!("Sending Seek 1 (audio_enabled: false, seq=3)...");
    me.tx.send(MediaCommand::Seek { frame_index: double_seek_target, keyframe_only: true, sequence_id: 3, audio_enabled: false })?;
    
    println!("Sending Seek 2 (audio_enabled: true, seq=4)...");
    me.tx.send(MediaCommand::Seek { frame_index: double_seek_target, keyframe_only: false, sequence_id: 4, audio_enabled: true })?;

    // We expect to receive the target frame of seq 4, and then subsequent playback frames because we were playing before!
    println!("Waiting for seek target frame (seq 4)...");
    let mut seq4_received = false;
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < Duration::from_secs(10) {
        if let Ok(frame) = frame_rx.recv_timeout(Duration::from_millis(200)) {
            if frame.sequence_id() == 4 {
                println!("Received target frame: index={}, seq={}", frame.frame_idx(), frame.sequence_id());
                if frame.frame_idx() >= double_seek_target {
                    seq4_received = true;
                    break;
                }
            } else {
                println!("Received other frame: index={}, seq={}", frame.frame_idx(), frame.sequence_id());
            }
        }
    }
    assert!(seq4_received, "Should receive seek target frame for seq 4");

    // Check if playback resumes automatically
    println!("Verifying that playback resumes automatically after double seek...");
    let mut double_post_seek_frames = vec![];
    let post_start_time = std::time::Instant::now();
    while post_start_time.elapsed() < Duration::from_secs(4) {
        if let Ok(frame) = frame_rx.recv_timeout(Duration::from_millis(200)) {
            if frame.sequence_id() == 4 {
                double_post_seek_frames.push(frame.frame_idx());
                if double_post_seek_frames.len() >= 3 {
                    break;
                }
            }
        }
    }
    println!("Double post-seek frames received: {:?}", double_post_seek_frames);
    assert!(double_post_seek_frames.len() >= 3, "Playback should automatically resume after double seek!");

    println!("\n=== All Tests Passed Successfully ===");
    Ok(())
}
