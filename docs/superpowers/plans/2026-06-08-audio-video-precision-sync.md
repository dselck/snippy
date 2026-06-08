# Ground-Truth Audio-Video Precision Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve fixed audio-video seek desync, clock jumps, and master clock stalls by implementing sample-accurate trimming/padding in the audio thread, a robust clock update policy in the CPAL callback, and ground-truth timestamp-based frame selection in the UI.

**Architecture:** 
1. **Audio Thread:** Coarsely discard pre-seek audio frames, then resample and trim/pad the first audio frame to match the target stream time exactly. Tags carry precise start timestamps.
2. **CPAL Callback:** Only update fallback atomic references when samples are actually consumed (`written > 0`), ensuring fallback CPU clock continuity during underruns or EOF.
3. **UI Thread:** Render frames based on `pts_sec` matching `master_time` directly, eliminating average FPS checks.

**Tech Stack:** Rust, ffmpeg-next, cpal, ringbuf, egui.

---

### Task 1: CPAL Callback Stall Fix

**Files:**
- Modify: `src/audio/mod.rs` (around lines 152-167)

- [ ] **Step 1: Wrap atomic updates in CPAL callback**
  In `src/audio/mod.rs`, modify the fallback updates block in the output callback to only run when `written > 0`:
  ```rust
                  if active {
                      let current_pos_us = if let Some((tag_idx, tag_pts_us)) = last_tag {
                          let elapsed_samples = samples_read_total.saturating_sub(tag_idx);
                          let elapsed_us = (elapsed_samples * 1_000_000) / (sample_rate as u64);
                          tag_pts_us + elapsed_us
                      } else {
                          let elapsed_us = (written as u64 * 1_000_000) / (sample_rate as u64);
                          current_position_us_callback.load(Ordering::Relaxed) + elapsed_us
                      };

                      // Only update the master/fallback clock if samples were actually popped and played!
                      if written > 0 {
                          current_position_us_callback.store(current_pos_us, Ordering::Relaxed);

                          // Update fallback clock reference atomically
                          fallback_base_us_callback.store(current_pos_us, Ordering::Relaxed);
                          let now_us = fallback_epoch_callback.elapsed().as_micros() as u64;
                          fallback_start_us_callback.store(now_us, Ordering::Relaxed);
                      }
                  }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/audio/mod.rs
  git commit -m "fix: only update master/fallback clock in CPAL callback when samples are played"
  ```

---

### Task 2: Sample-Accurate Audio Trimming & Padding

**Files:**
- Modify: `src/media/mod.rs` (around lines 494-558)

