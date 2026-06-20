# Waveform Rendering Refinements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Triple the visual height of the waveform viewer and fix the "phasing" (aliasing) issue when zoomed out.

**Architecture:** 
1. Update `wave_height = 72.0;` in `src/ui/mod.rs`.
2. Modify the drawing loop in `src/ui/mod.rs` to calculate the start and end bucket index range represented by each pixel column, find the maximum amplitude in that range (peak decimation), and use the center frame index of the range to check if a frame is cut.

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Refine Waveform Height and Rendering Logic

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Update waveform height and implement peak decimation**
  Edit `src/ui/mod.rs` around line 1431 to set `wave_height = 72.0;`.
  Replace the drawing loop:

  ```rust
                      let wave_height = 72.0;
  ```

  And update the synced frame calculations and bucket loop:

  ```rust
                      // Synced frame calculations matching filmstrip
                      let total_width_d = total_width as f64;
                      let visible_duration = total_width_d / pixels_per_second;
                      let visible_range_width_d = visible_duration * fps;
                      let start_frame_d = (current_time_sec - (total_width_d * 0.5) / pixels_per_second) * fps;

                      // Render real loudness profile bars — one bar per pixel column
                      let loudness = self.playback.loudness_profile();
                      let total_buckets = loudness.len();
                      let bar_count = wave_rect.width() as usize;
                      let bar_width = 1.0_f32;

                      let total_frames = self.playback.total_frames();
                      let db_floor = self.settings.playback.waveform_db_floor;

                      for i in 0..bar_count {
                          let pct_start = i as f64 / bar_count as f64;
                          let pct_end = (i + 1) as f64 / bar_count as f64;

                          let frame_start_d = start_frame_d + pct_start * visible_range_width_d;
                          let frame_end_d = start_frame_d + pct_end * visible_range_width_d;

                          // Use center of column for frame cut check
                          let frame_center_idx = (frame_start_d + frame_end_d) * 0.5;
                          let frame_idx_isize = frame_center_idx as isize;

                          let out_of_bounds = frame_idx_isize < 0 || frame_idx_isize >= total_frames as isize;

                          let mut amplitude_opt: Option<f32> = None;

                          if !out_of_bounds {
                              let b_start_d = frame_start_d * 4.0;
                              let b_end_d = frame_end_d * 4.0;

                              let b_start = b_start_d.floor() as isize;
                              let b_end = b_end_d.ceil() as isize;

                              let b_start_clamped = b_start.max(0) as usize;
                              let b_end_clamped = b_end.max(0) as usize;

                              if b_start < total_buckets as isize && b_end >= 0 {
                                  let limit_start = b_start_clamped.min(total_buckets);
                                  let limit_end = b_end_clamped.max(limit_start + 1).min(total_buckets);

                                  let mut max_amp: Option<f32> = None;
                                  for bi in limit_start..limit_end {
                                      if let Some(Some(amp)) = loudness.get(bi) {
                                          max_amp = Some(max_amp.map_or(*amp, |m| m.max(*amp)));
                                      }
                                  }
                                  amplitude_opt = max_amp;
                              }
                          }

                          if let Some(amplitude) = amplitude_opt {
                              let scaled_amplitude = if db_floor >= 0.0 {
                                  amplitude
                              } else if amplitude <= 0.0 {
                                  0.0
                              } else {
                                  let db = 20.0 * amplitude.log10();
                                  let pct = (db - db_floor) / -db_floor;
                                  pct.clamp(0.0, 1.0)
                              };

                              let bar_h = scaled_amplitude * (wave_rect.height() - 6.0);
                              let x = wave_rect.left() + i as f32 * bar_width;

                              let is_cut = if out_of_bounds {
                                  false
                              } else {
                                  self.is_frame_cut(frame_idx_isize as usize)
                              };

                              let bar_color = if out_of_bounds {
                                  Color32::from_rgb(20, 19, 32)
                              } else if is_cut {
                                  Color32::from_rgba_unmultiplied(99, 102, 241, 40)
                              } else {
                                  Color32::from_rgb(99, 102, 241)
                              };

                              painter.line_segment(
                                  [
                                      egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0),
                                      egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0 - bar_h),
                                  ],
                                  egui::Stroke::new(bar_width, bar_color),
                              );
                          }
                      }
  ```

- [ ] **Step 2: Run cargo check to verify compilation**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Run cargo test to verify all tests pass**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 4: Commit changes to git**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: triple waveform height and implement peak decimation rendering"
  ```
