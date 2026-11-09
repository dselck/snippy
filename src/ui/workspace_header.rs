use eframe::egui::{self, Color32, RichText, vec2};
use crate::ui::icons::{self, IconType};
use crate::ui::MainApp;

pub fn render_workspace_header(app: &mut MainApp, ui: &mut egui::Ui) {
    let palette = app.theme_palette();
    
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);

        // 1. Technical Brand Identity
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
            ui.label(
                RichText::new("SNIPPY")
                    .strong()
                    .size(15.0)
                    .color(palette.text_normal)
            );
            
            // Pro badge pill
            let (badge_rect, _) = ui.allocate_exact_size(vec2(32.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(badge_rect, 2.0, palette.widget_inactive_bg);
            ui.painter().rect_stroke(badge_rect, 2.0, egui::Stroke::new(1.0_f32, palette.border_color), egui::StrokeKind::Inside);
            ui.painter().text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                "PRO",
                egui::FontId::monospace(10.0),
                palette.accent,
            );
        });

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        // 2. Active Media Metadata Pill / Breadcrumb & Choice Banner
        let video_info = app.video_path.as_ref().map(|path| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path)
                .to_string()
        });

        if let Some(filename) = video_info {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                
                // Filename
                ui.label(
                    RichText::new(&filename)
                        .strong()
                        .size(13.0)
                        .color(palette.text_normal)
                );

                // Video format pill
                let fps = app.playback.video_fps();
                let total_frames = app.playback.total_frames();
                let duration_sec = if fps > 0.0 { total_frames as f64 / fps } else { 0.0 };
                let dur_str = app.format_seconds(duration_sec);

                let meta_str = format!("{:.2} fps • {}", fps, dur_str);
                ui.label(
                    RichText::new(format!("({})", meta_str))
                        .size(11.0)
                        .color(palette.text_muted)
                        .monospace()
                );

                // Contextual Choice Banner: when In/Out selection is active, offer 1-click addition
                let mut add_clicked = false;
                let mut clear_clicked = false;
                if let (Some(inf), Some(outf)) = (app.in_marker, app.out_marker) {
                    if inf < outf {
                        let dur_f = outf.saturating_sub(inf);
                        let dur_s = if fps > 0.0 { dur_f as f64 / fps } else { 0.0 };
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);

                        let choice_frame = egui::Frame::NONE
                            .fill(palette.card_bg)
                            .stroke(egui::Stroke::new(1.0_f32, palette.accent))
                            .corner_radius(3.0)
                            .inner_margin(egui::Margin::symmetric(8, 2));

                        choice_frame.show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                            ui.label(
                                RichText::new(format!("Sel: {:.2}s", dur_s))
                                    .monospace()
                                    .size(11.0)
                                    .color(Color32::WHITE),
                            );

                            if ui.add(
                                egui::Button::new(
                                    RichText::new("+ Add Snippet")
                                        .size(11.0)
                                        .strong()
                                        .color(Color32::WHITE),
                                )
                                .fill(palette.accent)
                                .corner_radius(2.0),
                            ).clicked() {
                                add_clicked = true;
                            }

                            if ui.add(
                                egui::Button::new(RichText::new("✕").size(10.0).color(palette.text_muted)).frame(false),
                            ).on_hover_text("Clear In/Out selection (X)").clicked() {
                                clear_clicked = true;
                            }
                        });
                    }
                }
                if add_clicked {
                    app.add_snippet_from_markers();
                }
                if clear_clicked {
                    app.in_marker = None;
                    app.out_marker = None;
                }
            });
        } else {
            ui.label(
                RichText::new("No media loaded")
                    .size(12.0)
                    .color(palette.text_muted)
            );
        }

        // 3. Right-aligned Segmented Mode Switcher & Action Buttons
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);

            // Settings Gear Button
            let gear_resp = icons::icon_button(ui, IconType::Settings, vec2(28.0, 24.0), app.show_settings_modal, "Settings (Ctrl+,)");
            if gear_resp.clicked() {
                app.open_settings_modal();
            }

            ui.separator();

            // Export Button (Accent action)
            let can_export = app.video_path.is_some() && (!app.ranges.is_empty() || app.cut_mode);
            ui.add_enabled_ui(can_export, |ui| {
                let btn = egui::Button::new(
                    RichText::new("Export Video")
                        .size(12.0)
                        .strong()
                        .color(if can_export { Color32::WHITE } else { palette.text_muted })
                )
                .fill(if can_export { palette.accent } else { palette.widget_inactive_bg })
                .corner_radius(3.0)
                .min_size(vec2(90.0, 24.0));

                if ui.add(btn).on_hover_text("Export video snippet (Ctrl+E)").clicked() {
                    app.trigger_export_dialog();
                }
            });

            // Open Video Button
            let open_btn = egui::Button::new(
                RichText::new("Open Video")
                    .size(12.0)
                    .color(palette.text_normal)
            )
            .fill(palette.widget_inactive_bg)
            .stroke(egui::Stroke::new(1.0_f32, palette.border_color))
            .corner_radius(3.0)
            .min_size(vec2(80.0, 24.0));

            if ui.add(open_btn).on_hover_text("Open video file (Ctrl+O)").clicked() {
                app.open_file_dialog();
            }

            ui.separator();

            // Floating Snippets HUD Toggle Button
            let hud_label = format!("✂ Snippets ({})", app.ranges.len());
            let hud_active = app.show_snippets_hud;
            let hud_btn = egui::Button::new(
                RichText::new(hud_label)
                    .size(11.0)
                    .strong()
                    .color(if hud_active { Color32::WHITE } else { palette.text_muted })
            )
            .fill(if hud_active { palette.card_bg } else { palette.widget_inactive_bg })
            .stroke(egui::Stroke::new(1.0_f32, if hud_active { palette.accent } else { palette.border_color }))
            .corner_radius(3.0)
            .min_size(vec2(95.0, 24.0));

            if ui.add(hud_btn).on_hover_text("Toggle floating Snippets HUD window (H)").clicked() {
                if !app.show_snippets_hud {
                    app.show_snippets_hud = true;
                    app.snippets_hud_collapsed = false;
                } else if app.snippets_hud_collapsed {
                    app.snippets_hud_collapsed = false;
                } else {
                    app.show_snippets_hud = false;
                }
            }

            // Timeline Pin Toggle Button
            let pin_label = if app.timeline_pinned_expanded { "📌 Timeline Pinned" } else { "⏱ Auto-Timeline" };
            let pin_active = app.timeline_pinned_expanded;
            let pin_btn = egui::Button::new(
                RichText::new(pin_label)
                    .size(11.0)
                    .color(if pin_active { palette.accent } else { palette.text_muted })
            )
            .fill(palette.widget_inactive_bg)
            .stroke(egui::Stroke::new(1.0_f32, palette.border_color))
            .corner_radius(3.0)
            .min_size(vec2(90.0, 24.0));

            if ui.add(pin_btn).on_hover_text("Pin timeline permanently expanded vs auto-expand on hover (P)").clicked() {
                app.timeline_pinned_expanded = !app.timeline_pinned_expanded;
            }

            ui.separator();

            // Segmented Mode Switcher: [ Edit Mode ] | [ Label Mode ]
            if app.video_path.is_some() {
                let in_label = app.label_session.is_some();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(2.0, 0.0);

                    // Edit Mode Button
                    let edit_active = !in_label;
                    let edit_btn = egui::Button::new(
                        RichText::new("Editor")
                            .size(11.0)
                            .strong()
                            .color(if edit_active { Color32::WHITE } else { palette.text_muted })
                    )
                    .fill(if edit_active { palette.accent } else { palette.widget_inactive_bg })
                    .corner_radius(3.0)
                    .min_size(vec2(60.0, 22.0));

                    if ui.add(edit_btn).clicked() && in_label {
                        app.toggle_label_mode();
                    }

                    // Label Mode Button
                    let label_active = in_label;
                    let label_btn = egui::Button::new(
                        RichText::new("Labels")
                            .size(11.0)
                            .strong()
                            .color(if label_active { Color32::WHITE } else { palette.text_muted })
                    )
                    .fill(if label_active { palette.warn_color } else { palette.widget_inactive_bg })
                    .corner_radius(3.0)
                    .min_size(vec2(60.0, 22.0));

                    if ui.add(label_btn).on_hover_text("Switch to training data labelling mode").clicked() && !in_label {
                        app.toggle_label_mode();
                    }
                });
            }
        });
    });
}
