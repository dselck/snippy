use eframe::egui::{self, RichText, Stroke, pos2, vec2};
use crate::ui::inspector::InspectorTab;
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

            // Stream Health Pill
            let (defect_count, total_sec, _) = app.playback.defect_map().read().unwrap().summary();
            let (health_text, health_color) = if defect_count == 0 {
                ("● HEALTH: CLEAN".to_string(), palette.keep_color)
            } else {
                (format!("● HEALTH: {} DEFECTS ({:.1}s)", defect_count, total_sec), palette.warn_color)
            };

            let health_btn = egui::Button::new(
                RichText::new(health_text)
                    .size(10.0)
                    .strong()
                    .monospace()
                    .color(health_color)
            )
            .frame(false);

            if ui.add(health_btn).on_hover_text("Stream health status. Click to open Stream Health inspector.").clicked() {
                app.show_snippets_hud = true;
                app.snippets_hud_collapsed = false;
                app.active_inspector_tab = InspectorTab::StreamHealth;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::types::{DefectErrorType, StreamTypeMask};

    #[test]
    fn test_render_status_bar_clean_and_defects() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = MainApp::new(&cc);

        // 1. Clean state
        let _ = ctx.run_ui(Default::default(), |ui| {
            render_status_bar(&mut app, ui);
        });

        // 2. State with defect
        app.playback.defect_map().write().unwrap().insert_or_coalesce(
            1.5,
            0.5,
            StreamTypeMask::VIDEO,
            DefectErrorType::CorruptBitstreamPayload,
            None,
        );

        let _ = ctx.run_ui(Default::default(), |ui| {
            render_status_bar(&mut app, ui);
        });
    }
}
