pub mod yuv_renderer;
pub mod theme;
pub mod fonts;
pub mod icons;
pub mod menu_bar;
pub mod workspace_header;
pub mod viewport;
pub mod timeline;
pub mod inspector;
pub mod status_bar;
pub mod modals;

pub use theme::{get_palette, ThemePalette};
pub use viewport::YuvCallback;
pub use modals::*;

use crate::media;
use crate::notifications::{Notification, NotificationQueue};
use crate::settings::Settings;
use eframe::egui::{self, Color32, Stroke};
use lru::LruCache;
use std::num::NonZeroUsize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportRange {
    pub id: usize,
    pub in_frame: usize,
    pub out_frame: usize,
}

pub struct MainApp {
    // Media Playback State
    pub video_path: Option<String>,
    pub playback: crate::media::PlaybackSession,

    // Range Selection Markers
    pub in_marker: Option<usize>,
    pub out_marker: Option<usize>,
    pub ranges: Vec<ExportRange>,
    pub next_range_id: usize,

    // Snippets cut/keep toggle (true = cut snippets out, false = keep only snippets)
    pub cut_mode: bool,

    // Filmstrip Zoom Level (0.0 = fully zoomed out, 1.0 = fully zoomed in)
    pub zoom_level: f32,

    // Height of the resizable bottom timeline deck panel
    pub timeline_height: f32,

    pub show_debug_hud: bool,
    pub decode_fps_frame_count: usize,
    pub decode_fps_last_reset: std::time::Instant,
    pub decode_fps: f32,

    pub thumbnail_cache: LruCache<usize, egui::TextureHandle>,
    pub thumbnails_dirty: bool,
    pub in_flight_requests: std::collections::HashMap<usize, std::time::Instant>,
    pub is_dragging_filmstrip: bool,
    pub is_resizing_timeline: bool,
    pub last_zoom_level: f32,
    pub last_thumbnail_height: f32,

    // Settings & Notifications
    pub settings: Settings,
    pub notifications: NotificationQueue,
    pub show_settings_modal: bool,
    /// Scratch copy of settings edited inside the modal; committed on close.
    pub pending_settings: Settings,

    pub export_status: std::sync::Arc<std::sync::Mutex<crate::media::types::ExportStatus>>,
    pub export_cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub show_export_setup_modal: bool,
    pub export_output_path: String,
    pub export_format: String,
    pub label_session: Option<crate::label_session::LabelSession>,

    // Floating Snippets HUD & Timeline Dynamic Shelf Focus State
    pub show_snippets_hud: bool,
    pub snippets_hud_collapsed: bool,
    pub timeline_pinned_expanded: bool,
    pub active_inspector_tab: inspector::InspectorTab,
}

