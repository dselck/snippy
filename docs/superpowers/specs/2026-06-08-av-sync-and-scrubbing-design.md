# Audio-Video Synchronization & Smooth Scrubbing Design

This document describes the design for achieving race-free audio-video synchronization and buttery smooth scrubbing in Snippy2 using absolute microsecond sample tagging.

---

## 1. Problem Statement
The current implementation suffers from temporary and permanent audio-video desync on seeks. This is caused by:
1. **Asynchronous Seek Race Conditions**: The demuxer thread seeks and sends new packets to the audio/video threads *before* the audio thread has processed its seek command. This causes the audio thread to discard the newly seeked packets during its flush step.
2. **Buffer Backpressure and Latency**: A large `2-second` CPAL ring buffer causes the audio thread to decode far ahead of the current frame when paused or scrubbing, wasting CPU and making scrubbing laggy.
3. **Stale Samples and Blind Clock**: Pausing and playing leaves stale samples in the ring buffer. The clock is blind and simply increments a counter, making it impossible to guarantee that the audio currently heard matches the video frame on screen.

---

## 2. Proposed Changes

### A. Synchronous Seek Handoff
To eliminate the seek race condition, the seek lifecycle will become synchronous across all threads. The demuxer thread will wait for both video and audio threads to complete their flushes before seeking the file container:

1. **Audio Command Response**: Update `AudioThreadCommand::Seek` to include a response channel:
   ```rust
   pub enum AudioThreadCommand {
       Seek {
           target_time: f64,
           resp_tx: crossbeam_channel::Sender<()>,
       },
   }
   ```
2. **Demuxer Sync Handoff**: On seek, the demuxer thread sends seek commands to both video and audio threads and blocks on both:
   ```rust
   let (v_resp_tx, v_resp_rx) = crossbeam_channel::bounded::<bool>(1);
   let _ = video_cmd_tx.send(VideoThreadCommand::Seek { frame_index, keyframe_only, sequence_id, resp_tx: v_resp_tx });

   let target_time = frame_index as f64 / fps;
   let (a_resp_tx, a_resp_rx) = crossbeam_channel::bounded::<()>(1);
   let _ = audio_cmd_tx.send(AudioThreadCommand::Seek { target_time, resp_tx: a_resp_tx });

   let _ = v_resp_rx.recv();
   let _ = a_resp_rx.recv();
   ```

### B. Microsecond Tagged-Sample Clock (Master Truth)
Instead of a blind count-based clock, we will use a lock-free secondary tags queue to tag the audio stream with its absolute container PTS in microseconds.

1. **Master Clock Update**:
   Update `AudioPlaybackState` to store the master clock in microseconds (`current_position_us: Arc<AtomicU64>`). `current_time()` will return `current_position_us / 1_000_000.0`.
2. **Audio Thread Tagging**:
   The audio thread will track the total number of audio frames written (`frames_written_total: u64`). Before pushing resampled samples for a decoded frame, it will push a tag `(frames_written_total, pts_us)` to a new tags ring buffer.
3. **CPAL Sync Verification**:
   The CPAL callback will track the total number of audio frames read (`samples_read_total: u64`). When it processes popped samples, it will consume tags from the queue. For the last tag where `tag_frame_idx <= samples_read_total`, it will update the master clock:
   ```rust
   let elapsed_samples = samples_read_total - tag_frame_idx;
   let elapsed_us = (elapsed_samples * 1_000_000) / sample_rate;
   current_position_us.store(tag_pts_us + elapsed_us, Ordering::Relaxed);
   ```

### C. Ring Buffer Size Reduction
Reduce the CPAL ring buffer size from `2 seconds` to `200ms` (`(sample_rate as usize * channels) / 5`).
* Draining and flushing the buffer becomes 10 times faster.
* During paused state or scrubbing, the audio thread will only decode 200ms ahead, preserving CPU resources and making scrubbing buttery smooth.

### D. Pause-For-Sync and Play Transitions
On seek/reset, the UI thread sets `audio_paused_for_sync = true` and resets the fallback clock to `None`.
* While `audio_paused_for_sync` is `true`, the CPAL callback plays silence, does not consume samples, and does not increment the clock.
* Once the UI thread receives and displays the seek target video frame, it sets `audio_paused_for_sync = false` and resumes fallback clock advancement (if playing).
