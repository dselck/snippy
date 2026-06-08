# Filmstrip Timeline & Compact Waveform Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the timeline panel into a centered, zoomable sprocket filmstrip (top) and a compact full-overview waveform scrub area (bottom).

**Architecture:**
- Add a `zoom_level: f32` state field to `MainApp`.
- Implement Zoom controls (slider + buttons) in the time indicator row.
- Render 7 thumbnails in the filmstrip. Zoom dynamically changes the frame delta (step) between thumbnails from `1` (max zoom) to `total_frames / 6` (min zoom) using exponential interpolation.
- Render filmstrip top and bottom sprocket holes using `egui::Painter`.
- Handle thumbnail click interaction to jump to that frame.
- Shrink the existing waveform scrub layout into a smaller `24.0` height timeline overview block.

**Tech Stack:** Rust, egui

---

### Task 1: Add zoom_level field to MainApp State

**Files:**
- Modify: `src/ui/mod.rs:26-50`
- Modify: `src/ui/mod.rs:50-83`

- [ ] **Step 1: Add `zoom_level` field to `MainApp`**

Extend `MainApp` struct definition to include `zoom_level`:

```rust
    // Mock Audio Loudness Profile (for visual layout)
    pub loudness_profile: Vec<f32>,

    // Filmstrip Zoom Level (0.0 = fully zoomed out, 1.0 = fully zoomed in)
    pub zoom_level: f32,
}
```

- [ ] **Step 2: Initialize `zoom_level` in `MainApp::new`**

Add `zoom_level: 0.0` at the end of the `Self` initializer:

```rust
            cut_mode: false,
            frame_rx: None,
            latest_frame: None,
            loudness_profile: loudness,
            zoom_level: 0.0,
        }
    }
```

