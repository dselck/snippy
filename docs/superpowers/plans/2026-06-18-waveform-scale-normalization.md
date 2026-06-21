# Waveform Scale Normalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Normalize the audio loudness profile waveform scale dynamically using the maximum RMS value currently visible on the screen, and draw it from the bottom up to represent positive-only RMS values.

**Architecture:** Locate the rendering of the audio loudness profile bars in `src/ui/mod.rs`. Perform a pre-pass to find the maximum amplitude among the buckets visible in the current viewport. Scale all visible amplitudes relative to this maximum, and draw the line segments from the bottom of the waveform bounding box upwards.

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Update Loudness Waveform Rendering

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Calculate the maximum visible RMS amplitude**

Add logic right before the rendering loop to calculate the maximum RMS amplitude among the visible buckets in the current viewport:

```rust
                    let first_visible_frame = start_frame.max(0) as usize;
                    let last_visible_frame = (start_frame + visible_range_width as isize)
                        .min(self.playback.total_frames() as isize) as usize;
                    let first_bucket = (first_visible_frame * 4).min(total_buckets);
                    let last_bucket = (last_visible_frame * 4 + 1).min(total_buckets);
                    let max_amplitude = if first_bucket < last_bucket {
                        loudness[first_bucket..last_bucket]
                            .iter()
                            .filter_map(|&amp_opt| amp_opt)
                            .fold(0.0_f32, |a, b| a.max(b))
                    } else {
                        0.0_f32;
                    };
                    let scale = if max_amplitude > 0.0 { max_amplitude } else { 1.0 };
```

- [ ] **Step 2: Scale amplitudes and draw waveform from the bottom up**

Modify the drawing code inside the rendering loop to scale the amplitude by the computed `scale`, and change the `line_segment` coordinates to start from `wave_rect.bottom() - 3.0` and draw upwards (subtracting `bar_h`):

```rust
                                let scaled_amplitude = amplitude / scale;
                                let bar_h = scaled_amplitude * (wave_rect.height() - 6.0);
                                let x = wave_rect.left() + i as f32 * bar_width;

                                // ... other code ...

                                painter.line_segment(
                                    [
                                        egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0),
                                        egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0 - bar_h),
                                    ],
                                    egui::Stroke::new(bar_width, bar_color),
                                );
```

- [ ] **Step 3: Build the application to verify compilation**

Run: `cargo check`
Expected: Compilation passes with no new errors.

- [ ] **Step 4: Run the test suite**

Run: `cargo test`
Expected: All tests pass.