impl MainApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        fonts::configure_fonts(&cc.egui_ctx);

        // Load settings from disk; push any load error as a Notification.
        let (settings, load_err) = Settings::load();

        // Configure theme using loaded settings
        theme::configure_theme(&cc.egui_ctx, &settings.appearance);

        if let Some(wgpu_state) = &cc.wgpu_render_state {
            let mut renderer = wgpu_state.renderer.write();
            renderer.callback_resources.insert(yuv_renderer::YuvRenderer::new(
                &wgpu_state.device,
                wgpu_state.target_format,
            ));
        }
        let mut notifications = NotificationQueue::new();
        if let Some(err) = load_err {
            notifications.push(Notification::error(err.message()));
        }

        let initial_zoom = settings.default_zoom_level(30.0);
        let lru_cap = NonZeroUsize::new(settings.thumbnails.lru_cache_size.max(1)).unwrap();
        let pending_settings = settings.clone();
        let playback = crate::media::PlaybackSession::new();

        Self {
            video_path: None,
            playback,
            in_marker: None,
            out_marker: None,
            ranges: vec![],
            next_range_id: 1,
            cut_mode: settings.general.default_cut_mode,
            zoom_level: initial_zoom,
            timeline_height: 210.0,
            show_debug_hud: false,
            decode_fps_frame_count: 0,
            decode_fps_last_reset: std::time::Instant::now(),
            decode_fps: 0.0,
            thumbnail_cache: LruCache::new(lru_cap),
            thumbnails_dirty: true,
            in_flight_requests: std::collections::HashMap::new(),
            is_dragging_filmstrip: false,
            is_resizing_timeline: false,
            last_zoom_level: initial_zoom,
            last_thumbnail_height: 0.0,
            settings,
            notifications,
            show_settings_modal: false,
            pending_settings,
            export_status: std::sync::Arc::new(std::sync::Mutex::new(crate::media::types::ExportStatus::Idle)),
            export_cancel_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            show_export_setup_modal: false,
            export_output_path: String::new(),
            export_format: "mp4".to_string(),
            label_session: None,
            show_snippets_hud: true,
            snippets_hud_collapsed: false,
            timeline_pinned_expanded: false,
            active_inspector_tab: inspector::InspectorTab::Snippets,
        }
    }

    pub fn theme_palette(&self) -> ThemePalette {
        theme::get_palette(&self.settings.appearance)
    }

    pub fn frame_to_time(&self, frame: usize) -> String {
        let fps = self.playback.video_fps();
        let total_seconds = if fps > 0.0 { frame as f64 / fps } else { 0.0 };
        self.format_seconds(total_seconds)
    }

    pub fn frame_to_smpte_timecode(&self, frame: usize, fps: f64) -> String {
        let nominal_fps = if fps > 0.0 { fps } else { 30.0 };
        let total_seconds = (frame as f64 / nominal_fps).floor() as usize;
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        let frames = (frame as f64 % nominal_fps).floor() as usize;
        format!("{:02}:{:02}:{:02}:{:02}", hours, minutes, seconds, frames)
    }

    pub fn format_seconds(&self, total_seconds: f64) -> String {
        let hours = (total_seconds / 3600.0).floor() as usize;
        let minutes = ((total_seconds % 3600.0) / 60.0).floor() as usize;
        let seconds = total_seconds.floor() as usize % 60;
        let centiseconds = ((total_seconds - total_seconds.floor()) * 100.0).floor() as usize;
        if hours > 0 {
            format!("{:02}:{:02}:{:02}.{:02}", hours, minutes, seconds, centiseconds)
        } else {
            format!("{:02}:{:02}.{:02}", minutes, seconds, centiseconds)
        }
    }

    pub fn is_frame_cut(&self, frame: usize) -> bool {
        is_frame_cut_helper(frame, &self.ranges, self.cut_mode)
    }

    pub fn open_file_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter("Video", &["mp4", "mkv", "avi", "mov", "ts"]);
        if !self.settings.general.default_open_directory.is_empty() {
            dlg = dlg.set_directory(&self.settings.general.default_open_directory);
        }
        if let Some(path) = dlg.pick_file() {
            let path_str = path.to_string_lossy().to_string();
            self.load_video_from_path(&path_str);
        }
    }

    pub fn load_video_from_path(&mut self, path_str: &str) {
        self.playback.load_video(path_str, &self.settings);
        self.settings.add_recent_file(path_str);
        self.pending_settings.general.recent_files = self.settings.general.recent_files.clone();
        self.video_path = Some(path_str.to_string());
        self.in_marker = None;
        self.out_marker = None;
        self.label_session = None;
    }

    pub fn close_current_video(&mut self) {
        self.video_path = None;
        self.in_marker = None;
        self.out_marker = None;
        self.ranges.clear();
        self.next_range_id = 1;
        self.label_session = None;
        self.thumbnail_cache.clear();
    }

    pub fn trigger_export_dialog(&mut self) {
        self.show_export_setup_modal = true;
        if let Some(ref input_path) = self.video_path {
            let path = std::path::Path::new(input_path);
            let parent = if !self.settings.general.default_open_directory.is_empty() {
                std::path::PathBuf::from(&self.settings.general.default_open_directory)
            } else {
                path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
            };
            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("export");
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_lowercase();
            let format = if ["mp4", "mkv", "ts", "mov"].contains(&ext.as_str()) {
                ext
            } else {
                "mp4".to_string()
            };
            self.export_format = format.clone();
            let export_name = format!("{}_export.{}", file_stem, format);
            self.export_output_path = parent.join(export_name).to_string_lossy().to_string();
        }
    }

    pub fn start_export_thread(&mut self) {
        let input_path = match self.video_path.clone() {
            Some(p) => p,
            None => return,
        };
        let final_output = self.export_output_path.clone();
        let ranges = self.ranges.clone();
        let cut_mode = self.cut_mode;

        if let Some(metadata) = self.playback.metadata() {
            let status_clone = self.export_status.clone();
            let cancel_clone = self.export_cancel_flag.clone();

            {
                let mut status_guard = status_clone.lock().unwrap();
                *status_guard = crate::media::types::ExportStatus::Exporting {
                    progress: 0.0,
                    message: "Starting smart export...".to_string(),
                    is_encoding: false,
                    current_frame: 0,
                };
            }
            cancel_clone.store(false, std::sync::atomic::Ordering::Relaxed);

            std::thread::spawn(move || {
                if let Err(e) = crate::media::export::export_video(
                    &input_path,
                    &final_output,
                    ranges,
                    cut_mode,
                    metadata,
                    status_clone.clone(),
                    cancel_clone,
                ) {
                    let mut status_guard = status_clone.lock().unwrap();
                    *status_guard = crate::media::types::ExportStatus::Failed(e.to_string());
                }
            });
        }
    }

    pub fn open_settings_modal(&mut self) {
        self.pending_settings = self.settings.clone();
        self.show_settings_modal = true;
    }

    pub fn commit_settings(&mut self, ctx: &egui::Context) {
        self.settings = self.pending_settings.clone();
        let _ = self.settings.save();
        theme::configure_theme(ctx, &self.settings.appearance);
    }

    pub fn add_snippet_from_markers(&mut self) {
        if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker) {
            if inf < outf {
                self.ranges.push(ExportRange {
                    id: self.next_range_id,
                    in_frame: inf,
                    out_frame: outf,
                });
                self.reindex_ranges();
            }
        }
    }

    pub fn reindex_ranges(&mut self) {
        coalesce_and_reindex_ranges(&mut self.ranges);
        self.next_range_id = self.ranges.len() + 1;
    }

    pub fn toggle_label_mode(&mut self) {
        if self.label_session.is_some() {
            // Exit label mode
            if let Some(mut session) = self.label_session.take() {
                if session.dirty {
                    let _ = session.save();
                }
            }
        } else if let Some(ref path_str) = self.video_path {
            let total_f = self.playback.total_frames() as u64;
            let fps = self.playback.video_fps();
            let mut sess = crate::label_session::LabelSession::new(std::path::PathBuf::from(path_str), total_f, fps);
            let gt_path = sess.ground_truth_path.clone();
            if gt_path.exists() {
                if let Err(e) = sess.load_ground_truth(&gt_path) {
                    self.notifications.push(Notification::error(format!("Failed to load ground truth: {}", e)));
                } else {
                    self.notifications.push(Notification::info("Resumed ground truth session".to_string()));
                }
            }
            self.label_session = Some(sess);
        }
    }

    pub fn save_ground_truth(&mut self) {
        if let Some(session) = &mut self.label_session {
            match session.save() {
                Ok(_) => self.notifications.push(Notification::info("Ground truth saved successfully!".to_string())),
                Err(e) => self.notifications.push(Notification::error(format!("Failed to save ground truth: {}", e))),
            }
        }
    }

    pub fn load_detection_dialog(&mut self) {
        if let Some(session) = &mut self.label_session {
            if let Some(path) = rfd::FileDialog::new().add_filter("Detection JSON", &["json"]).pick_file() {
                match session.load_detection_output(&path) {
                    Ok(_) => self.notifications.push(Notification::info("Imported detection breaks".to_string())),
                    Err(e) => self.notifications.push(Notification::error(format!("Failed to load detection JSON: {}", e))),
                }
            }
        }
    }
}