- [ ] **Step 3: Verify with cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: add zoom_level field to MainApp struct"
```

---

### Task 2: Replace Timeline and Waveform Deck UI Layout

**Files:**
- Modify: `src/ui/mod.rs:623-793`

- [ ] **Step 1: Rewrite BottomPanel timeline deck**

Replace the existing `timeline_deck` BottomPanel rendering block to draw the timecode + zoom header, sprocket filmstrip row, and compact waveform sequentially.

```rust
        // 2. Bottom Panel: Timeline and Waveform Deck (Full width)
        egui::TopBottomPanel::bottom("timeline_deck")
            .min_height(145.0)
            .frame(egui::Frame::none()
                .fill(Color32::from_rgb(8, 8, 14))
                .inner_margin(egui::Margin::symmetric(16.0, 12.0)))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Timeline Time Indicators & Zoom Controls
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(self.frame_to_time(self.current_frame))
                                .size(16.0)
                                .color(Color32::WHITE)
                        );
                        ui.label(format!("/ {}", self.frame_to_time(self.total_frames)));
                        
                        // Zoom controls aligned right
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Zoom In button (+)
                            if ui.small_button("+").on_hover_text("Zoom in filmstrip (show fewer frames)").clicked() {
                                self.zoom_level = (self.zoom_level + 0.1).clamp(0.0, 1.0);
                            }
                            
                            // Zoom slider
                            ui.add(egui::Slider::new(&mut self.zoom_level, 0.0..=1.0)
                                .show_value(false)
                                .text("Zoom"));

                            // Zoom Out button (-)
                            if ui.small_button("−").on_hover_text("Zoom out filmstrip (show more overview)").clicked() {
                                self.zoom_level = (self.zoom_level - 0.1).clamp(0.0, 1.0);
                            }

                            ui.add_space(8.0);
                            ui.label(format!("Frame {} / {}", self.current_frame, self.total_frames));
                        });
                    });

                    ui.add_space(4.0);

                    // 2. Sprocket Filmstrip Viewport
                    let filmstrip_height = 56.0;
                    let (filmstrip_rect, _response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), filmstrip_height),
                        egui::Sense::hover(),
                    );
                    
                    let painter = ui.painter_at(filmstrip_rect);
                    // Draw filmstrip track card background
                    painter.rect_filled(filmstrip_rect, 6.0, Color32::from_rgb(12, 11, 20));
                    painter.rect_stroke(filmstrip_rect, 6.0, egui::Stroke::new(1.0, Color32::from_rgb(26, 25, 43)));

                    let num_thumbnails = 7;
                    let num_gaps = num_thumbnails - 1;
                    let thumbnail_spacing = 6.0;
                    let sprocket_height = 8.0;
                    let inner_height = filmstrip_rect.height() - (sprocket_height * 2.0);
                    let inner_rect = egui::Rect::from_min_max(
                        egui::pos2(filmstrip_rect.left(), filmstrip_rect.top() + sprocket_height),
                        egui::pos2(filmstrip_rect.right(), filmstrip_rect.bottom() - sprocket_height),
                    );

                    let total_width = inner_rect.width();
                    let total_spacing = thumbnail_spacing * num_gaps as f32;
                    let thumbnail_width = (total_width - total_spacing) / num_thumbnails as f32;

                    // Draw sprockets on top and bottom margins
                    let sprocket_width = 8.0;
                    let sprocket_h = 5.0;
                    let num_sprockets = 15;
                    let sprocket_gap = (total_width - (sprocket_width * num_sprockets as f32)) / (num_sprockets - 1) as f32;
                    for s in 0..num_sprockets {
                        let sx = filmstrip_rect.left() + s as f32 * (sprocket_width + sprocket_gap);
                        
                        // Top sprocket hole
                        let top_sprocket_rect = egui::Rect::from_min_size(
                            egui::pos2(sx, filmstrip_rect.top() + (sprocket_height - sprocket_h) * 0.5),
                            egui::vec2(sprocket_width, sprocket_h),
                        );
                        painter.rect_filled(top_sprocket_rect, 1.0, Color32::from_rgb(24, 23, 40));

                        // Bottom sprocket hole
                        let bottom_sprocket_rect = egui::Rect::from_min_size(
                            egui::pos2(sx, filmstrip_rect.bottom() - (sprocket_height + sprocket_h) * 0.5),
                            egui::vec2(sprocket_width, sprocket_h),
                        );
                        painter.rect_filled(bottom_sprocket_rect, 1.0, Color32::from_rgb(24, 23, 40));
                    }

                    // Calculate frame delta step exponentially
                    let max_delta = (self.total_frames as f32 / num_gaps as f32).max(1.0);
                    let delta_f = (max_delta.ln() * (1.0 - self.zoom_level)).exp();
                    let delta = (delta_f as usize).max(1);

                    for i in 0..num_thumbnails {
                        let offset_idx = i as isize - (num_thumbnails / 2) as isize;
                        let target_frame = self.current_frame as isize + offset_idx * delta as isize;

                        let tx = inner_rect.left() + i as f32 * (thumbnail_width + thumbnail_spacing);
                        let thub_rect = egui::Rect::from_min_size(
                            egui::pos2(tx, inner_rect.top()),
                            egui::vec2(thumbnail_width, inner_height),
                        );

                        if target_frame >= 0 && target_frame <= self.total_frames as isize {
                            let is_center = offset_idx == 0;
                            
                            let bg_color = if is_center {
                                Color32::from_rgb(33, 31, 54)
                            } else {
                                Color32::from_rgb(18, 17, 28)
                            };
                            let border_color = if is_center {
                                Color32::from_rgb(236, 72, 153) // Electric pink border for center playhead
                            } else {
                                Color32::from_rgb(45, 43, 75)
                            };
                            painter.rect_filled(thub_rect, 4.0, bg_color);
                            painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(if is_center { 1.5 } else { 1.0 }, border_color));

                            // Color graphic procedural thumbnail
                            let hash_val = target_frame as usize;
                            let bar_color = Color32::from_rgb(
                                ((hash_val * 37) % 120 + 30) as u8,
                                ((hash_val * 59) % 120 + 30) as u8,
                                ((hash_val * 97) % 120 + 60) as u8,
                            );
                            let content_rect = egui::Rect::from_min_max(
                                egui::pos2(thub_rect.left() + 3.0, thub_rect.top() + 3.0),
                                egui::pos2(thub_rect.right() - 3.0, thub_rect.bottom() - 13.0),
                            );
                            painter.rect_filled(content_rect, 2.0, bar_color);

                            // Underlay text code (timecode)
                            let time_text = self.frame_to_time(target_frame as usize);
                            painter.text(
                                egui::pos2(thub_rect.center().x, thub_rect.bottom() - 6.0),
                                egui::Align2::CENTER_CENTER,
                                time_text,
                                egui::FontId::monospace(8.0),
                                Color32::from_rgb(148, 163, 184),
                            );

                            // Handle click interaction to center this frame
                            let response_id = ui.make_persistent_id(format!("filmstrip_th_{}", i));
                            let th_response = ui.interact(thub_rect, response_id, egui::Sense::click());
                            if th_response.clicked() {
                                self.current_frame = target_frame as usize;
                                self.is_playing = false; // Stop playback on seek
                            }
                        } else {
                            // Out of bounds thumbnail placeholder
                            painter.rect_filled(thub_rect, 4.0, Color32::from_rgb(8, 8, 14));
                            painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(20, 19, 32)));
                            
                            painter.text(
                                thub_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "OUT",
                                egui::FontId::proportional(9.0),
                                Color32::from_rgb(45, 43, 75),
                            );
                        }
                    }

                    // Vertical Center Playhead line over the filmstrip
                    let center_x = inner_rect.center().x;
                    painter.line_segment(
                        [egui::pos2(center_x, filmstrip_rect.top() - 2.0), egui::pos2(center_x, filmstrip_rect.bottom() + 2.0)],
                        egui::Stroke::new(2.0, Color32::from_rgb(236, 72, 153)),
                    );
                    painter.circle_filled(
                        egui::pos2(center_x, filmstrip_rect.top()),
                        4.0,
                        Color32::from_rgb(236, 72, 153),
                    );

                    ui.add_space(6.0);

                    // 3. Compact Waveform Overview Component
                    let wave_height = 24.0;
                    let (wave_rect, wave_response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), wave_height),
                        egui::Sense::click_and_drag(),
                    );
                    let wave_response = wave_response.on_hover_text("Waveform Overview: Drag or click to scrub");

                    let painter = ui.painter_at(wave_rect);
                    painter.rect(
                        wave_rect,
                        4.0,
                        Color32::from_rgb(14, 13, 23),
                        egui::Stroke::new(1.0, Color32::from_rgb(26, 25, 43)),
                    );

                    // Render mock waveform loudness bars
                    let bar_count = self.loudness_profile.len();
                    let bar_width = wave_rect.width() / bar_count as f32;
                    let middle_y = wave_rect.center().y;

                    for i in 0..bar_count {
                        let amplitude = self.loudness_profile[i];
                        let bar_h = amplitude * (wave_rect.height() - 6.0);
                        let x = wave_rect.left() + i as f32 * bar_width;

                        let frame_idx = (i as f32 / bar_count as f32 * self.total_frames as f32) as usize;
                        
                        let inside_active_selection = match (self.in_marker, self.out_marker) {
                            (Some(inf), Some(outf)) => frame_idx >= inf && frame_idx <= outf,
                            _ => false,
                        };

                        let inside_queued_range = self.ranges.iter().any(|r| frame_idx >= r.in_frame && frame_idx <= r.out_frame);

                        let bar_color = if inside_active_selection {
                            Color32::from_rgb(59, 130, 246)
                        } else if inside_queued_range {
                            if self.cut_mode {
                                Color32::from_rgb(239, 68, 68)
                            } else {
                                Color32::from_rgb(16, 185, 129)
                            }
                        } else {
                            if self.cut_mode {
                                Color32::from_rgb(12, 55, 35)
                            } else {
                                Color32::from_rgb(85, 30, 30)
                            }
                        };

                        painter.line_segment(
                            [
                                egui::pos2(x + bar_width * 0.5, middle_y - bar_h * 0.5),
                                egui::pos2(x + bar_width * 0.5, middle_y + bar_h * 0.5),
                            ],
                            egui::Stroke::new(bar_width * 0.75, bar_color),
                        );
                    }

                    // Highlight queued ranges
                    for range in &self.ranges {
                        let left_x = wave_rect.left() + (range.in_frame as f32 / self.total_frames as f32) * wave_rect.width();
                        let right_x = wave_rect.left() + (range.out_frame as f32 / self.total_frames as f32) * wave_rect.width();
                        
                        let overlay_color = if self.cut_mode {
                            Color32::from_rgba_premultiplied(239, 68, 68, 15)
                        } else {
                            Color32::from_rgba_premultiplied(16, 185, 129, 15)
                        };
                        
                        let line_color = if self.cut_mode {
                            Color32::from_rgba_premultiplied(239, 68, 68, 100)
                        } else {
                            Color32::from_rgba_premultiplied(16, 185, 129, 100)
                        };

                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(left_x, wave_rect.top() + 1.0),
                                egui::pos2(right_x, wave_rect.bottom() - 1.0),
                            ),
                            0.0,
                            overlay_color,
                        );
                        
                        painter.line_segment(
                            [egui::pos2(left_x, wave_rect.top()), egui::pos2(left_x, wave_rect.bottom())],
                            egui::Stroke::new(1.0, line_color),
                        );
                        painter.line_segment(
                            [egui::pos2(right_x, wave_rect.top()), egui::pos2(right_x, wave_rect.bottom())],
                            egui::Stroke::new(1.0, line_color),
                        );
                    }

                    // Highlight selected range
                    if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker) {
                        let left_x = wave_rect.left() + (inf as f32 / self.total_frames as f32) * wave_rect.width();
                        let right_x = wave_rect.left() + (outf as f32 / self.total_frames as f32) * wave_rect.width();
                        painter.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(left_x, wave_rect.top() + 1.0),
                                egui::pos2(right_x, wave_rect.bottom() - 1.0),
                            ),
                            0.0,
                            Color32::from_rgba_premultiplied(59, 130, 246, 15),
                        );
                        
                        painter.line_segment(
                            [egui::pos2(left_x, wave_rect.top()), egui::pos2(left_x, wave_rect.bottom())],
                            egui::Stroke::new(2.0, Color32::from_rgb(59, 130, 246)),
                        );
                        painter.line_segment(
                            [egui::pos2(right_x, wave_rect.top()), egui::pos2(right_x, wave_rect.bottom())],
                            egui::Stroke::new(2.0, Color32::from_rgb(59, 130, 246)),
                        );
                    }

                    // Playhead vertical line on compact waveform
                    let playhead_x = wave_rect.left() + (self.current_frame as f32 / self.total_frames as f32) * wave_rect.width();
                    painter.line_segment(
                        [egui::pos2(playhead_x, wave_rect.top() - 2.0), egui::pos2(playhead_x, wave_rect.bottom() + 2.0)],
                        egui::Stroke::new(2.0, Color32::from_rgb(236, 72, 153)),
                    );
                    painter.circle_filled(
                        egui::pos2(playhead_x, wave_rect.top()),
                        3.0,
                        Color32::from_rgb(236, 72, 153),
                    );

                    // Handle scrub scrubbing on compact waveform
                    if wave_response.dragged() || wave_response.clicked() {
                        if let Some(mouse_pos) = wave_response.interact_pointer_pos() {
                            let rel_x = (mouse_pos.x - wave_rect.left()).clamp(0.0, wave_rect.width());
                            let pct = rel_x / wave_rect.width();
                            self.current_frame = (pct * self.total_frames as f32) as usize;
                            self.is_playing = false; // Stop playback on scrub
                        }
                    }
                });
            });
```

- [ ] **Step 2: Run cargo check to verify compilation**

Run: `cargo check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: implement sprocket filmstrip and compact waveform timeline deck"
```
