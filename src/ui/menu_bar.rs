use eframe::egui;
use crate::settings::PlayheadMode;
use crate::ui::MainApp;

pub fn render_menu_bar(app: &mut MainApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    
    egui::MenuBar::new().ui(ui, |ui| {
        ui.style_mut().spacing.button_padding = egui::vec2(6.0, 3.0);

        // --- File Menu ---
        ui.menu_button("File", |ui| {
            if ui.add(egui::Button::new("Open Video...").shortcut_text("Ctrl+O")).clicked() {
                ui.close();
                app.open_file_dialog();
            }

            if !app.settings.general.recent_files.is_empty() {
                ui.menu_button("Recent Files", |ui| {
                    let mut file_to_open = None;
                    for path in &app.settings.general.recent_files {
                        let display_name = std::path::Path::new(path)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(path);
                        if ui.button(display_name).on_hover_text(path).clicked() {
                            file_to_open = Some(path.clone());
                            ui.close();
                        }
                    }
                    if let Some(p) = file_to_open {
                        app.load_video_from_path(&p);
                    }
                });
            }

            ui.separator();

            let can_export = app.video_path.is_some() && (!app.ranges.is_empty() || app.cut_mode);
            if ui.add_enabled(can_export, egui::Button::new("Smart Export Video...").shortcut_text("Ctrl+E")).clicked() {
                ui.close();
                app.trigger_export_dialog();
            }

            ui.separator();

            if ui.add_enabled(app.video_path.is_some(), egui::Button::new("Close Video").shortcut_text("Ctrl+W")).clicked() {
                ui.close();
                app.close_current_video();
            }

            if ui.add(egui::Button::new("Settings...").shortcut_text("Ctrl+,")).clicked() {
                ui.close();
                app.open_settings_modal();
            }

            ui.separator();

            if ui.add(egui::Button::new("Quit").shortcut_text("Ctrl+Q")).clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        // --- Edit Menu ---
        ui.menu_button("Edit", |ui| {
            let cut_label = if app.cut_mode { "Mode: CUT (Snippets crop out)" } else { "Mode: KEEP (Snippets preserve)" };
            if ui.add(egui::Button::new(cut_label).shortcut_text("C")).clicked() {
                app.cut_mode = !app.cut_mode;
                ui.close();
            }

            ui.separator();

            let has_video = app.video_path.is_some();
            if ui.add_enabled(has_video, egui::Button::new("Mark In Point").shortcut_text("I or [")).clicked() {
                app.in_marker = Some(app.playback.current_frame());
                ui.close();
            }

            if ui.add_enabled(has_video, egui::Button::new("Mark Out Point").shortcut_text("O or ]")).clicked() {
                app.out_marker = Some(app.playback.current_frame());
                ui.close();
            }

            let can_add_snippet = app.in_marker.is_some() && app.out_marker.is_some();
            if ui.add_enabled(can_add_snippet, egui::Button::new("Add Snippet from In/Out").shortcut_text("Enter")).clicked() {
                app.add_snippet_from_markers();
                ui.close();
            }

            if ui.add_enabled(app.in_marker.is_some() || app.out_marker.is_some(), egui::Button::new("Clear In/Out Markers").shortcut_text("X")).clicked() {
                app.in_marker = None;
                app.out_marker = None;
                ui.close();
            }

            ui.separator();

            if ui.add_enabled(!app.ranges.is_empty(), egui::Button::new("Clear All Snippets")).clicked() {
                app.ranges.clear();
                app.next_range_id = 1;
                ui.close();
            }
        });

        // --- Playback Menu ---
        ui.menu_button("Playback", |ui| {
            let has_video = app.video_path.is_some();
            let play_label = if app.playback.is_playing() { "Pause" } else { "Play" };
            if ui.add_enabled(has_video, egui::Button::new(play_label).shortcut_text("Space")).clicked() {
                app.playback.toggle_playback();
                ui.close();
            }

            if ui.add_enabled(has_video, egui::Button::new("Step Forward 1 Frame").shortcut_text("→")).clicked() {
                app.playback.step_forward();
                ui.close();
            }

            if ui.add_enabled(has_video, egui::Button::new("Step Backward 1 Frame").shortcut_text("←")).clicked() {
                app.playback.step_backward();
                ui.close();
            }

            if ui.add_enabled(has_video, egui::Button::new("Jump to Start").shortcut_text("Home")).clicked() {
                app.playback.seek_internal(0, false, true);
                ui.close();
            }

            if ui.add_enabled(has_video, egui::Button::new("Jump to End").shortcut_text("End")).clicked() {
                app.playback.seek_internal(app.playback.total_frames(), false, true);
                ui.close();
            }

            ui.separator();

            ui.menu_button("Playhead Navigation", |ui| {
                let current_mode = app.settings.playback.playhead_mode;
                if ui.radio(current_mode == PlayheadMode::CenterAnchored, "Center-Locked (Scrolling filmstrip)").clicked() {
                    app.settings.playback.playhead_mode = PlayheadMode::CenterAnchored;
                    app.pending_settings.playback.playhead_mode = PlayheadMode::CenterAnchored;
                    ui.close();
                }
                if ui.radio(current_mode == PlayheadMode::FreeTravelling, "Free-Moving (Travelling cursor)").clicked() {
                    app.settings.playback.playhead_mode = PlayheadMode::FreeTravelling;
                    app.pending_settings.playback.playhead_mode = PlayheadMode::FreeTravelling;
                    ui.close();
                }
            });
        });

        // --- View Menu ---
        ui.menu_button("View", |ui| {
            let snip_hud_label = if app.show_snippets_hud { "Hide Floating Snippets HUD" } else { "Show Floating Snippets HUD" };
            if ui.add(egui::Button::new(snip_hud_label).shortcut_text("H")).clicked() {
                app.show_snippets_hud = !app.show_snippets_hud;
                ui.close();
            }

            let pin_label = if app.timeline_pinned_expanded { "Unpin Timeline (Auto-Expand on Hover)" } else { "Pin Timeline Expanded" };
            if ui.add(egui::Button::new(pin_label).shortcut_text("P")).clicked() {
                app.timeline_pinned_expanded = !app.timeline_pinned_expanded;
                ui.close();
            }

            let hud_label = if app.show_debug_hud { "Hide Diagnostics HUD" } else { "Show Diagnostics HUD" };
            if ui.add(egui::Button::new(hud_label).shortcut_text("Ctrl+D")).clicked() {
                app.show_debug_hud = !app.show_debug_hud;
                ui.close();
            }

            ui.separator();

            if ui.add(egui::Button::new("Zoom In").shortcut_text("+")).clicked() {
                app.zoom_level = (app.zoom_level + 0.1).clamp(0.0, 1.0);
                ui.close();
            }

            if ui.add(egui::Button::new("Zoom Out").shortcut_text("-")).clicked() {
                app.zoom_level = (app.zoom_level - 0.1).clamp(0.0, 1.0);
                ui.close();
            }

            if ui.add(egui::Button::new("Reset Zoom").shortcut_text("Ctrl+0")).clicked() {
                app.zoom_level = 0.5;
                ui.close();
            }
        });

        // --- Labels Menu ---
        ui.menu_button("Labels", |ui| {
            let in_label_mode = app.label_session.is_some();
            let toggle_label = if in_label_mode { "Exit Label Mode" } else { "Enter Label Mode" };
            if ui.add_enabled(app.video_path.is_some(), egui::Button::new(toggle_label)).clicked() {
                app.toggle_label_mode();
                ui.close();
            }

            ui.separator();

            if in_label_mode {
                if ui.add(egui::Button::new("Add Break at Playhead").shortcut_text("B")).clicked() {
                    if let Some(session) = &mut app.label_session {
                        session.add_break_at(app.playback.current_frame() as u64);
                    }
                    ui.close();
                }

                if ui.add(egui::Button::new("Delete Selected Break").shortcut_text("Del")).clicked() {
                    if let Some(session) = &mut app.label_session {
                        if let Some(idx) = session.selected_break {
                            session.remove_break(idx);
                        }
                    }
                    ui.close();
                }

                if ui.add(egui::Button::new("Toggle Commercial / Promo").shortcut_text("T")).clicked() {
                    if let Some(session) = &mut app.label_session {
                        if let Some(idx) = session.selected_break {
                            session.toggle_break_type(idx);
                        }
                    }
                    ui.close();
                }

                ui.separator();

                if ui.add(egui::Button::new("Save Ground Truth").shortcut_text("Ctrl+Shift+S")).clicked() {
                    app.save_ground_truth();
                    ui.close();
                }

                if ui.add(egui::Button::new("Load Detection Sidecar (.json)...")).clicked() {
                    ui.close();
                    app.load_detection_dialog();
                }
            } else {
                ui.label(egui::RichText::new("Enter Label Mode to edit training markers").color(app.theme_palette().text_muted).size(11.0));
            }
        });

        // --- Help Menu ---
        ui.menu_button("Help", |ui| {
            if ui.button("Keyboard Shortcuts Reference").clicked() {
                app.show_settings_modal = true;
                ui.close();
            }
            ui.separator();
            ui.label(egui::RichText::new("Snippy Pro v2.0").strong().size(12.0));
            ui.label(egui::RichText::new("Native High-Performance Video Editor").size(11.0).color(app.theme_palette().text_muted));
        });
    });
}
