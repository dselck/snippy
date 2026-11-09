use eframe::egui::{self, Color32, RichText, Stroke, pos2, vec2};
use crate::notifications::Notification;
use crate::ui::icons::{self, IconType};
use crate::ui::MainApp;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InspectorTab {
    Snippets,
    MediaInfo,
    BreakLabels,
}

pub fn render_floating_inspector_hud(app: &mut MainApp, ctx: &egui::Context) {
    if !app.show_snippets_hud {
        return;
    }

    let palette = app.theme_palette();
    let screen_rect = ctx.content_rect();
    let default_pos = pos2((screen_rect.right() - 320.0).max(10.0), 74.0);
    let is_collapsed = app.snippets_hud_collapsed;

    let mut open = app.show_snippets_hud;

    egui::Window::new("Snippets HUD")
        .id(egui::Id::new("floating_snippets_hud_window"))
        .default_pos(default_pos)
        .default_width(300.0)
        .resizable(!is_collapsed)
        .collapsible(false)
        .title_bar(false)
        .frame(
            egui::Frame::NONE
                .fill(Color32::from_rgba_premultiplied(18, 18, 22, 235))
                .stroke(Stroke::new(1.0_f32, palette.border_color))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(10, 8)),
        )
        .show(ctx, |ui| {
            // --- 1. Custom Studio Header Bar (Drag handle, Title, Roll-up chevron, Close) ---
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);

                // Scissor Icon
                let (icon_rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                icons::draw_icon(ui.painter(), icon_rect, IconType::Scissor, palette.accent);

                // Title
                let title_text = if app.label_session.is_some() { "BREAK LABELS" } else { "SNIPPETS" };
                ui.label(RichText::new(title_text).size(12.0).strong().color(palette.text_normal));

                // Snippet count pill
                let count = if app.label_session.is_some() {
                    app.label_session.as_ref().map_or(0, |s| s.breaks.len())
                } else {
                    app.ranges.len()
                };
                let count_color = if count > 0 { palette.accent } else { palette.text_muted };
                ui.label(RichText::new(format!("({})", count)).size(11.0).strong().color(count_color));

                // Right-aligned actions (Roll-up chevron & Close)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Close button ✕
                    if ui.add(egui::Button::new(RichText::new("✕").size(10.0).color(palette.text_muted)).frame(false)).clicked() {
                        open = false;
                    }

                    // Roll-up / Unroll chevron ▲ / ▼
                    let chev = if is_collapsed { "▼" } else { "▲" };
                    let tip = if is_collapsed { "Expand HUD (show snippets list)" } else { "Roll up HUD (minimize)" };
                    if ui.add(egui::Button::new(RichText::new(chev).size(11.0).color(palette.text_normal)).frame(false)).on_hover_text(tip).clicked() {
                        app.snippets_hud_collapsed = !app.snippets_hud_collapsed;
                    }

                    // If collapsed, show a quick duration summary right in the header!
                    if is_collapsed && app.label_session.is_none() {
                        let fps = app.playback.video_fps();
                        let mut cut_frames = 0usize;
                        for r in &app.ranges {
                            cut_frames += r.out_frame.saturating_sub(r.in_frame);
                        }
                        let cut_sec = if fps > 0.0 { cut_frames as f64 / fps } else { 0.0 };
                        let dur_str = app.format_seconds(cut_sec);
                        ui.label(RichText::new(format!("{} cut", dur_str)).size(10.0).monospace().color(palette.cut_color));
                    }
                });
            });

            // If collapsed, we don't render the tab body
            if is_collapsed {
                return;
            }

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(4.0);

            // --- 2. Tab Navigation & Content ---
            render_inspector(app, ui);
        });

    app.show_snippets_hud = open;
}