- [ ] **Step 1: Implement precise trimming and padding on seek**
  In `src/media/mod.rs`'s audio decoder thread loop, replace the first-frame discarding and resampling loop with the following precise implementation:
  ```rust
                                          if is_first_audio_frame {
                                              let target_container_time = video_start_time + skip_until_time;
                                              if pts_sec + duration_sec <= target_container_time {
                                                  continue; // Discard whole frame before seek target
                                              }
                                          }

                                          let r = match resampler {
                                              Some(ref mut r) if r.input().format == frame.format()
                                                  && r.input().channel_layout == frame.channel_layout()
                                                  && r.input().rate == frame.rate() => r,
                                              _ => {
                                                  let target_layout = if target_channels == 1 {
                                                      ChannelLayout::MONO
                                                  } else {
                                                      ChannelLayout::STEREO
                                                  };
                                                  
                                                  let new_r = ResamplerContext::get(
                                                      frame.format(),
                                                      frame.channel_layout(),
                                                      frame.rate(),
                                                      Sample::F32(ffmpeg_next::format::sample::Type::Packed),
                                                      target_layout,
                                                      target_sample_rate,
                                                  ).unwrap();
                                                  resampler = Some(new_r);
                                                  resampler.as_mut().unwrap()
                                              }
                                          };

                                          let mut resampled = ffmpeg_next::util::frame::Audio::empty();
                                          if r.run(&frame, &mut resampled).is_ok() {
                                              let sample_count = resampled.samples();
                                              if sample_count > 0 {
                                                  let byte_count = sample_count * target_channels * 4;
                                                  let mut data: &[f32] = bytemuck::cast_slice(&resampled.data(0)[..byte_count]);
                                                  
                                                  if is_first_audio_frame {
                                                      let target_container_time = video_start_time + skip_until_time;
                                                      if pts_sec < target_container_time {
                                                          // Trim overlap
                                                          let overlap_sec = target_container_time - pts_sec;
                                                          let discard_samples = (overlap_sec * target_sample_rate as f64).round() as usize;
                                                          if discard_samples < data.len() {
                                                              data = &data[discard_samples..];
                                                              let pts_us = (skip_until_time.max(0.0) * 1_000_000.0) as u64;
                                                              let _ = tags_tx.try_push((frames_written_total, pts_us));
                                                          } else {
                                                              continue;
                                                          }
                                                      } else {
                                                          // Gap: Pad with silence or handle large gap
                                                          let gap_sec = pts_sec - target_container_time;
                                                          if gap_sec <= 0.15 {
                                                              let pad_samples = (gap_sec * target_sample_rate as f64).round() as usize;
                                                              let pts_us = (skip_until_time.max(0.0) * 1_000_000.0) as u64;
                                                              let _ = tags_tx.try_push((frames_written_total, pts_us));
                                                              
                                                              for _ in 0..pad_samples {
                                                                  while audio_tx.try_push(0.0).is_err() {
                                                                      if let Ok(cmd) = audio_cmd_rx.try_recv() {
                                                                          match cmd {
                                                                              AudioThreadCommand::Seek { target_time, resp_tx } => {
                                                                                  handle_audio_seek!(target_time, resp_tx);
                                                                                  seek_aborted = true;
                                                                                  break;
                                                                              }
                                                                          }
                                                                      }
                                                                      thread::sleep(std::time::Duration::from_millis(5));
                                                                  }
                                                                  if seek_aborted {
                                                                      break;
                                                                  }
                                                              }
                                                              if seek_aborted {
                                                                  break;
                                                              }
                                                              frames_written_total += pad_samples as u64;
                                                          } else {
                                                              // Large Gap: No padding, tag at actual PTS
                                                              let pts_us = ((pts_sec - video_start_time).max(0.0) * 1_000_000.0) as u64;
                                                              let _ = tags_tx.try_push((frames_written_total, pts_us));
                                                          }
                                                      }
                                                      is_first_audio_frame = false;
                                                  } else {
                                                      let pts_us = ((pts_sec - video_start_time).max(0.0) * 1_000_000.0) as u64;
                                                      let _ = tags_tx.try_push((frames_written_total, pts_us));
                                                  }

                                                  for &sample in data {
                                                      while audio_tx.try_push(sample).is_err() {
                                                          if let Ok(cmd) = audio_cmd_rx.try_recv() {
                                                              match cmd {
                                                                  AudioThreadCommand::Seek { target_time, resp_tx } => {
                                                                      handle_audio_seek!(target_time, resp_tx);
                                                                      seek_aborted = true;
                                                                      break;
                                                                  }
                                                              }
                                                          }
                                                          thread::sleep(std::time::Duration::from_millis(5));
                                                      }
                                                      if seek_aborted {
                                                          break;
                                                      }
                                                  }
                                                  if !seek_aborted {
                                                      frames_written_total += data.len() as u64;
                                                  }
                                              }
                                          }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Commit**
  ```bash
  git add src/media/mod.rs
  git commit -m "fix: implement sample-accurate audio trimming and silence padding post-seek"
  ```

---

### Task 3: Ground-Truth Video Selection and Catch-up

**Files:**
- Modify: `src/ui/mod.rs` (around lines 817-868)

- [ ] **Step 1: Replace index-based promotion with direct timestamp check**
  In `src/ui/mod.rs`'s `update` method, refactor the background frame processing, catch-up check, and `current_frame` update to compare timestamps (`pts_sec()` and `master_time`):
  ```rust
          // 1. Process background frames
          if let Some(ref rx) = self.frame_rx {
              let master_time = self.playback_state.current_time();
              
              // Promote future frame if it's now ready and not stale
              if let Some(ref f) = self.future_frame {
                  if f.sequence_id() < self.latest_sequence_id {
                      self.stale_discards_count += 1;
                      self.future_frame = None;
                  } else if f.pts_sec() <= master_time {
                      self.latest_frame = self.future_frame.take();
                  }
              }

              // Receive new frames from channel
              if self.future_frame.is_none() {
                  while let Ok(frame) = rx.try_recv() {
                      if frame.sequence_id() < self.latest_sequence_id {
                          self.stale_discards_count += 1;
                          continue; // Discard stale frame!
                      }
                      if frame.pts_sec() <= master_time {
                          self.latest_frame = Some(frame);
                      } else {
                          self.future_frame = Some(frame);
                          break;
                      }
                  }
              }

              if self.playback_state.audio_paused_for_sync.load(std::sync::atomic::Ordering::SeqCst) {
                  let mut caught_up = false;
                  if let Some(ref f) = self.latest_frame {
                      if f.sequence_id() == self.latest_sequence_id {
                          if self.ui_state == PlaybackState::Seeking {
                              caught_up = false; // Ignore catch-up in seeking state
                          } else {
                              if f.pts_sec() >= master_time || f.frame_idx() >= self.total_frames.saturating_sub(1) {
                                  caught_up = true;
                              }
                          }
                      }
                  }
                  if caught_up {
                      self.seek_in_progress = false;
                      if self.ui_state == PlaybackState::Playing {
                          self.playback_state.audio_paused_for_sync.store(false, std::sync::atomic::Ordering::SeqCst);
                          self.playback_state.start_fallback_clock();
                      }
                  }
              }

              ctx.request_repaint();
          }
  ```

- [ ] **Step 2: Update current frame index from displaying frame**
  Replace lines 807-811 in `src/ui/mod.rs`:
  ```rust
              self.current_frame = (self.playback_state.current_time() * self.video_fps) as usize;
              if self.current_frame >= self.total_frames {
                  self.current_frame = self.total_frames;
                  self.set_playing(false);
              }
  ```
  with:
  ```rust
              let master_time = self.playback_state.current_time();
              let calculated_frame = (master_time * self.video_fps) as usize;
              self.current_frame = self.latest_frame.as_ref()
                  .filter(|f| f.sequence_id() == self.latest_sequence_id)
                  .map_or(calculated_frame, |f| f.frame_idx());
              if self.current_frame >= self.total_frames {
                  self.current_frame = self.total_frames;
                  self.set_playing(false);
              }
  ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "fix: use ground-truth pts_sec for video frame selection and catch-up check"
  ```

---

### Task 4: Integration Verification

- [ ] **Step 1: Run sync verification test**
  Run: `cargo run --bin test_playback_sync`
  Expected: Compilation passes and stdout prints "=== All Tests Passed Successfully ==="

- [ ] **Step 2: Run cargo tests**
  Run: `cargo test`
  Expected: All tests PASS
