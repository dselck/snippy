# Timeline Visual Indicators Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement visual indicators on the scrolling timeline and waveform to display In/Out markers and differentiate Cut vs Kept frames.

**Architecture:** Add a unit-tested helper to evaluate the cut status of any frame based on the active snippets and editing mode. Modify the scrolling filmstrip and waveform decks to draw dimmed/crimson-hatched overlays for cut regions, electric blue borders for active selection regions, and clean vertical lines with caps for markers.

**Tech Stack:** Rust, egui

---

### Task 1: Add and Unit Test the Cut/Keep Helper

**Files:**
- Modify: `src/ui/mod.rs:138-147`
- Test: `src/ui/mod.rs` (at the bottom of the file)

- [ ] **Step 1: Write the failing test and helper skeleton**
  Add the helper function and tests block to the bottom of `src/ui/mod.rs`.

  ```rust
  // Insert at the very end of src/ui/mod.rs:
  
  pub fn is_frame_cut_helper(frame: usize, ranges: &[ExportRange], cut_mode: bool) -> bool {
      false // Temporary skeleton
  }
  
  #[cfg(test)]
  mod tests {
      use super::*;
  
      #[test]
      fn test_is_frame_cut() {
          let ranges = vec![
              ExportRange { id: 1, in_frame: 10, out_frame: 20 },
              ExportRange { id: 2, in_frame: 40, out_frame: 50 },
          ];
          // Cut mode: ranges are cut, rest is kept
          assert!(is_frame_cut_helper(15, &ranges, true));
          assert!(!is_frame_cut_helper(30, &ranges, true));
          assert!(is_frame_cut_helper(45, &ranges, true));
  
          // Keep mode: ranges are kept, rest is cut
          assert!(!is_frame_cut_helper(15, &ranges, false));
          assert!(is_frame_cut_helper(30, &ranges, false));
          assert!(!is_frame_cut_helper(45, &ranges, false));
      }
  }
  ```

- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test --bin snippy test_is_frame_cut`
  Expected: FAIL (assertion fails since helper returns false)

- [ ] **Step 3: Write minimal implementation for the helper and add MainApp delegation**
  Implement the logic in `is_frame_cut_helper` and add the method to `impl MainApp`.

  ```rust
  // In src/ui/mod.rs, near other helpers (e.g. after frame_to_time):
  impl MainApp {
      pub fn is_frame_cut(&self, frame: usize) -> bool {
          is_frame_cut_helper(frame, &self.ranges, self.cut_mode)
      }
  }
  
  // At the bottom of src/ui/mod.rs:
  pub fn is_frame_cut_helper(frame: usize, ranges: &[ExportRange], cut_mode: bool) -> bool {
      let is_in_snippet = ranges.iter().any(|r| frame >= r.in_frame && frame <= r.out_frame);
      if cut_mode {
          is_in_snippet
      } else {
          !is_in_snippet
      }
  }
  ```

- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test --bin snippy test_is_frame_cut`
  Expected: PASS

- [ ] **Step 5: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: add is_frame_cut helper with unit tests"
  ```

---

### Task 2: Implement Thumbnail Hatching and Dimming in Filmstrip

**Files:**
- Modify: `src/ui/mod.rs:1230-1290` (visible thumbnails loop)

- [ ] **Step 1: Implement the hatching utility function**
  Add the `draw_hatching` helper near the top of the timeline code or at the bottom of `src/ui/mod.rs` right above tests.

  ```rust
  fn draw_hatching(painter: &egui::Painter, rect: egui::Rect, color: Color32, stroke_width: f32, spacing: f32) {
      let clipped_painter = painter.with_clip_rect(rect);
      let mut x = rect.min.x - rect.height();
      while x < rect.max.x {
          let p1 = egui::pos2(x, rect.min.y);
          let p2 = egui::pos2(x + rect.height(), rect.max.y);
          clipped_painter.line_segment([p1, p2], egui::Stroke::new(stroke_width, color));
          x += spacing;
      }
  }
  ```

- [ ] **Step 2: Update the thumbnail rendering loop**
  Modify the loop that draws visible thumbnails. Check if the frame is cut, dim the thumbnail tint, draw a red semi-transparent overlay, and apply hatching. Also, draw an electric blue border for selection.

  ```rust
  // Modify inside the loop over visible slots (around line 1230):
  let is_cut = self.is_frame_cut(target_frame);
  let tint = if is_cut {
      Color32::from_gray(90) // Dimmed
  } else {
      Color32::WHITE
  };

  if let Some(texture) = self.thumbnail_cache.get(&target_frame) {
      // Draw decoded thumbnail image
      painter.image(
          texture.id(),
          thub_rect,
          egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
          tint,
      );

      if is_cut {
          // Draw red transparent tint
          painter.rect_filled(
              thub_rect,
              0.0,
              Color32::from_rgba_unmultiplied(239, 68, 68, 20),
          );
          // Draw diagonal hatching lines
          draw_hatching(
              &painter,
              thub_rect,
              Color32::from_rgba_unmultiplied(239, 68, 68, 45),
              1.0,
              12.0,
          );
      }
  } else {
      // Placeholder loading card
      let placeholder_bg = if is_cut {
          Color32::from_rgb(18, 10, 15) // Darker reddish bg
      } else {
          Color32::from_rgb(18, 17, 28)
      };
      painter.rect_filled(thub_rect, 4.0, placeholder_bg);
      // ... (existing spinner drawing code remains unchanged)
      if is_cut {
          draw_hatching(
              &painter,
              thub_rect,
              Color32::from_rgba_unmultiplied(239, 68, 68, 30),
              1.0,
              12.0,
          );
      }
  }

  // Draw active selection border (blue outline)
  if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker) {
      if target_frame >= inf && target_frame <= outf {
          painter.rect_stroke(
              thub_rect,
              4.0,
              egui::Stroke::new(2.0, Color32::from_rgb(59, 130, 246)),
              egui::StrokeKind::Inside,
          );
      }
  }
  ```

- [ ] **Step 3: Verify build**
  Run: `cargo check`
  Expected: Success

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: dim and hatch cut frames and outline selected frames on filmstrip"
  ```