pub fn render_inspector(app: &mut MainApp, ui: &mut egui::Ui) {
    let palette = app.theme_palette();
    
    // Choose active tab (default to BreakLabels if label session active, otherwise Snippets)
    let in_label_mode = app.label_session.is_some();

    ui.vertical(|ui| {
        // 1. Tab Header Bar
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);

            // Adapt active tab
            if in_label_mode && app.active_inspector_tab != InspectorTab::BreakLabels && app.active_inspector_tab != InspectorTab::MediaInfo {
                app.active_inspector_tab = InspectorTab::BreakLabels;
            } else if !in_label_mode && app.active_inspector_tab == InspectorTab::BreakLabels {
                app.active_inspector_tab = InspectorTab::Snippets;
            }

            // Tab Button: Snippets
            let snip_label = format!("Snippets ({})", app.ranges.len());
            let is_snip = app.active_inspector_tab == InspectorTab::Snippets;
            if ui.add(
                egui::Button::new(
                    RichText::new(snip_label)
                        .size(11.0)
                        .strong()
                        .color(if is_snip { Color32::WHITE } else { palette.text_muted })
                )
                .fill(if is_snip { palette.accent } else { palette.widget_inactive_bg })
                .corner_radius(2.0)
            ).clicked() {
                app.active_inspector_tab = InspectorTab::Snippets;
            }

            // Tab Button: Media Info
            let is_media = app.active_inspector_tab == InspectorTab::MediaInfo;
            if ui.add(
                egui::Button::new(
                    RichText::new("Media Info")
                        .size(11.0)
                        .strong()
                        .color(if is_media { Color32::WHITE } else { palette.text_muted })
                )
                .fill(if is_media { palette.accent } else { palette.widget_inactive_bg })
                .corner_radius(2.0)
            ).clicked() {
                app.active_inspector_tab = InspectorTab::MediaInfo;
            }

            // Tab Button: Break Labels (only in Label Mode)
            if in_label_mode {
                let breaks_count = app.label_session.as_ref().map_or(0, |s| s.breaks.len());
                let brk_label = format!("Labels ({})", breaks_count);
                let is_brk = app.active_inspector_tab == InspectorTab::BreakLabels;
                if ui.add(
                    egui::Button::new(
                        RichText::new(brk_label)
                            .size(11.0)
                            .strong()
                            .color(if is_brk { Color32::WHITE } else { palette.text_muted })
                    )
                    .fill(if is_brk { palette.warn_color } else { palette.widget_inactive_bg })
                    .corner_radius(2.0)
                ).clicked() {
                    app.active_inspector_tab = InspectorTab::BreakLabels;
                }
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);

        // 2. Tab Content View
        match app.active_inspector_tab {
            InspectorTab::Snippets => render_snippets_tab(app, ui, &palette),
            InspectorTab::MediaInfo => render_media_info_tab(app, ui, &palette),
            InspectorTab::BreakLabels => render_break_labels_tab(app, ui, &palette),
        }
    });
}

fn render_snippets_tab(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    let fps = app.playback.video_fps();
    let total_frames = app.playback.total_frames();

    // Summary Statistics Header
    let mut total_snippet_frames = 0usize;
    for r in &app.ranges {
        total_snippet_frames += r.out_frame.saturating_sub(r.in_frame);
    }

    let kept_frames = if app.cut_mode {
        total_frames.saturating_sub(total_snippet_frames)
    } else {
        total_snippet_frames
    };
    let cut_frames = total_frames.saturating_sub(kept_frames);

    let kept_sec = if fps > 0.0 { kept_frames as f64 / fps } else { 0.0 };
    let cut_sec = if fps > 0.0 { cut_frames as f64 / fps } else { 0.0 };

    egui::Frame::NONE
        .fill(palette.panel_bg)
        .stroke(Stroke::new(1.0_f32, palette.border_color))
        .corner_radius(2.0)
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Kept:").size(11.0).color(palette.text_muted));
                ui.label(RichText::new(app.format_seconds(kept_sec)).size(11.0).strong().monospace().color(palette.keep_color));
                ui.add_space(8.0);
                ui.label(RichText::new("Cut:").size(11.0).color(palette.text_muted));
                ui.label(RichText::new(app.format_seconds(cut_sec)).size(11.0).strong().monospace().color(palette.cut_color));
            });
        });

    ui.add_space(6.0);

    // Contextual Choice Card for active In/Out selection
    if let (Some(inf), Some(outf)) = (app.in_marker, app.out_marker) {
        if inf < outf {
            let dur_frames = outf.saturating_sub(inf);
            let dur_sec = if fps > 0.0 { dur_frames as f64 / fps } else { 0.0 };
            egui::Frame::NONE
                .fill(palette.widget_inactive_bg)
                .stroke(Stroke::new(1.0_f32, palette.accent))
                .corner_radius(3.0)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new("SELECTION").size(9.0).strong().color(palette.accent));
                            ui.label(RichText::new(format!("{} → {} ({:.2}s)", app.frame_to_time(inf), app.frame_to_time(outf), dur_sec)).size(10.0).monospace().color(Color32::WHITE));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let add_btn = egui::Button::new(RichText::new("+ Add").size(10.0).strong().color(Color32::WHITE))
                                .fill(palette.accent)
                                .corner_radius(2.0);
                            if ui.add(add_btn).clicked() {
                                app.add_snippet_from_markers();
                            }
                        });
                    });
                });
            ui.add_space(6.0);
        }
    }

    // List of Snippets
    if app.ranges.is_empty() {
        ui.add_space(16.0);
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("No snippets added.\nMark In [ and Out ] on timeline.")
                    .size(11.0)
                    .color(palette.text_muted)
            );
        });
        return;
    }

    let mut to_delete = None;
    let mut seek_to = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (idx, range) in app.ranges.iter().enumerate() {
            let in_str = app.frame_to_time(range.in_frame);
            let out_str = app.frame_to_time(range.out_frame);
            let dur_frames = range.out_frame.saturating_sub(range.in_frame);
            let dur_sec = if fps > 0.0 { dur_frames as f64 / fps } else { 0.0 };
            let dur_str = format!("{:.2}s", dur_sec);

            egui::Frame::NONE
                .fill(palette.widget_inactive_bg)
                .stroke(Stroke::new(1.0_f32, palette.border_color))
                .corner_radius(2.0)
                .inner_margin(egui::Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Badge #
                        ui.label(
                            RichText::new(format!("{:02}", range.id))
                                .monospace()
                                .size(11.0)
                                .color(palette.accent)
                                .strong()
                        );

                        // Timecode range (clickable to seek)
                        let tc_lbl = egui::Label::new(
                            RichText::new(format!("{} → {}", in_str, out_str))
                                .monospace()
                                .size(11.0)
                                .color(palette.text_normal)
                        ).sense(egui::Sense::click());

                        if ui.add(tc_lbl).on_hover_text("Jump to snippet start").clicked() {
                            seek_to = Some(range.in_frame);
                        }

                        // Right-aligned Duration and Delete action
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icons::icon_button(ui, IconType::Trash, vec2(20.0, 20.0), false, "Remove snippet").clicked() {
                                to_delete = Some(idx);
                            }

                            ui.label(
                                RichText::new(dur_str)
                                    .monospace()
                                    .size(10.0)
                                    .color(palette.text_muted)
                            );
                        });
                    });
                });

            ui.add_space(4.0);
        }
    });

    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        let mode_text = if app.cut_mode { "Cut Mode" } else { "Keep Mode" };
        let mode_col = if app.cut_mode { palette.cut_color } else { palette.keep_color };
        if ui.button(RichText::new(mode_text).size(10.0).strong().color(mode_col)).on_hover_text("Toggle Cut/Keep snippet mode (C)").clicked() {
            app.cut_mode = !app.cut_mode;
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(RichText::new("Clear All").size(10.0).color(palette.text_muted)).clicked() {
                app.ranges.clear();
                app.next_range_id = 1;
            }
        });
    });

    if let Some(seek_frame) = seek_to {
        app.playback.seek_internal(seek_frame, false, true);
    }

    if let Some(del_idx) = to_delete {
        app.ranges.remove(del_idx);
        app.reindex_ranges();
    }
}

