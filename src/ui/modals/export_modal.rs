use eframe::egui::{self, Color32, Rect, RichText, Stroke, StrokeKind, pos2, vec2};
use crate::ui::is_frame_cut_helper;
use crate::ui::MainApp;

pub fn render_export_modal(app: &mut MainApp, ctx: &egui::Context) {
    let palette = app.theme_palette();

    egui::Window::new("Export Settings")
        .id(egui::Id::new("export_setup_modal"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([480.0, 230.0])
        .frame(
            egui::Frame::window(&ctx.global_style())
                .fill(palette.panel_bg)
                .stroke(Stroke::new(1.0_f32, palette.border_color))
                .corner_radius(3.0)
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            ui.set_width(448.0);

            // Title
            ui.label(RichText::new("Smart Export Video").size(16.0).color(palette.text_normal).strong());
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(10.0);

            // Format Selection
            ui.horizontal(|ui| {
                ui.label(RichText::new("Container:").strong().size(12.0).color(palette.text_normal));
                ui.add_space(8.0);

                for fmt in &["mp4", "mkv", "ts", "mov"] {
                    if ui.radio_value(&mut app.export_format, fmt.to_string(), fmt.to_uppercase()).clicked() {
                        if !app.export_output_path.is_empty() {
                            let path = std::path::Path::new(&app.export_output_path);
                            if let Some(parent) = path.parent() {
                                if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                                    let new_filename = format!("{}.{}", file_stem, app.export_format);
                                    app.export_output_path = parent.join(new_filename).to_string_lossy().to_string();
                                }
                            }
                        }
                    }
                }
            });

            ui.add_space(10.0);

            // Destination Path
            ui.label(RichText::new("Output Path:").strong().size(12.0).color(palette.text_normal));
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.export_output_path)
                        .desired_width(ui.available_width() - 80.0)
                        .hint_text("Choose destination file path...")
                );

                if ui.button("Browse...").clicked() {
                    let mut dlg = rfd::FileDialog::new()
                        .add_filter(&format!("{} Video", app.export_format.to_uppercase()), &[&app.export_format]);

                    if !app.export_output_path.is_empty() {
                        let path = std::path::Path::new(&app.export_output_path);
                        if let Some(parent) = path.parent() {
                            if parent.exists() {
                                dlg = dlg.set_directory(parent);
                            }
                        }
                        if let Some(file_name) = path.file_name() {
                            dlg = dlg.set_file_name(file_name.to_string_lossy().to_string());
                        }
                    }

                    if let Some(path) = dlg.save_file() {
                        app.export_output_path = path.to_string_lossy().to_string();
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            let ext_lower = ext.to_lowercase();
                            if ["mp4", "mkv", "ts", "mov"].contains(&ext_lower.as_str()) {
                                app.export_format = ext_lower;
                            }
                        }
                    }
                }
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);

            // Actions
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let has_path = !app.export_output_path.is_empty();
                    let start_btn = egui::Button::new(
                        RichText::new("Start Export")
                            .strong()
                            .size(12.0)
                            .color(if has_path { Color32::WHITE } else { palette.text_muted })
                    )
                    .fill(if has_path { palette.accent } else { palette.widget_inactive_bg })
                    .corner_radius(3.0)
                    .min_size(vec2(90.0, 26.0));

                    if ui.add_enabled(has_path, start_btn).clicked() {
                        app.show_export_setup_modal = false;
                        app.start_export_thread();
                    }

                    if ui.button("Cancel").clicked() {
                        app.show_export_setup_modal = false;
                    }
                });
            });
        });
}