---

### Task 3: Draw Dynamic In/Out Markers and Sync Waveform

**Files:**
- Modify: `src/ui/mod.rs:1355-1435` (waveform and marker lines)

- [ ] **Step 1: Update Waveform bars styling**
  Modify the compact synced waveform column rendering loop to dim bars that belong to cut frames.

  ```rust
  // Modify inside the waveform bar rendering loop (around line 1420):
  let frame_idx_isize = start_frame + (pct * visible_range_width) as isize;
  let out_of_bounds = frame_idx_isize < 0 || frame_idx_isize > self.playback.total_frames() as isize;
  
  let is_cut = if out_of_bounds {
      false
  } else {
      self.is_frame_cut(frame_idx_isize as usize)
  };

  let bar_color = if out_of_bounds {
      Color32::from_rgb(20, 19, 32)
  } else if is_cut {
      // Dimmed Indigo color for cut regions
      Color32::from_rgba_unmultiplied(99, 102, 241, 40)
  } else {
      Color32::from_rgb(99, 102, 241) // Premium Indigo accent color
  };
  ```

- [ ] **Step 2: Draw vertical marker lines and brackets**
  At the end of the `timeline_deck` panel rendering (after the waveform drawing, around line 1435), draw the vertical marker lines and brackets for the In and Out markers.

  ```rust
  // Draw scrolling markers on the filmstrip & waveform
  let draw_timeline_marker = |painter: &egui::Painter, frame: usize, color: Color32, direction: f32| {
      let frame_time = frame as f64 / fps;
      let x = scroll_offset_x + frame_time * pixels_per_second;
      let x_f32 = x as f32;
      
      // Draw only if horizontally visible inside the filmstrip rect
      if x_f32 >= filmstrip_rect.left() && x_f32 <= filmstrip_rect.right() {
          // 1. Draw continuous vertical line through both filmstrip and waveform
          painter.line_segment(
              [egui::pos2(x_f32, filmstrip_rect.top()), egui::pos2(x_f32, wave_rect.bottom())],
              egui::Stroke::new(1.5, color),
          );
          
          // 2. Top Bracket Caps at filmstrip_rect.top()
          painter.line_segment(
              [egui::pos2(x_f32, filmstrip_rect.top()), egui::pos2(x_f32 + direction * 8.0, filmstrip_rect.top())],
              egui::Stroke::new(1.5, color),
          );
          painter.line_segment(
              [egui::pos2(x_f32 + direction * 8.0, filmstrip_rect.top()), egui::pos2(x_f32 + direction * 8.0, filmstrip_rect.top() + 6.0)],
              egui::Stroke::new(1.5, color),
          );
          
          // 3. Bottom Bracket Caps at wave_rect.bottom()
          painter.line_segment(
              [egui::pos2(x_f32, wave_rect.bottom()), egui::pos2(x_f32 + direction * 8.0, wave_rect.bottom())],
              egui::Stroke::new(1.5, color),
          );
          painter.line_segment(
              [egui::pos2(x_f32 + direction * 8.0, wave_rect.bottom()), egui::pos2(x_f32 + direction * 8.0, wave_rect.bottom() - 6.0)],
              egui::Stroke::new(1.5, color),
          );
      }
  };

  if let Some(inf) = self.in_marker {
      draw_timeline_marker(&ui.painter(), inf, Color32::from_rgb(251, 191, 36), 1.0);
  }
  if let Some(outf) = self.out_marker {
      draw_timeline_marker(&ui.painter(), outf, Color32::from_rgb(249, 115, 22), -1.0);
  }
  ```

- [ ] **Step 3: Verify full project builds**
  Run: `cargo build`
  Expected: Success

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: draw scrolling In/Out marker lines and dim waveform for cut frames"
  ```