impl eframe::App for MainApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let palette = self.theme_palette();
        let ctx = ui.ctx().clone();

        // 1. Process Export Status Updates
        let mut status_changed = None;
        {
            let status_guard = self.export_status.lock().unwrap();
            match &*status_guard {
                crate::media::types::ExportStatus::Completed => status_changed = Some(crate::media::types::ExportStatus::Completed),
                crate::media::types::ExportStatus::Failed(err) => status_changed = Some(crate::media::types::ExportStatus::Failed(err.clone())),
                crate::media::types::ExportStatus::Cancelled => status_changed = Some(crate::media::types::ExportStatus::Cancelled),
                _ => {}
            }
        }
        if let Some(new_status) = status_changed {
            let mut status_guard = self.export_status.lock().unwrap();
            *status_guard = crate::media::types::ExportStatus::Idle;
            match new_status {
                crate::media::types::ExportStatus::Completed => {
                    self.notifications.push(Notification::info("Video exported successfully!".to_string()));
                }
                crate::media::types::ExportStatus::Failed(err) => {
                    self.notifications.push(Notification::error(format!("Export failed: {}", err)));
                }
                crate::media::types::ExportStatus::Cancelled => {
                    self.notifications.push(Notification::warning("Export cancelled.".to_string()));
                }
                _ => {}
            }
        }

        // 2. Global Keyboard Shortcuts
        let wants_kb = ctx.egui_wants_keyboard_input();
        let prev_pressed = !wants_kb && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft));
        let next_pressed = !wants_kb && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
        let prev_down = !wants_kb && ctx.input(|i| i.key_down(egui::Key::ArrowLeft));
        let next_down = !wants_kb && ctx.input(|i| i.key_down(egui::Key::ArrowRight));
        let prev_released = !wants_kb && ctx.input(|i| i.key_released(egui::Key::ArrowLeft));
        let next_released = !wants_kb && ctx.input(|i| i.key_released(egui::Key::ArrowRight));

        if !wants_kb && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.playback.toggle_playback();
        }

        let ctrl_d = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::D);
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_d)) {
            self.show_debug_hud = !self.show_debug_hud;
        }

        let settings_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma);
        let ctrl_comma = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::Comma);
        if ctx.input_mut(|i| i.consume_shortcut(&settings_shortcut) || i.consume_shortcut(&ctrl_comma)) {
            if !self.show_settings_modal {
                self.open_settings_modal();
            }
        }

        let ctrl_o = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
        let win_ctrl_o = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::O);
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_o) || i.consume_shortcut(&win_ctrl_o)) {
            self.open_file_dialog();
        }

        let ctrl_e = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::E);
        let win_ctrl_e = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::E);
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_e) || i.consume_shortcut(&win_ctrl_e)) {
            self.trigger_export_dialog();
        }

        if !wants_kb {
            if ctx.input(|i| i.key_pressed(egui::Key::I) || i.key_pressed(egui::Key::OpenBracket)) {
                self.in_marker = Some(self.playback.current_frame());
            }
            if ctx.input(|i| i.key_pressed(egui::Key::O) || i.key_pressed(egui::Key::CloseBracket)) {
                self.out_marker = Some(self.playback.current_frame());
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.add_snippet_from_markers();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::X)) {
                self.in_marker = None;
                self.out_marker = None;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::C)) {
                self.cut_mode = !self.cut_mode;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::H)) {
                self.show_snippets_hud = !self.show_snippets_hud;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::P)) {
                self.timeline_pinned_expanded = !self.timeline_pinned_expanded;
            }
        }

        // Label mode keyboard shortcuts
        if let Some(session) = &mut self.label_session {
            let save_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::S);
            let ctrl_save_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::S);
            if ctx.input_mut(|i| i.consume_shortcut(&save_shortcut) || i.consume_shortcut(&ctrl_save_shortcut)) {
                match session.save() {
                    Ok(_) => self.notifications.push(Notification::info("Ground truth saved successfully!".to_string())),
                    Err(e) => self.notifications.push(Notification::error(format!("Failed to save: {}", e))),
                }
            }

            if !wants_kb {
                let cur = self.playback.current_frame() as u64;
                if ctx.input(|i| i.key_pressed(egui::Key::B)) {
                    session.add_break_at(cur);
                    ctx.request_repaint();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
                    if let Some(idx) = session.selected_break {
                        session.remove_break(idx);
                        ctx.request_repaint();
                    }
                }
                if ctx.input(|i| i.key_pressed(egui::Key::T)) {
                    if let Some(idx) = session.selected_break {
                        session.toggle_break_type(idx);
                        ctx.request_repaint();
                    }
                }
            }
        }

        // 3. Receive frames and telemetry from Media Engine
        let (latest_frame, frames_popped, loudness_updated) = self.playback.update();
        if loudness_updated || latest_frame.is_some() || self.playback.is_playing() || self.playback.is_stepping() || self.playback.is_scrubbing() {
            ctx.request_repaint();
        }
        if frames_popped > 0 {
            self.decode_fps_frame_count += frames_popped;
        }

        let elapsed = self.decode_fps_last_reset.elapsed();
        if elapsed >= std::time::Duration::from_secs(1) {
            self.decode_fps = self.decode_fps_frame_count as f32 / elapsed.as_secs_f32();
            self.decode_fps_frame_count = 0;
            self.decode_fps_last_reset = std::time::Instant::now();
        }

        // Poll decoded thumbnails
        if let Some(rx) = self.playback.thumbnail_rx.as_ref() {
            while let Ok(resp) = rx.try_recv() {
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [resp.width as usize, resp.height as usize],
                    &resp.rgba_data,
                );
                let texture = ctx.load_texture(
                    format!("thumb_{}", resp.served_frames.first().copied().unwrap_or(0)),
                    color_image,
                    egui::TextureOptions::LINEAR,
                );
                for frame_idx in &resp.served_frames {
                    self.thumbnail_cache.put(*frame_idx, texture.clone());
                    self.in_flight_requests.remove(frame_idx);
                }
            }
        }

        // Dispatch continuous scrub events
        if prev_pressed { self.playback.handle_input(media::InputEvent::ArrowLeftPressed); }
        if prev_down { self.playback.handle_input(media::InputEvent::ArrowLeftDown); }
        if prev_released { self.playback.handle_input(media::InputEvent::ArrowLeftReleased); }
        if next_pressed { self.playback.handle_input(media::InputEvent::ArrowRightPressed); }
        if next_down { self.playback.handle_input(media::InputEvent::ArrowRightDown); }
        if next_released { self.playback.handle_input(media::InputEvent::ArrowRightReleased); }

        // 4. Notifications Top Panel
        let mut dismiss_idx = None;
        for (idx, notif) in self.notifications.items.iter().enumerate() {
            let (bg, fg) = match notif.kind {
                crate::notifications::NotificationKind::Error => (palette.cut_color, Color32::WHITE),
                crate::notifications::NotificationKind::Warning => (palette.warn_color, Color32::BLACK),
                crate::notifications::NotificationKind::Info => (palette.accent, Color32::WHITE),
            };

            egui::Panel::top(format!("notif_{}", idx))
                .frame(egui::Frame::NONE.fill(bg).inner_margin(egui::Margin::symmetric(12, 6)))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&notif.message).color(fg).size(12.0).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                dismiss_idx = Some(idx);
                            }
                        });
                    });
                });
        }
        if let Some(i) = dismiss_idx {
            self.notifications.dismiss(i);
        }

        // 5. Desktop Application Menu Bar
        egui::Panel::top("app_menu_bar")
            .frame(egui::Frame::NONE.fill(palette.header_bg).inner_margin(egui::Margin::symmetric(10, 3)))
            .show_inside(ui, |ui| {
                menu_bar::render_menu_bar(self, ui);
            });

        // 6. Workspace Header Toolbar
        egui::Panel::top("workspace_header_bar")
            .frame(
                egui::Frame::NONE
                    .fill(palette.header_bg)
                    .stroke(Stroke::new(1.0_f32, palette.border_color))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show_inside(ui, |ui| {
                workspace_header::render_workspace_header(self, ui);
            });

        // 7. Desktop Status Bar (Bottom 22px)
        egui::Panel::bottom("app_status_bar")
            .frame(egui::Frame::NONE.fill(palette.status_bar_bg).inner_margin(egui::Margin::symmetric(10, 2)))
            .show_inside(ui, |ui| {
                status_bar::render_status_bar(self, ui);
            });

        // 8. Bottom Multi-Track Timeline Deck (macOS Dock Dynamic Focus)
        let content_bottom = ctx.content_rect().bottom();
        let timeline_thresh = content_bottom - self.timeline_height.max(160.0) - 24.0;
        let is_timeline_hovered = ctx.input(|i| {
            i.pointer.latest_pos().map_or(false, |pos| pos.y >= timeline_thresh)
        });

        let is_interacting = self.playback.is_scrubbing() || self.is_dragging_filmstrip;
        let should_expand = self.timeline_pinned_expanded || is_timeline_hovered || is_interacting;

        let resting_height = 48.0_f32;
        let expanded_height = self.timeline_height.clamp(150.0, 360.0);
        let target_height = if should_expand { expanded_height } else { resting_height };

        let anim_height = ctx.animate_value_with_time(
            egui::Id::new("timeline_dynamic_height_anim"),
            target_height,
            0.18,
        );

        if (anim_height - target_height).abs() > 0.5 {
            ctx.request_repaint();
        }

        let panel_response = egui::Panel::bottom("timeline_deck")
            .resizable(should_expand)
            .exact_size(anim_height)
            .frame(
                egui::Frame::NONE
                    .fill(palette.panel_bg)
                    .stroke(Stroke::new(1.0_f32, palette.border_color))
                    .inner_margin(egui::Margin::symmetric(14, 4)),
            )
            .show_inside(ui, |ui| {
                timeline::render_timeline(self, ui, anim_height);
            });

        if should_expand && panel_response.response.dragged() {
            self.timeline_height = panel_response.response.rect.height();
        }

        // 9. Central Monitor Viewport Deck (Expansive Full Width & Height)
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(palette.panel_bg)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show_inside(ui, |ui| {
                viewport::render_viewport(self, ui);
            });

        // 10. Floating Studio Snippets & Inspector HUD (Roll-up Acrylic Window)
        inspector::render_floating_inspector_hud(self, &ctx);

        // 11. Modal Dialogs
        if self.show_settings_modal {
            modals::render_settings_modal(self, &ctx);
        }
        if self.show_export_setup_modal {
            modals::render_export_modal(self, &ctx);
        }
        modals::render_export_progress_modal(self, &ctx);
    }
}

