# Timeline and Waveform Refinements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refine the video timeline and audio waveform visuals (removing the filmstrip line indicator, removing all audio waveform indicators/interaction), and restore timeline clickability.

**Architecture:** 
1. Replace the solid playhead line and circle on the filmstrip with a pink triangle pointing down at the top edge.
2. Disable interaction on the audio waveform (changing Sense to hover), remove highlights (queued ranges, active selection), remove playhead indicators, and simplify bar color mapping to a neutral indigo.
3. Fix click-to-seek functionality in both the filmstrip viewport and the progress bar by falling back to `ui.input(|i| i.pointer.hover_pos())` if `interact_pointer_pos()` is `None` on click release.

**Tech Stack:** Rust, `egui` GUI framework.

---

### Task 1: Refine Filmstrip Playhead Indicator

**Files:**
- Modify: `src/ui/mod.rs`

- [x] **Step 1: Replace playhead line and circle with a triangle pointing down**
  Edit `src/ui/mod.rs` to replace the vertical playhead line segment and circle with a convex polygon triangle pointing down.

  ```rust
                    // Playhead Triangle pointing down at the top of the filmstrip
                    let center_x = playhead_x;
                    let p1 = egui::pos2(center_x - 6.0, filmstrip_rect.top() - 2.0);
                    let p2 = egui::pos2(center_x + 6.0, filmstrip_rect.top() - 2.0);
                    let p3 = egui::pos2(center_x, filmstrip_rect.top() + 6.0);
                    painter.add(egui::Shape::convex_polygon(
                        vec![p1, p2, p3],
                        Color32::from_rgb(236, 72, 153),
                        egui::Stroke::NONE,
                    ));
  ```

- [x] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 2: Simplify Audio Waveform (Remove Interaction & Indicators)

**Files:**
- Modify: `src/ui/mod.rs`

- [x] **Step 1: Make audio waveform non-interactable**
  Update the allocation of `wave_rect` and `wave_response` to use `egui::Sense::hover()` instead of `egui::Sense::click_and_drag()`, and remove the hover text.

  ```rust
                    // 3. Compact Synced Waveform Component
                    let wave_height = 24.0;
                    let (wave_rect, wave_response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), wave_height),
                        egui::Sense::hover(),
                    );
  ```

- [x] **Step 2: Simplify bar color mapping**
  Replace the complex color mapping (which highlights active selection, queued ranges, keep/cut modes) with a simple check for out-of-bounds, using premium indigo for active frames.

  ```rust
                                let bar_color = if out_of_bounds {
                                    Color32::from_rgb(20, 19, 32)
                                } else {
                                    Color32::from_rgb(99, 102, 241) // Premium Indigo accent color
                                };

                                painter.line_segment(
                                    [
                                        egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0),
                                        egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0 - bar_h),
                                    ],
                                    egui::Stroke::new(bar_width, bar_color),
                                );
  ```

- [x] **Step 3: Remove selection, range, playhead highlights and drag/click handlers**
  Delete:
  - Highlights for queued ranges inside the visible window.
  - Highlights for the selected range inside the visible window.
  - Playhead vertical line and circle in the synced waveform.
  - Waveform drag and click input handling logic at the end of the waveform block.

- [x] **Step 4: Verify compilation**
  Run: `cargo check`
  Expected: PASS

---

### Task 3: Restore Clickability to Filmstrip and Progress Bar Timelines

**Files:**
- Modify: `src/ui/mod.rs`

- [x] **Step 1: Update filmstrip click-to-seek logic**
  Use `.or_else(|| ui.input(|i| i.pointer.hover_pos()))` on `filmstrip_response.interact_pointer_pos()` to ensure pointer coordinates are retrieved on mouse release / click.

  ```rust
                    if filmstrip_response.dragged() || filmstrip_response.clicked() {
                        self.playback.set_playing(false);
                        if let Some(pointer_pos) = filmstrip_response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos())) {
                            let dx_pixels = pointer_pos.x - playhead_x as f32;
                            let dt_seconds = dx_pixels as f64 / pixels_per_second;
                            let target_time = current_time_sec + dt_seconds;
                            let target_frame = (target_time * fps).round().max(0.0).min(total_frames as f64 - 1.0) as usize;
                            self.playback.seek_internal(target_frame, false, true);
                        }
                    }
  ```

- [x] **Step 2: Update progress bar click-to-seek logic**
  Apply the same fallback to the progress bar interaction logic.

  ```rust
        if response.drag_started() || response.clicked() {
            self.playback.handle_input(media::InputEvent::ScrubDragStart);
        }
        if (response.dragged() || response.clicked())
            && let Some(mouse_pos) = response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos())) {
                let rel_x = (mouse_pos.x - rect.left()).clamp(0.0, rect.width());
                let pct = rel_x / rect.width();
                let target_frame = (pct * self.playback.total_frames() as f32) as usize;
                self.playback.handle_input(media::InputEvent::ScrubDragUpdate { target_frame });
            }
        if response.drag_stopped() || response.clicked() {
            self.playback.handle_input(media::InputEvent::ScrubDragEnd);
        }
  ```

- [x] **Step 3: Run full verification suite**
  Run: `cargo test`
  Expected: PASS