pub fn render_export_progress_modal(app: &mut MainApp, ctx: &egui::Context) {
    let palette = app.theme_palette();
    let export_status = app.export_status.clone();
    let status_guard = export_status.lock().unwrap();

    if let crate::media::types::ExportStatus::Exporting { progress, message, is_encoding, current_frame } = &*status_guard {
        egui::Window::new("Exporting Video")
            .id(egui::Id::new("export_modal"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([460.0, 220.0])
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .fill(palette.panel_bg)
                    .stroke(Stroke::new(1.0_f32, palette.border_color))
                    .corner_radius(3.0),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Smart Exporting...").strong().size(15.0).color(palette.text_normal));
                    ui.add_space(10.0);

                    let bar_color = if *is_encoding { palette.warn_color } else { palette.keep_color };

                    // Numeric progress bar
                    ui.add(
                        egui::ProgressBar::new(*progress)
                            .text(format!("{:.0}%", progress * 100.0))
                            .fill(bar_color)
                    );

                    ui.add_space(10.0);

                    // Segmented GOP timeline visualization
                    if let Some(ref meta) = app.playback.metadata() {
                        let keyframes = &meta.keyframes;
                        let total_frames = meta.total_frames;

                        if total_frames > 0 && !keyframes.is_empty() {
                            let desired_size = vec2(ui.available_width(), 16.0);
                            let (rect, _response) = ui.allocate_at_least(desired_size, egui::Sense::hover());
                            let painter = ui.painter();

                            painter.rect_filled(rect, 2.0, palette.timeline_track_bg);

                            let mut current_type: Option<(bool, Color32)> = None;
                            let mut segment_start = 0;

                            for i in 0..keyframes.len() {
                                let start_frame = keyframes[i];
                                let end_frame = if i + 1 < keyframes.len() { keyframes[i + 1] } else { total_frames };

                                let mut kept_count = 0;
                                let mut cut_count = 0;
                                for f in start_frame..end_frame {
                                    if is_frame_cut_helper(f, &app.ranges, app.cut_mode) {
                                        cut_count += 1;
                                    } else {
                                        kept_count += 1;
                                    }
                                }

                                let gop_type = if kept_count == (end_frame - start_frame) {
                                    crate::media::export::GopType::Copy
                                } else if cut_count == (end_frame - start_frame) {
                                    crate::media::export::GopType::Cut
                                } else {
                                    crate::media::export::GopType::Transcode
                                };

                                for f in start_frame..end_frame {
                                    let is_frame_cut = is_frame_cut_helper(f, &app.ranges, app.cut_mode);
                                    let color = match gop_type {
                                        crate::media::export::GopType::Copy => palette.keep_color,
                                        crate::media::export::GopType::Cut => palette.timeline_track_bg,
                                        crate::media::export::GopType::Transcode => {
                                            if is_frame_cut { palette.timeline_track_bg } else { palette.warn_color }
                                        }
                                    };
                                    let is_kept = !is_frame_cut && gop_type != crate::media::export::GopType::Cut;

                                    if let Some((last_kept, last_color)) = current_type {
                                        if last_kept != is_kept || last_color != color {
                                            if last_kept {
                                                let start_pct = segment_start as f32 / total_frames as f32;
                                                let end_pct = f as f32 / total_frames as f32;
                                                let seg_rect = Rect::from_min_max(
                                                    pos2(rect.left() + rect.width() * start_pct, rect.top()),
                                                    pos2(rect.left() + rect.width() * end_pct, rect.bottom()),
                                                );
                                                painter.rect_filled(seg_rect, 0.0, last_color);
                                            }
                                            segment_start = f;
                                            current_type = Some((is_kept, color));
                                        }
                                    } else {
                                        segment_start = f;
                                        current_type = Some((is_kept, color));
                                    }
                                }
                            }

                            if let Some((last_kept, last_color)) = current_type {
                                if last_kept {
                                    let start_pct = segment_start as f32 / total_frames as f32;
                                    let end_pct = 1.0_f32;
                                    let seg_rect = Rect::from_min_max(
                                        pos2(rect.left() + rect.width() * start_pct, rect.top()),
                                        pos2(rect.left() + rect.width() * end_pct, rect.bottom()),
                                    );
                                    painter.rect_filled(seg_rect, 0.0, last_color);
                                }
                            }

                            painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, palette.border_color), StrokeKind::Inside);

                            // Active export playhead
                            let playhead_pct = (*current_frame as f32 / total_frames as f32).clamp(0.0, 1.0);
                            let playhead_x = rect.left() + rect.width() * playhead_pct;
                            painter.line_segment(
                                [pos2(playhead_x, rect.top() - 2.0), pos2(playhead_x, rect.bottom() + 2.0)],
                                Stroke::new(2.0_f32, palette.accent),
                            );
                        }
                    }

                    ui.add_space(6.0);
                    ui.label(RichText::new(message).size(11.0).color(palette.text_muted));
                    ui.add_space(8.0);

                    if ui.button(RichText::new("Cancel").color(palette.warn_color)).clicked() {
                        app.export_cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
            });
    }
}
