# src/ui

## Overview
Comprehensive egui-based user interface subsystem implementing the video monitor viewport, multi-track dynamic timeline deck, theme and font management, floating inspector HUD, status bar, and custom WGPU YUV shader rendering.

## Interface
- `[mod.rs:MainApp]` - Top-level egui application container holding session state, media engine, and view configuration
- `[mod.rs:MainApp::new]` - Constructor loading user settings, configuring themes, and initializing the application window
- `[mod.rs:MainApp::open_file_dialog]` - Launches native file chooser to select a video file
- `[mod.rs:MainApp::load_video_from_path]` - Initializes playback session, timeline markers, and decoders for a given path
- `[mod.rs:MainApp::close_current_video]` - Flushes engine, resets playhead, and unloads active media session
- `[mod.rs:MainApp::start_export_thread]` - Spawns lossless smart cut export background worker
- `[mod.rs:MainApp::add_snippet_from_markers]` - Converts current IN/OUT marker boundary into an export range
- `[mod.rs:MainApp::reindex_ranges]` - Renumbers cut and keep snippet ranges sequentially
- `[mod.rs:MainApp::toggle_label_mode]` - Toggles between Snippet export mode and Ground Truth break labelling mode
- `[mod.rs:MainApp::save_ground_truth]` - Exports labelled commercial breaks to `<video>.ground-truth.json`
- `[mod.rs:ExportRange]` - Range descriptor defining start/end frames and sequential ID
- `[mod.rs:is_frame_cut_helper]` - Evaluates whether a frame index is included or excluded under cut mode
- `[mod.rs:coalesce_and_reindex_ranges]` - Merges overlapping ranges and applies sequential 1-based indexing
- `[yuv_renderer.rs:YuvRenderer]` - Custom WGPU render pipeline drawing YUV/NV12 planes with BT.601 / BT.709 color conversion
- `[viewport.rs:YuvCallback]` - Egui paint callback binding WGPU render pass for native frame rendering
- `[theme.rs:configure_theme]` - Configures application visual styling based on user appearance preferences
- `[theme.rs:get_palette]` - Generates dynamic OKLCH color palette matching the selected theme
- `[theme.rs:ThemePalette]` - Struct containing semantic UI colors (primary, background, accents, timeline bars)
- `[theme.rs:rgb_to_oklch]` - Converts sRGB u8 triplets to OKLCH perceptual color space
- `[theme.rs:oklch_to_rgb]` - Converts OKLCH coordinates to egui Color32
- `[icons.rs]` - Procedural vector icons (`draw_play`, `draw_pause`, `draw_step_backward`, `draw_step_forward`, `draw_scissors`)
- `[fonts.rs:configure_fonts]` - Loads and registers custom typography fonts (IBM Plex Sans, IBM Plex Mono, Outfit)
- `[yuv.wgsl]` - Embedded WGSL shader performing YUV420p and NV12 planar texture color conversion to RGB
- `[menu_bar.rs:render_menu_bar]` - Top menu bar providing File, Edit, View, and Help menus
- `[workspace_header.rs:render_workspace_header]` - Top workspace bar with file title, mode toggle, and action buttons
- `[viewport.rs:render_viewport]` - Video monitor viewport widget supporting aspect ratio letterboxing
- `[timeline.rs:render_timeline]` - Dynamic multi-track timeline deck with playhead, filmstrip, waveform, and markers
- `[inspector.rs:render_floating_inspector_hud]` - Floating inspector overlay displaying media metadata, snippets, and label tools
- `[inspector.rs:InspectorTab]` - Inspector tab enum: `Snippets`, `Label`, `MediaInfo`
- `[status_bar.rs:render_status_bar]` - Bottom status bar with transport info, timecode, zoom level, and background status
- `[status_bar.rs:STATUS_BAR_HEIGHT]` - Fixed constant for bottom status bar layout (24.0px)
- `modals`: [./modals/README.md](./modals/README.md) - Modal dialog overlays for preferences and video export

## Invariants
- `[mod.rs:coalesce_and_reindex_ranges]` - Export ranges are kept sorted by start frame and contiguous overlapping intervals are automatically merged with 1-based sequential IDs
- `[mod.rs:MainApp::ui]` - Multi-track timeline deck animates between resting height (48px) and expanded height (150-360px) based on pointer proximity or active scrubbing interaction
- `[mod.rs:MainApp::new]` - Thumbnail texture handles are cached in an LRU cache bounded by `settings.thumbnails.lru_cache_size`
- `[yuv_renderer.rs:YuvRenderer]` - WGPU render pipeline binds planar YUV/NV12 textures and converts color spaces in fragment shader with BT.601 / BT.709 coefficients

## Side Effects
- `[yuv_renderer.rs:YuvRenderer::prepare]` - Direct GPU resource allocation and texture uploads via WGPU render queue
- `[mod.rs:MainApp::open_file_dialog]` - Native OS file chooser dialog execution via `rfd`
- `[mod.rs:MainApp::commit_settings]` - Modifies user settings file on disk via `Settings::save`
- `[mod.rs:MainApp::start_export_thread]` - Spawns background encoding thread

## Verification
- Run: `cargo test`
