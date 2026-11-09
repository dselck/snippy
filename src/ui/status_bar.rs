use eframe::egui::{self, RichText, Stroke, pos2, vec2};
use crate::ui::MainApp;

pub const STATUS_BAR_HEIGHT: f32 = 22.0;

pub fn render_status_bar(app: &mut MainApp, ui: &mut egui::Ui) {
    let palette = app.theme_palette();
    let total_width = ui.available_width();

    let (rect, _response) = ui.allocate_exact_size(vec2(total_width, STATUS_BAR_HEIGHT), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Background and top dividing hairline
    painter.rect_filled(rect, 0.0, palette.status_bar_bg);
    painter.line_segment(
        [pos2(rect.left(), rect.top()), pos2(rect.right(), rect.top())],
        Stroke::new(1.0_f32, palette.border_color),
    );

    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 0.0);

            // 1. Left: Playback / Engine State
            let (state_text, state_color) = if app.video_path.is_some() {
                if app.playback.is_playing() {
                    ("PLAYING", palette.keep_color)
                } else if app.playback.is_scrubbing() {
                    ("SCRUBBING", palette.accent)
                } else {
                    ("READY", palette.text_muted)
                }
            } else {
                ("IDLE", palette.text_muted)
            };

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                // Status dot
                let (dot_rect, _) = ui.allocate_exact_size(vec2(6.0, 6.0), egui::Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 2.5, state_color);

                ui.label(
                    RichText::new(state_text)
                        .size(10.0)
                        .strong()
                        .monospace()
                        .color(state_color)
                );
            });

            if let Some(path) = &app.video_path {
                let filename = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path);
                ui.label(
                    RichText::new(filename)
                        .size(10.0)
                        .color(palette.text_muted)
                );
            }

            // 2. Right: Telemetry (Sync drift, Decode FPS, Zoom level)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing = vec2(12.0, 0.0);

                // Zoom percentage
                let zoom_pct = ((app.zoom_level * 100.0).round() as usize).max(1);
                ui.label(
                    RichText::new(format!("Zoom: {}%", zoom_pct))
                        .size(10.0)
                        .monospace()
                        .color(palette.text_muted)
                );

                // Decode FPS
                let fps_col = if app.decode_fps >= 24.0 { palette.keep_color } else { palette.warn_color };
                ui.label(
                    RichText::new(format!("{:.1} fps", app.decode_fps))
                        .size(10.0)
                        .monospace()
                        .color(fps_col)
                );

                // Master Clock Audio Sample Rate
                let sample_rate = app.playback.master_clock().map_or(0, |mc| mc.sample_rate());
                if sample_rate > 0 {
                    ui.label(
                        RichText::new(format!("{} Hz", sample_rate))
                            .size(10.0)
                            .monospace()
                            .color(palette.text_muted)
                    );
                }

                // Video FPS / Timebase
                let video_fps = app.playback.video_fps();
                if video_fps > 0.0 {
                    ui.label(
                        RichText::new(format!("{:.2} fps", video_fps))
                            .size(10.0)
                            .monospace()
                            .color(palette.text_muted)
                    );
                }
            });
        });
    });
}
