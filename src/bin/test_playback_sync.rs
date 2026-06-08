use crossbeam_channel::{bounded, unbounded};
use ringbuf::traits::*;
use std::thread;
use std::time::Duration;
use snippy2::media::{DecodedFrame, MediaCommand, MediaEngine};

fn main() -> Result<(), anyhow::Error> {
    let video_path = "/Users/dselck/Downloads/Season 1 (2008)/The Hollowmen - 0101 - Fat Chance.mp4";
    println!("=== Testing Playback & Sync with: {} ===", video_path);

    // 1. Create channels
    let (frame_tx, frame_rx) = bounded::<DecodedFrame>(4);
    let (_cmd_tx, cmd_rx) = unbounded::<MediaCommand>();

    // Set up dummy audio producer ring buffer
    let rb = ringbuf::SharedRb::<ringbuf::storage::Heap<f32>>::new(48000 * 2 * 2);
    let (audio_tx, mut audio_cons) = rb.split();

    // Set up dummy tags ring buffer
    let tags_rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(u64, u64)>>::new(256);
    let (tags_tx, mut tags_cons) = tags_rb.split();

    // Spawn a dummy audio consumer thread to drain audio buffer so media engine doesn't block on backpressure
    let _audio_consumer = thread::spawn(move || {
        let mut count = 0;
        loop {
            while audio_cons.try_pop().is_some() {
                count += 1;
            }
            while tags_cons.try_pop().is_some() {}
            thread::sleep(Duration::from_millis(10));
            if count > 0 && count % 50000 == 0 {
                println!("[Test Audio Consumer] Consumed {} samples so far", count);
            }
        }
    });

    let playback_state = std::sync::Arc::new(snippy2::audio::AudioPlaybackState {
        current_position_us: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        sample_rate: 48000,
        has_audio: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        fallback_start_time: std::sync::Arc::new(std::sync::Mutex::new(None)),
        fallback_base_time: std::sync::Arc::new(std::sync::Mutex::new(0.0)),
        should_flush: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        is_playing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        audio_paused_for_sync: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });

    // 2. Initialize Media Engine
    let (me, metadata) = MediaEngine::new(
        video_path,
        48000,
        1, // Mono
        frame_tx,
        audio_tx,
        tags_tx,
        cmd_rx,
        playback_state,
    )?;

    println!("Loaded metadata: total_frames={}, fps={}, width={}, height={}, has_audio={}, keyframes count={}",
             metadata.total_frames, metadata.fps, metadata.width, metadata.height, metadata.has_audio, metadata.keyframes.len());
    println!("First 10 keyframes: {:?}", &metadata.keyframes[..10.min(metadata.keyframes.len())]);

    // 3. Test Seek Keyframe-only (Scrubbing Simulation)
    let test_frame = 100;
    // Find nearest keyframe before 100
    let nearest_kf = match metadata.keyframes.binary_search(&test_frame) {
        Ok(i) => metadata.keyframes[i],
        Err(0) => metadata.keyframes[0],
        Err(i) => metadata.keyframes[i - 1],
    };
    println!("\n--- Test Scrubbing Seek: target_frame={}, nearest_keyframe={} ---", test_frame, nearest_kf);
    me.tx.send(MediaCommand::Seek { frame_index: nearest_kf, keyframe_only: true, sequence_id: 1 })?;

    // We expect to receive exactly the keyframe
    match frame_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(frame) => {
            println!("Received frame: index={}, seq={}", frame.frame_idx(), frame.sequence_id());
            assert_eq!(frame.frame_idx(), nearest_kf, "Should receive the nearest keyframe!");
            assert_eq!(frame.sequence_id(), 1, "Should have sequence_id 1");
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
    assert!(play_frames[0] > nearest_kf, "Play should decode from keyframe onwards!");

    // 5. Test Pause Command
    println!("\n--- Test Pause Command ---");
    me.tx.send(MediaCommand::Pause)?;
    thread::sleep(Duration::from_millis(200));
    // Clear frame_rx
    while frame_rx.try_recv().is_ok() {}

    // 6. Test Exact Seek (Non-keyframe)
    let exact_target = 105;
    println!("\n--- Test Exact Seek: target_frame={} ---", exact_target);
    me.tx.send(MediaCommand::Seek { frame_index: exact_target, keyframe_only: false, sequence_id: 2 })?;

    // Drain frame_rx (giving a short sleep to allow the video thread to process/unblock and clear the channel)
    thread::sleep(Duration::from_millis(50));
    while frame_rx.try_recv().is_ok() {}

    // We expect to receive multiple frames as the decoder catches up, ending at exact_target or greater.
    // Discard any stale frames with sequence_id < 2.
    let mut seek_frames = vec![];
    let start_time = std::time::Instant::now();
    while start_time.elapsed() < Duration::from_secs(2) {
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

    println!("\n=== All Tests Passed Successfully ===");
    Ok(())
}
