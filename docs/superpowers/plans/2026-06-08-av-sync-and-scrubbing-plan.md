# Audio-Video Sync & Smooth Scrubbing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement synchronous seek handoff, prioritized audio commands, and a reduced audio ring buffer size to resolve all audio-video desync and ensure buttery smooth scrubbing.

**Architecture:** Change `AudioThreadCommand::Seek` to be synchronous using a response channel. Have the demuxer thread block on both video and audio threads flushing before seeking. Prioritize checking the command channel in the audio thread. Reduce the CPAL ring buffer size to 200ms.

**Tech Stack:** Rust, FFmpeg, CPAL

---

### Task 1: Update AudioThreadCommand & Demuxer Seek in `src/media/mod.rs`

**Files:**
- Modify: `src/media/mod.rs` (file:///Users/dselck/Documents/Snippy2/src/media/mod.rs)

- [x] **Step 1: Update `AudioThreadCommand` enum**
  Modify `AudioThreadCommand` definition around line 90:
  ```rust
  pub enum AudioThreadCommand {
      Seek { target_time: f64, resp_tx: Sender<()> },
  }
  ```

- [x] **Step 2: Update seek handling in main demuxer loop**
  Modify the seek handling block inside `run_decoding_loop` around lines 550-590:
  ```rust
          if let Some((frame_index, keyframe_only, sequence_id)) = seek_requested.take() {
              let (v_resp_tx, v_resp_rx) = crossbeam_channel::bounded::<bool>(1);
              let _ = video_cmd_tx.send(VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, resp_tx: v_resp_tx });

              let target_time = frame_index as f64 / fps;
              let (a_resp_tx, a_resp_rx) = crossbeam_channel::bounded::<()>(1);
              let _ = audio_cmd_tx.send(AudioThreadCommand::Seek { target_time, resp_tx: a_resp_tx });

              // Block on both flushes
              let _ = v_resp_rx.recv();
              let _ = a_resp_rx.recv();

              pending_packet = None; // Clear any pending packet on seek
              
              // Perform seek
              drop(packets_iter);
              let seek_pts = ((video_start_time + frame_index as f64 / fps) * 1_000_000.0) as i64;
              let _ = input.seek(seek_pts, ..seek_pts + 1);

              // Recreate packets_iter
              packets_iter = input.packets();
              continue;
          }
  ```

- [x] **Step 3: Update seek handling at end-of-stream in demuxer loop**
  Modify the seek handling block inside the end-of-stream check around lines 600-630:
  ```rust
                          if let Some((frame_index, keyframe_only, sequence_id)) = seek_requested.take() {
                              let (v_resp_tx, v_resp_rx) = crossbeam_channel::bounded::<bool>(1);
                              let _ = video_cmd_tx.send(VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, resp_tx: v_resp_tx });

                              let target_time = frame_index as f64 / fps;
                              let (a_resp_tx, a_resp_rx) = crossbeam_channel::bounded::<()>(1);
                              let _ = audio_cmd_tx.send(AudioThreadCommand::Seek { target_time, resp_tx: a_resp_tx });

                              let _ = v_resp_rx.recv();
                              let _ = a_resp_rx.recv();

                              drop(packets_iter);
                              let seek_pts = ((video_start_time + frame_index as f64 / fps) * 1_000_000.0) as i64;
                              let _ = input.seek(seek_pts, ..seek_pts + 1);
                              
                              packets_iter = input.packets();
                          }
  ```

- [x] **Step 4: Run `cargo check` to verify**
  Verify compilation. It should fail only on the audio thread implementation.

---

### Task 2: Implement Audio Thread Prioritization and Flush Acknowledgment

**Files:**
- Modify: `src/media/mod.rs` (file:///Users/dselck/Documents/Snippy2/src/media/mod.rs)

- [x] **Step 1: Update audio thread loops to prioritize seek commands**
  Modify the audio thread loop around lines 454-572:
  1. Add prioritized command checking at the start of the `loop`:
     ```rust
                 if let Ok(cmd) = audio_cmd_rx.try_recv() {
                     match cmd {
                         AudioThreadCommand::Seek { target_time, resp_tx } => {
                             while audio_packet_rx.try_recv().is_ok() {}
                             audio_decoder.flush();
                             resampler = None;
                             skip_until_time = target_time;
                             is_first_audio_frame = true;
                             current_audio_time = video_start_time + target_time;
                             playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                             let _ = resp_tx.send(());
                             continue;
                         }
                     }
                 }
     ```
  2. Add command checking inside the `try_push` block (lines 482-500):
     ```rust
                                                     while audio_tx.try_push(sample).is_err() {
                                                         if let Ok(cmd) = audio_cmd_rx.try_recv() {
                                                             match cmd {
                                                                 AudioThreadCommand::Seek { target_time, resp_tx } => {
                                                                     while audio_packet_rx.try_recv().is_ok() {}
                                                                     audio_decoder.flush();
                                                                     resampler = None;
                                                                     skip_until_time = target_time;
                                                                     is_first_audio_frame = true;
                                                                     current_audio_time = video_start_time + target_time;
                                                                     playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                                                                     let _ = resp_tx.send(());
                                                                     seek_aborted = true;
                                                                     break;
                                                                 }
                                                             }
                                                         }
                                                         thread::sleep(std::time::Duration::from_millis(5));
                                                     }
     ```
  3. Add check once per decoded frame inside the `receive_frame` loop:
     ```rust
                                     let mut seek_aborted = false;
                                     let mut frame = ffmpeg_next::util::frame::Audio::empty();
                                     while audio_decoder.receive_frame(&mut frame).is_ok() {
                                         if let Ok(cmd) = audio_cmd_rx.try_recv() {
                                             match cmd {
                                                 AudioThreadCommand::Seek { target_time, resp_tx } => {
                                                     while audio_packet_rx.try_recv().is_ok() {}
                                                     audio_decoder.flush();
                                                     resampler = None;
                                                     skip_until_time = target_time;
                                                     is_first_audio_frame = true;
                                                     current_audio_time = video_start_time + target_time;
                                                     playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                                                     let _ = resp_tx.send(());
                                                     seek_aborted = true;
                                                     break;
                                                 }
                                             }
                                         }
     ```
  4. Update the `recv(audio_cmd_rx)` arm inside `select!`:
     ```rust
                     recv(audio_cmd_rx) -> cmd_res => {
                         match cmd_res {
                             Ok(AudioThreadCommand::Seek { target_time, resp_tx }) => {
                                 while audio_packet_rx.try_recv().is_ok() {}
                                 audio_decoder.flush();
                                 resampler = None;
                                 skip_until_time = target_time;
                                 is_first_audio_frame = true;
                                 current_audio_time = video_start_time + target_time;
                                 playback_state.should_flush.store(true, std::sync::atomic::Ordering::SeqCst);
                                 let _ = resp_tx.send(());
                             }
                             Err(_) => break,
                         }
                     }
     ```

- [x] **Step 2: Run `cargo check` to verify**
  Run: `cargo check`
  Expected: PASS

---

### Task 3: Reduce Audio Ring Buffer Size in `src/audio/mod.rs`

**Files:**
- Modify: `src/audio/mod.rs` (file:///Users/dselck/Documents/Snippy2/src/audio/mod.rs)

- [x] **Step 1: Reduce ring buffer size to 200ms**
  Modify line 78 in `src/audio/mod.rs`:
  ```rust
  let rb = SharedRb::<Heap<f32>>::new((sample_rate as usize * channels) / 5);
  ```

- [x] **Step 2: Run `cargo check` to verify**
  Run: `cargo check`
  Expected: PASS

- [x] **Step 3: Run integration test to verify seek behavior**
  Run: `cargo run --bin test_playback_sync`
  Expected: PASS

- [x] **Step 4: Commit all changes**
  Run:
  ```bash
  git add src/media/mod.rs src/audio/mod.rs
  git commit -m "fix: implement synchronous seek handoff and reduce audio buffer to 200ms for perfect AV sync"
  ```