fn render_media_info_tab(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    if let Some(path) = &app.video_path {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(RichText::new("FILE PROPERTIES").size(10.0).strong().color(palette.accent));
            ui.add_space(4.0);

            info_row(ui, "Path", path, palette);
            
            if let Ok(metadata) = std::fs::metadata(path) {
                let bytes = metadata.len();
                let mb = bytes as f64 / (1024.0 * 1024.0);
                info_row(ui, "Size", &format!("{:.1} MB ({} bytes)", mb, bytes), palette);
            }

            ui.add_space(10.0);
            ui.label(RichText::new("VIDEO STREAM").size(10.0).strong().color(palette.accent));
            ui.add_space(4.0);

            let fps = app.playback.video_fps();
            let total_frames = app.playback.total_frames();
            info_row(ui, "Frame Rate", &format!("{:.3} fps", fps), palette);
            info_row(ui, "Total Frames", &format!("{}", total_frames), palette);

            if let Some(meta) = app.playback.metadata() {
                let gop_sec = if meta.keyframes.len() > 1 && fps > 0.0 {
                    (total_frames as f64 / meta.keyframes.len() as f64) / fps
                } else {
                    0.0
                };
                info_row(ui, "Keyframes", &format!("{} (GOPs: ~{:.1}s)", meta.keyframes.len(), gop_sec), palette);
            }

            ui.add_space(10.0);
            ui.label(RichText::new("AUDIO STREAMS").size(10.0).strong().color(palette.accent));
            ui.add_space(4.0);

            let sample_rate = app.playback.master_clock().map_or(48000, |mc| mc.sample_rate());
            info_row(ui, "Sample Rate", &format!("{} Hz", sample_rate), palette);

            // Audio Stream Selection for multi-track files
            ui.horizontal(|ui| {
                ui.label(RichText::new("Audio Profile:").size(11.0).color(palette.text_muted));
                let current_track = app.settings.playback.selected_audio_track;
                let track_str = current_track.map_or("Primary Stream".to_string(), |t| format!("Track #{}", t + 1));

                egui::ComboBox::from_id_salt("audio_stream_selector")
                    .selected_text(track_str)
                    .show_ui(ui, |ui| {
                        if ui.selectable_value(&mut app.settings.playback.selected_audio_track, None, "Primary Stream (Auto)").clicked() {
                            app.pending_settings.playback.selected_audio_track = None;
                        }
                        if ui.selectable_value(&mut app.settings.playback.selected_audio_track, Some(0), "Track #1 (Stereo)").clicked() {
                            app.pending_settings.playback.selected_audio_track = Some(0);
                        }
                        if ui.selectable_value(&mut app.settings.playback.selected_audio_track, Some(1), "Track #2 (Aux / Mic)").clicked() {
                            app.pending_settings.playback.selected_audio_track = Some(1);
                        }
                    });
            });
        });
    } else {
        ui.centered_and_justified(|ui| {
            ui.label(RichText::new("No media file loaded").size(12.0).color(palette.text_muted));
        });
    }
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str, palette: &crate::ui::ThemePalette) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{}:", label)).size(11.0).color(palette.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(value).size(11.0).monospace().color(palette.text_normal));
        });
    });
}

