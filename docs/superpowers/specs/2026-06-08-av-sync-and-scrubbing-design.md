# Audio-Video Synchronization & Smooth Scrubbing Design

This document describes the design for achieving race-free audio-video synchronization and buttery smooth scrubbing in Snippy2.

---

## 1. Problem Statement
The current implementation suffers from temporary and permanent audio-video desync on seeks. This is caused by:
1. **Asynchronous Seek Race Conditions**: The demuxer thread seeks and sends new packets to the audio/video threads *before* the audio thread has processed its seek command. This causes the audio thread to discard the newly seeked packets during its flush step.
2. **Buffer Backpressure and Latency**: A large `2-second` CPAL ring buffer causes the audio thread to decode far ahead of the current frame when paused or scrubbing, wasting CPU and making scrubbing laggy.
3. **Transition Audio Stutters**: Pausing and playing leaves stale samples in the ring buffer, which play on resume.

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

### B. Prioritized Audio Thread Seek
The audio thread will prioritize command checking over packet decoding and sample pushing to respond to seeks instantly:
* Check `audio_cmd_rx` at the top of the loop using `try_recv()`.
* Check `audio_cmd_rx` once per decoded frame inside `receive_frame` to abort long-running decoding/pushing immediately.
* Return the flush acknowledgment over `resp_tx` only after the packet queue is drained and the decoder is flushed.

### C. Ring Buffer Size Reduction
Reduce the CPAL ring buffer size from `2 seconds` to `200ms` (`(sample_rate as usize * channels) / 5`).
* Draining and flushing the buffer becomes 10 times faster.
* During paused state or scrubbing, the audio thread will only decode 200ms ahead, preserving CPU resources and making scrubbing buttery smooth.

### D. Play/Pause Transition Flush
On play/pause transitions, `playback_state.should_flush` is set to `true` to ensure the CPAL callback drains any stale samples in the ring buffer, starting playback cleanly.
