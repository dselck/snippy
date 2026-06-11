# Lookahead Pre-decoding to Prevent GOP Boundary Spikes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decouple decoding from real-time presentation timing by buffering decoded frames in an internal play queue on the Media Engine thread, eliminating playback stutters at GOP boundaries.

**Architecture:** Introduce `play_queue: VecDeque<DecodedFrame>` in the `MediaEngine` thread. If the queue has space (< 15 frames) and decoding is active, read and decode packets. If the front frame in the queue is due according to the Master Clock, pop and present it immediately without blocking or sleeping in a wait loop.

**Tech Stack:** Rust standard library (`std::collections::VecDeque`).

---

### Task 1: Initialize Play Queue in Media Engine Thread

**Files:**
- Modify: [src/media/mod.rs](file:///Users/dselck/Documents/Snippy2/src/media/mod.rs)

- [ ] **Step 1: Import VecDeque and initialize play queue variables**

Import `std::collections::VecDeque` at the top of the file (or use path). Before the main loop starts in `MediaEngine::new` background thread (around line 210), initialize the `play_queue` and the `MAX_LOOKAHEAD_FRAMES` constant.

```rust
// In src/media/mod.rs around line 210
    let mut sync_engine = crate::media::sync::VideoSyncEngine::new(Box::new(master_clock.clone()));
    let mut audio_enabled = true;
    let mut packets_iter = input.packets();

    let mut play_queue: std::collections::VecDeque<DecodedFrame> = std::collections::VecDeque::new();
    const MAX_LOOKAHEAD_FRAMES: usize = 15;

    loop {
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Compiles successfully with warnings about unused variables.

- [ ] **Step 3: Commit**

```bash
git add src/media/mod.rs
git commit -m "refactor: initialize play queue in MediaEngine loop"
```

---

### Task 2: Clear Play Queue on Seek

**Files:**
- Modify: [src/media/mod.rs](file:///Users/dselck/Documents/Snippy2/src/media/mod.rs)

- [ ] **Step 1: Clear the play queue when a Seek command is received**

Modify the command handling block for `MediaCommand::Seek` inside the main loop to call `play_queue.clear()`.

```rust
// In src/media/mod.rs around lines 220-230
                MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
                    audio_enabled = ae;
                    play_queue.clear(); // Clear the lookahead buffer
                    let handled = video_pipeline.seek(frame_index, keyframe_only, sequence_id, audio_enabled);
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add src/media/mod.rs
git commit -m "refactor: clear play queue on seek command"
```

---

### Task 3: Implement Lookahead and Presentation Loop

**Files:**
- Modify: [src/media/mod.rs](file:///Users/dselck/Documents/Snippy2/src/media/mod.rs)

- [ ] **Step 1: Replace the loop body from line 265 onwards**

Replace the old rate limit, packet demuxing, decoding, and blocking wait loop with the lookahead queue presentation and decoding steps.

```rust
// In src/media/mod.rs, replace old lines 265-383 with:
        // 2. Present due frames from the front of the queue
        if master_clock.is_playing() {
            if let Some(front_frame) = play_queue.front() {
                let pts_sec = match front_frame {
                    DecodedFrame::Cpu { pts_sec, .. } => *pts_sec,
                    DecodedFrame::Gpu { pts_sec, .. } => *pts_sec,
                };

                let action = sync_engine.evaluate_frame(pts_sec);
                match action {
                    Ok(SyncAction::Present) => {
                        let frame = play_queue.pop_front().unwrap();
                        let _ = frame_tx.send(frame);
                    }
                    Ok(SyncAction::Wait(_)) => {
                        // Frame is not due yet, don't block. We can decode more or sleep.
                    }
                    Err(_) => {
                        let frame = play_queue.pop_front().unwrap();
                        let _ = frame_tx.send(frame);
                    }
                }
            }
        } else {
            // Paused/seek mode: drain queue and send immediately
            while let Some(frame) = play_queue.pop_front() {
                let _ = frame_tx.send(frame);
            }
        }

        // 3. Decode ahead if queue has space and decoder is active
        let queue_needs_frames = play_queue.len() < MAX_LOOKAHEAD_FRAMES;
        let is_decoding_active = video_pipeline.is_active();

        if queue_needs_frames && is_decoding_active {
            // Rate limit Demuxer if audio ringbuffer is too full (avoid blocking)
            if audio_tx.is_full() {
                std::thread::sleep(std::time::Duration::from_millis(2));
                continue;
            }

            if let Some(packet_res) = packets_iter.next() {
                let (stream, packet) = packet_res;
                let stream_idx = stream.index();

                if stream_idx == video_stream_idx {
                    if let Ok(frames) = video_pipeline.process_packet(&packet) {
                        for frame in frames {
                            play_queue.push_back(frame);
                        }
                    }
                } else if Some(stream_idx) == audio_stream_idx_opt && audio_enabled {
                    if let Some(mut audio_pipeline) = audio_pipeline_opt.as_mut() {
                        if let Some(pts) = packet.pts() {
                            audio_pipeline.update_current_time_from_pts(pts);
                        }
                        if audio_pipeline.send_packet(&packet).is_ok() {
                            if let Ok(chunks) = audio_pipeline.receive_samples() {
                                let generation = master_clock.playback_generation();
                                for chunk in chunks {
                                    if let Some(tag) = chunk.tag {
                                        let _ = tags_tx.try_push(tag);
                                    }
                                    for &sample in &chunk.data {
                                        while audio_tx.try_push((sample, generation)).is_err() {
                                            std::thread::sleep(std::time::Duration::from_millis(1));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                // EOF, sleep a bit to avoid hotloop
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        } else {
            // Queue is full or paused: sleep briefly to prevent 100% CPU hot loop
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
```

- [ ] **Step 2: Verify compiling**

Run: `cargo check`
Expected: Compiles cleanly.

- [ ] **Step 3: Run unit tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/media/mod.rs
git commit -m "feat: implement lookahead pre-decoding queue in MediaEngine"
```

---

### Task 4: Verify GOP boundary intervals in decode benchmark

**Files:**
- Test: [src/bin/bench_decode.rs](file:///Users/dselck/Documents/Snippy2/src/bin/bench_decode.rs)

- [ ] **Step 1: Run performance benchmarks**

Run:
```bash
SNIPPY_BENCH_VIDEO="/Users/dselck/Downloads/GOAT.2026.1080p.WEBRip.AAC5.1.10bits.x265-Rapta.mkv" cargo run --release --bin bench_decode
```

Expected:
- Benchmarks run successfully.
- Correctness passes.
- Under **GOP boundary intervals**, the maximum interval is significantly reduced (it should stay under 2× the median, and no spikes should be detected).
- Playback runs extremely smooth without stutters.