fn format_frame_seconds(frame: usize, fps: f64) -> String {
    let total_seconds = if fps > 0.0 { frame as f64 / fps } else { 0.0 };
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

fn render_break_labels_tab(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    let fps = app.playback.video_fps();
    if let Some(session) = &mut app.label_session {
        // Channel & Region metadata fields
        ui.horizontal(|ui| {
            ui.label(RichText::new("Channel:").size(11.0).color(palette.text_muted));
            if ui.text_edit_singleline(&mut session.channel).changed() {
                session.dirty = true;
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Region:").size(11.0).color(palette.text_muted));
            if ui.text_edit_singleline(&mut session.region).changed() {
                session.dirty = true;
            }
        });

        ui.add_space(6.0);

        // Ground Truth Save Button
        let save_text = if session.dirty { "Save Ground Truth*" } else { "Save Ground Truth" };
        let save_col = if session.dirty { palette.warn_color } else { palette.text_normal };
        if ui.button(RichText::new(save_text).size(11.0).color(save_col)).clicked() {
            match session.save() {
                Ok(_) => app.notifications.push(Notification::info("Ground truth saved successfully!".to_string())),
                Err(e) => app.notifications.push(Notification::error(format!("Failed to save ground truth: {}", e))),
            }
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        let mut to_delete = None;
        let mut toggle_idx = None;
        let mut seek_frame = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (idx, brk) in session.breaks.iter().enumerate() {
                let is_sel = session.selected_break == Some(idx);
                let fill = if is_sel { palette.widget_hovered_bg } else { palette.widget_inactive_bg };

                egui::Frame::NONE
                    .fill(fill)
                    .stroke(Stroke::new(1.0_f32, if is_sel { palette.accent } else { palette.border_color }))
                    .corner_radius(2.0)
                    .inner_margin(egui::Margin::symmetric(8, 6))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("[{:02}]", idx + 1))
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.warn_color)
                                        .strong()
                                );

                                let in_str = format_frame_seconds(brk.start_cut_frame as usize, fps);
                                let out_str = format_frame_seconds(brk.end_cut_frame as usize, fps);
                                let time_lbl = egui::Label::new(
                                    RichText::new(format!("{} → {}", in_str, out_str))
                                        .monospace()
                                        .size(11.0)
                                        .color(palette.text_normal)
                                ).sense(egui::Sense::click());

                                if ui.add(time_lbl).clicked() {
                                    seek_frame = Some((idx, brk.start_cut_frame as usize));
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if icons::icon_button(ui, IconType::Trash, vec2(18.0, 18.0), false, "Remove break").clicked() {
                                        to_delete = Some(idx);
                                    }

                                    let (tag_str, tag_col) = match brk.break_type {
                                        snippy_schema::BreakType::Commercial => ("AD", palette.warn_color),
                                        snippy_schema::BreakType::Promo => ("PROMO", palette.secondary),
                                    };
                                    let type_btn = egui::Button::new(
                                        RichText::new(tag_str).strong().size(9.0).color(Color32::WHITE)
                                    ).fill(tag_col).corner_radius(2.0);

                                    if ui.add(type_btn).on_hover_text("Toggle between Commercial (AD) and Promo").clicked() {
                                        toggle_idx = Some(idx);
                                    }
                                });
                            });
                        });
                    });

                ui.add_space(4.0);
            }
        });

        if let Some((idx, f)) = seek_frame {
            session.selected_break = Some(idx);
            app.playback.seek_internal(f, false, true);
        }
        if let Some(del) = to_delete {
            session.remove_break(del);
        }
        if let Some(t_idx) = toggle_idx {
            session.toggle_break_type(t_idx);
        }
    }
}
