# Design Spec: Sprocket Filmstrip & Waveform Timeline

## 1. Objectives
- Split the bottom timeline panel into two horizontal areas:
  1. **Filmstrip** (top): Displays a row of evenly spaced thumbnails with sprocket holes on top and bottom, representing frames relative to the current playhead (always locked to the center).
  2. **Waveform Overview** (bottom): A compact version of the full-video audio waveform.
- Implement **Zoom In / Zoom Out** controls (a slider + buttons) for the filmstrip, controlling the frame step size between adjacent thumbnails.
- Clicking any thumbnail centers that frame (making it the current frame).
- Keep out-of-bounds frames (before start or after end of video) styled as blank placeholders.

## 2. UI Layout & Component Design
Inside `src/ui/mod.rs`:
1. **State Modifications**:
   - Add `zoom_level: f32` to `MainApp` (default: `0.0` (fully zoomed out), max: `1.0` (fully zoomed in)).
2. **Filmstrip Drawing**:
   - Determine number of thumbnails $N = 7$ (so index ranges from $-3$ to $+3$).
   - Calculate step size `delta = lerp(total_frames / (N - 1), 1.0, zoom_level)`. If `zoom_level = 1.0` (max zoom), `delta = 1`.
   - Calculate frame index for each thumbnail `i`: `frame = current_frame + i * delta`.
   - If `frame` is between `0` and `total_frames`:
     - Render thumbnail box (with gradient background, frame number, and timecode).
     - If clicked, set `self.current_frame = frame`.
   - If `frame` is out of bounds, render a dark gray diagonal hash pattern box to show empty space.
   - Paint sprocket holes: Small rounded squares drawn on the top and bottom margins of the filmstrip track.
3. **Compact Waveform**:
   - Render the waveform below the filmstrip (height reduced to ~25px).
   - Render the selection highlight ranges and playhead on the compact waveform, maintaining full-timeline scrubbing functionality.
4. **Zoom Row**:
   - Place a compact zoom slider and +/- buttons in a header bar above the filmstrip or inline.

## 3. Implementation Steps
1. Add `zoom_level: f32` to `MainApp::new`.
2. Update the bottom panel drawing logic to display the Zoom controls, Sprocket Filmstrip, and Compact Waveform sequentially.
3. Add helper function `draw_filmstrip_thumbnail` and `draw_sprocket_holes`.
4. Verify and compile.