// ---------------------------------------------------------------------------
// Helper Functions & Re-indexed range operations
// ---------------------------------------------------------------------------

pub fn is_frame_cut_helper(frame: usize, ranges: &[ExportRange], cut_mode: bool) -> bool {
    let is_in_snippet = ranges.iter().any(|r| frame >= r.in_frame && frame <= r.out_frame);
    if cut_mode {
        is_in_snippet
    } else {
        !is_in_snippet
    }
}

pub fn coalesce_and_reindex_ranges(ranges: &mut Vec<ExportRange>) {
    if ranges.is_empty() {
        return;
    }

    ranges.sort_by_key(|r| r.in_frame);
    let mut merged: Vec<ExportRange> = Vec::with_capacity(ranges.len());

    for range in ranges.drain(..) {
        if let Some(last) = merged.last_mut() {
            if range.in_frame <= last.out_frame + 1 {
                last.out_frame = last.out_frame.max(range.out_frame);
            } else {
                merged.push(range);
            }
        } else {
            merged.push(range);
        }
    }

    for (idx, range) in merged.iter_mut().enumerate() {
        range.id = idx + 1;
    }
    *ranges = merged;
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

    #[test]
    fn test_coalesce_and_reindex_ranges() {
        let mut ranges = vec![];
        coalesce_and_reindex_ranges(&mut ranges);
        assert!(ranges.is_empty());

        let mut ranges = vec![
            ExportRange { id: 99, in_frame: 21, out_frame: 30 },
            ExportRange { id: 100, in_frame: 10, out_frame: 20 },
            ExportRange { id: 101, in_frame: 15, out_frame: 25 },
            ExportRange { id: 102, in_frame: 40, out_frame: 50 },
        ];
        coalesce_and_reindex_ranges(&mut ranges);

        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].id, 1);
        assert_eq!(ranges[0].in_frame, 10);
        assert_eq!(ranges[0].out_frame, 30);

        assert_eq!(ranges[1].id, 2);
        assert_eq!(ranges[1].in_frame, 40);
        assert_eq!(ranges[1].out_frame, 50);
    }

    #[test]
    fn test_oklch_roundtrip() {
        let red_rgb = (255, 0, 0);
        let (_l, _c, h) = oklch_to_rgb_and_back(red_rgb.0, red_rgb.1, red_rgb.2);
        assert!(h > 15.0 && h < 35.0);

        let blue_rgb = (0, 0, 255);
        let (_, _, blue_h) = oklch_to_rgb_and_back(blue_rgb.0, blue_rgb.1, blue_rgb.2);
        assert!(blue_h > 240.0 && blue_h < 280.0);
    }

    #[test]
    fn test_main_app_lifecycle() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = MainApp::new(&cc);

        for _ in 0..5 {
            let _ = ctx.run_ui(Default::default(), |ui| {
                let mut frame = eframe::Frame::_new_kittest();
                <MainApp as eframe::App>::ui(&mut app, ui, &mut frame);
            });
        }
    }

    fn oklch_to_rgb_and_back(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
        let (l, c, h) = crate::ui::theme::rgb_to_oklch(r, g, b);
        let rgb_back = crate::ui::theme::oklch_to_rgb(l, c, h);
        let dist = ((r as f32 - rgb_back.r() as f32).powi(2)
            + (g as f32 - rgb_back.g() as f32).powi(2)
            + (b as f32 - rgb_back.b() as f32).powi(2))
            .sqrt();
        assert!(dist < 15.0, "Dist between original and round-tripped is too large: {dist}");
        (l, c, h)
    }
}
