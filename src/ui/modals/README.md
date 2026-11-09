# src/ui/modals

## Overview
Modal dialog overlays for user preferences configuration and smart video export parameter setup and progress tracking.

## Interface
- `[settings_modal.rs:render_settings_modal]` - Preferences modal configuration dialog
- `[export_modal.rs:render_export_modal]` - Export settings and destination configuration modal
- `[export_modal.rs:render_export_progress_modal]` - Active background export progress and cancellation dialog

## Invariants
- `[settings_modal.rs:render_settings_modal]` - Preferences edits apply to `app.pending_settings` scratch state until committed on Save & Apply; Cancel restores original appearance theme
- `[export_modal.rs:render_export_modal]` - Start Export button is disabled unless `app.export_output_path` is non-empty
- `[export_modal.rs:render_export_progress_modal]` - Progress bar and status polling read from thread-safe `Arc<Mutex<ExportStatus>>`, and cancellation sets atomic `export_cancel_flag`

## Side Effects
- `[settings_modal.rs:render_settings_modal]` - Commits settings to disk via `app.commit_settings(ctx)` and mutates `app.show_settings_modal`
- `[export_modal.rs:render_export_modal]` - Invokes native OS file dialog via `rfd::FileDialog::save_file` when browsing destination path
- `[export_modal.rs:render_export_modal]` - Spawns background smart export thread via `app.start_export_thread()`

## Verification
- Run: `cargo test`
