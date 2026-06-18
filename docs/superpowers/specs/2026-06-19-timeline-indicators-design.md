# Design Spec: Timeline Visual Indicators

## Overview
This document outlines the design and implementation details for adding visual indicators for **In/Out markers** and **Cut/Kept regions** on the scrolling bottom timeline (filmstrip and waveform decks) in Snippy.

Currently, the absolute progress bar (directly below the video) has markers, but the scrolling bottom panel lacks any representation of the active range or what segments are cut versus kept. This update will unify the visual style to provide a premium, modern video editing experience.

---

## Requirements

1. **In & Out Markers on Scrolling Timeline:**
   - Display a vertical line at the exact time of the In and Out markers.
   - The lines must span the height of the filmstrip track and continue down through the waveform deck.
   - The lines must scroll dynamically with the timeline, matching the playhead-centered coordinate system.
   - Provide directional bracket caps pointing inward (`[` for In, `]` for Out) at the top and bottom of the track.
   - No text labels (like "IN" or "OUT") to avoid UI clutter.

2. **Cut vs. Kept Visuals on Filmstrip & Waveform:**
   - Clearly distinguish regions of the video that will be cut from those that will be kept.
   - **Cut Regions:** Dimmed to 30% opacity, with a subtle red tint and diagonal hatching lines overlaying the thumbnails and waveform.
   - **Kept Regions:** Rendered at full brightness.
   - The active range selection (between In and Out markers) should have a clear electric blue outline to indicate focus.
   - No text labels (like "CUT" or "KEEP") on the frames.

---

## Technical Design

### 1. Unified Cut/Keep Logic
Add a helper method to [MainApp](file:///Users/dselck/Documents/Snippy2/src/ui/mod.rs) to determine the status of any frame index $f$:

```rust
impl MainApp {
    /// Returns true if the given frame falls within a cut region.
    pub fn is_frame_cut(&self, frame: usize) -> bool {
        let is_in_snippet = self.ranges.iter().any(|r| frame >= r.in_frame && frame <= r.out_frame);
        if self.cut_mode {
            is_in_snippet // In cut mode, snippets are cut
        } else {
            !is_in_snippet // In keep mode, snippets are kept, rest is cut
        }
    }
}
```

### 2. Filmstrip Overlay & Marker Rendering
Inside `timeline_deck` rendering in `src/ui/mod.rs`:

- **Coordinate Mapping:**
  We can compute the horizontal $x$-coordinate for any frame index $f$ relative to the central playhead position:
  $$x = \text{scroll\_offset\_x} + \left(f \times \frac{\text{pixels\_per\_second}}{\text{fps}}\right)$$

- **Thumbnail Dimming and Shading:**
  For each visible thumbnail loop slot:
  - If the frame is cut, render the image using `Color32::from_gray(90)` (for a dimmed effect).
  - Draw a semi-transparent red cover: `Color32::from_rgba_unmultiplied(239, 68, 68, 20)`.
  - Draw diagonal lines (hatching) across the thumbnail bounding box.
  - If the frame is within the active selection (`in_marker` to `out_marker`), draw a premium electric blue stroke (`Color32::from_rgb(59, 130, 246)`) around the thumbnail card.

- **Marker Drawing:**
  For both `self.in_marker` and `self.out_marker`:
  - Calculate their $x$ coordinate.
  - If the coordinate lies within the visible width of the timeline panel, use the `painter` to draw:
    1. A vertical line from the top of the sprocket region to the bottom of the sprocket region.
    2. Side-bracket lines forming `[` or `]` caps of height $6$px and width $8$px.

### 3. Waveform Syncing
Inside the compact synced waveform rendering loop in `src/ui/mod.rs`:

- Check if the frame index mapped to each column column is cut.
- If cut, render the waveform column bar in a dimmed, low-opacity color (e.g. `Color32::from_rgba_unmultiplied(99, 102, 241, 50)` or `#1f1923`).
- Draw the vertical line extensions for In/Out markers through the waveform vertical range to keep visual continuity.

---

## Implementation Details

### Diagonal Hatching Line Helper
To draw diagonal hatching across a rectangle `r: egui::Rect` in `egui`:
```rust
fn draw_hatching(painter: &egui::Painter, rect: egui::Rect, color: Color32, stroke_width: f32, spacing: f32) {
    let mut x = rect.min.x - rect.height();
    while x < rect.max.x {
        let p1 = egui::pos2(x, rect.min.y);
        let p2 = egui::pos2(x + rect.height(), rect.max.y);
        // Clip to the target rectangle
        let line_p1 = egui::pos2(p1.x.clamp(rect.min.x, rect.max.x), p1.y + (rect.min.x - p1.x).max(0.0));
        let line_p2 = egui::pos2(p2.x.clamp(rect.min.x, rect.max.x), p2.y - (p2.x - rect.max.x).max(0.0));
        if line_p1.x < line_p2.x {
            painter.line_segment([line_p1, line_p2], egui::Stroke::new(stroke_width, color));
        }
        x += spacing;
    }
}
```

---

## Testing Strategy

1. **Visual Inspection:**
   - Load a sample video.
   - Toggle between **Cut Mode** and **Keep Mode** to verify that the dimmed/shaded regions invert correctly.
   - Verify that the active selection range is outlined in electric blue.
   - Move the playhead and verify that markers scroll with perfect synchronization.

2. **Integration Verification:**
   - Set In and Out markers and confirm they show up correctly.
   - Add a snippet and verify that the cut/kept shading updates immediately.
