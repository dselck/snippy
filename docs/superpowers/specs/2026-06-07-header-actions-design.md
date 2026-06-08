# Design Spec: Header Load & Export Actions

## 1. Objectives
- Remove the bottom "🚀 Export All Snippets" button from the right sidebar panel.
- Add "Load Video" and "Export Video" buttons inline with the `[snippy] ✂` logo and file path header in the central viewport panel.
- Right-justify the Load & Export buttons while keeping the logo and path left-justified.
- Use the selected "Classic Tray & Arrow" vector glyphs for Load and Export actions.
- Provide descriptive hover tooltips for both buttons.

## 2. UI Layout & Component Design
Inside `src/ui/mod.rs`:
1. **Header Layout**:
   - The central panel header will display `[snippy] ✂` and the video file path left-aligned.
   - A right-to-left layout (`egui::Layout::right_to_left(egui::Align::Center)`) will place the "Export Video" and "Load Video" buttons at the far right.
2. **Icons Design**:
   - `PlaybackIcon` enum will be extended with:
     - `LoadVideo`: A tray with a downward arrow.
     - `ExportVideo`: A tray with an upward arrow.
   - `draw_playback_icon` will be updated to draw these custom shapes using vector segments (using `egui::Painter`).
3. **Button Actions**:
   - Clicking **Load Video** will trigger a file open action (or placeholder/print message for now).
   - Clicking **Export Video** will trigger the export action (disabled if the snippet ranges list is empty).

## 3. Implementation Steps
1. Add `LoadVideo` and `ExportVideo` to `PlaybackIcon`.
2. Add painter paths inside `draw_playback_icon`.
3. Update `playback_control_button` to support styling of these buttons (matching the playback control style, 26px diameter, circle background, tooltips).
4. Remove the old sidebar bottom-up panel block containing "🚀 Export All Snippets" in `src/ui/mod.rs` (lines 841-850).
5. Update the Central Panel header layout (lines 861-902) to include the new buttons right-justified.
6. Verify and compile.
