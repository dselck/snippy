# src

## Overview
Root source tree and application entry point for Snippy 2, encompassing application initialization, global settings management, label session ground truth tracking, notifications, and subsystem coordination between audio, media, bench, and UI.

## Interface
- `[main.rs:main]` - Application entry point initializing logging, GPU graphics context, and running the `eframe` event loop
- `[lib.rs:label_session]` - Re-exports label session models and ground truth break management
- `[lib.rs:notifications]` - Re-exports application notification queues and toast notifications
- `[lib.rs:settings]` - Re-exports persistent configuration structs and zoom calculations
- `[label_session.rs:LabelSession]` - Core state tracker managing ground truth commercial break annotations and adjustments
- `[label_session.rs:LabelSession::new]` - Initializes labelling session with video metadata
- `[label_session.rs:LabelSession::load_detection_output]` - Imports detected breaks from `snippy-detect` output JSON
- `[label_session.rs:LabelSession::load_ground_truth]` - Imports existing ground truth annotations from JSON
- `[label_session.rs:LabelSession::save]` - Saves ground truth break labels to `<video>.ground-truth.json`
- `[label_session.rs:LabelSession::add_break_at]` - Inserts a new break boundary at the designated frame index
- `[label_session.rs:LabelSession::remove_break]` - Deletes a labelled break by index
- `[label_session.rs:LabelSession::set_start_frame]` - Sets the starting boundary frame of a commercial break
- `[label_session.rs:LabelSession::set_end_frame]` - Sets the ending boundary frame of a commercial break
- `[label_session.rs:LabelSession::toggle_break_type]` - Cycles break type between Commercial and Promo
- `[label_session.rs:LabelSession::next_break_index]` - Returns index of next break boundary following playhead
- `[label_session.rs:LabelSession::prev_break_index]` - Returns index of previous break boundary preceding playhead
- `[label_session.rs:LabelSession::break_confidence_band]` - Maps a confidence float to a high/medium/low visual band
- `[label_session.rs:LabelledBreak]` - Internal representation of a labelled commercial interval
- `[label_session.rs:ConfidenceBand]` - Visual categorization enum: `High`, `Medium`, `Low`
- `[notifications.rs:NotificationQueue]` - Thread-safe queue of temporary toast notifications
- `[notifications.rs:NotificationQueue::new]` - Initializes an empty notification queue
- `[notifications.rs:NotificationQueue::push]` - Appends a notification message with auto-dismiss duration
- `[notifications.rs:NotificationQueue::dismiss]` - Removes a notification by index
- `[notifications.rs:Notification]` - Notification message struct: title, body, kind, and timestamp
- `[notifications.rs:NotificationKind]` - Severity enum: `Info`, `Success`, `Warning`, `Error`
- `[settings.rs:Settings]` - Top-level application settings container loaded from and saved to TOML
- `[settings.rs:Settings::load]` - Loads settings from `~/.config/snippy/settings.toml`, restoring defaults on error
- `[settings.rs:Settings::save]` - Persists configuration state to disk
- `[settings.rs:Settings::add_recent_file]` - Adds path to recent files list with deduplication and cap of 10 items
- `[settings.rs:Settings::zoom_to_seconds]` - Converts zoom slider units to visible timeline duration in seconds
- `[settings.rs:Settings::interval_to_zoom_level]` - Maps visible time span to normalized zoom slider value
- `[settings.rs:Settings::zoom_level_to_interval]` - Maps normalized zoom slider value to visible time span
- `[settings.rs:Settings::default_zoom_level]` - Returns initial default zoom level for a video's frame rate
- `[settings.rs:config_path]` - Returns path to `settings.toml` in the user's config directory
- `[settings.rs:PlaybackSettings]` - User playback preferences (audio buffer, scrubbing behaviors)
- `[settings.rs:HardwareSettings]` - Hardware acceleration preferences (auto-detect, VAAPI, VideoToolbox)
- `[settings.rs:ThumbnailSettings]` - Thumbnail filmstrip caching and height preferences
- `[settings.rs:AdvancedSettings]` - Internal audio/video decoding tuning parameters
- `[settings.rs:AppearanceSettings]` - Color themes, typography, and OKLCH color harmonies
- `[settings.rs:TimelineSettings]` - Timeline deck sizing, indicators, and waveform decibel scaling
- `[settings.rs:GeneralSettings]` - General preferences and recent file history
- `[settings.rs:ZoomUnit]` - Unit enum: `Seconds`, `Frames`
- `[settings.rs:ZoomBehavior]` - Zoom anchoring enum: `CenterOnPlayhead`, `KeepPlayheadPosition`
- `[settings.rs:PlayheadMode]` - Playhead scrolling enum: `Continuous`, `Page`, `StationaryCenter`
- `[settings.rs:Theme]` - UI visual theme preset: `Dark`, `Light`, `System`, `Custom`
- `[settings.rs:CustomMode]` - OKLCH dynamic theme generator mode: `Monochrome`, `Complementary`, `Triadic`
- `[settings.rs:ColorHarmony]` - Color relationship model for custom palette generation
- `[settings.rs:SemanticStyle]` - Visual style rules for buttons, highlights, and borders
- `[settings.rs:LoadError]` - Configuration loading error descriptor
- `audio`: [./audio/README.md](./audio/README.md) - Real-time CPAL audio playback and MasterClock synchronization
- `bench`: [./bench/README.md](./bench/README.md) - Headless benchmarking rig and profiling harnesses
- `bin`: [./bin/README.md](./bin/README.md) - Standalone benchmark executables and test runners
- `media`: [./media/README.md](./media/README.md) - Multi-threaded FFmpeg media engine, GOP caching, and smart export
- `ui`: [./ui/README.md](./ui/README.md) - egui user interface, dynamic timeline deck, and WGPU renderer

## Invariants
- `[settings.rs:Settings::load]` - If the TOML config file is corrupt, it is automatically backed up to `settings.toml.bak` and default settings are restored
- `[label_session.rs:LabelSession::add_break_at]` - Breaks list is strictly maintained in ascending order of `start_cut_frame`
- `[settings.rs:Settings::add_recent_file]` - Recent files list deduplicates paths and enforces a maximum cap of 10 items
- `[main.rs:main]` - Configures the `eframe` application window with an 1100x750 initial size, 800x500 minimum size, and validates texture format features on adapter creation

## Side Effects
- `[main.rs:main]` - Runs native GUI window loop via `eframe::run_native`
- `[settings.rs:Settings::save]` - Persists user preferences to `~/.config/snippy/settings.toml`
- `[label_session.rs:LabelSession::save]` - Saves ground truth annotations to disk in `<video>.ground-truth.json`

## Verification
- Run: `cargo test`
