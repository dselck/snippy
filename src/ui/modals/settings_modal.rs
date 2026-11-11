use eframe::egui::{self, Color32, RichText, Stroke, vec2};
use crate::settings::{CustomMode, PlayheadMode, ProlongedDropoutBehavior, Settings, Theme};
use crate::ui::theme::{self, oklch_to_rgb, rgb_to_oklch};
use crate::ui::MainApp;

#[derive(Clone, Copy, Debug, PartialEq)]
enum SettingsCategory {
    Playback,
    Appearance,
    Hardware,
    Thumbnails,
    General,
    Shortcuts,
}

pub fn render_settings_modal(app: &mut MainApp, ctx: &egui::Context) {
    let palette = app.theme_palette();

    egui::Window::new("Preferences & Settings")
        .id(egui::Id::new("settings_modal"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([620.0, 480.0])
        .frame(
            egui::Frame::window(&ctx.global_style())
                .fill(palette.panel_bg)
                .stroke(Stroke::new(1.0_f32, palette.border_color))
                .corner_radius(3.0)
                .inner_margin(egui::Margin::same(16)),
        )
        .show(ctx, |ui| {
            ui.set_width(588.0);

            // Title
            ui.horizontal(|ui| {
                ui.label(RichText::new("Preferences").size(16.0).color(palette.text_normal).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("Ctrl+,").size(11.0).color(palette.text_muted).monospace());
                });
            });

            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);

            // Category Sidebar + Content Split
            let cat_id = ui.make_persistent_id("settings_category");
            let mut current_cat = ui.data(|d| d.get_temp::<SettingsCategory>(cat_id))
                .unwrap_or(SettingsCategory::Playback);

            ui.horizontal(|ui| {
                // Category List (Left Column, 140px)
                ui.allocate_ui_with_layout(
                    vec2(140.0, 360.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 3.0);

                        let mut cat_button = |cat: SettingsCategory, label: &str| {
                            let is_sel = current_cat == cat;
                            let btn = egui::Button::new(
                                RichText::new(label)
                                    .size(11.5)
                                    .strong()
                                    .color(if is_sel { Color32::WHITE } else { palette.text_normal })
                            )
                            .fill(if is_sel { palette.accent } else { palette.widget_inactive_bg })
                            .corner_radius(2.0)
                            .min_size(vec2(136.0, 26.0));

                            if ui.add(btn).clicked() {
                                current_cat = cat;
                            }
                        };

                        cat_button(SettingsCategory::Playback, "Playback & Nav");
                        cat_button(SettingsCategory::Appearance, "Appearance");
                        cat_button(SettingsCategory::Hardware, "Hardware Accel");
                        cat_button(SettingsCategory::Thumbnails, "Thumbnails");
                        cat_button(SettingsCategory::General, "General & Paths");
                        cat_button(SettingsCategory::Shortcuts, "Shortcuts");
                    },
                );

                ui.separator();

                // Settings Content (Right Column, ~420px)
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 360.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            match current_cat {
                                SettingsCategory::Playback => render_playback_settings(app, ui, &palette),
                                SettingsCategory::Appearance => render_appearance_settings(app, ui, &palette, ctx),
                                SettingsCategory::Hardware => render_hardware_settings(app, ui, &palette),
                                SettingsCategory::Thumbnails => render_thumbnails_settings(app, ui, &palette),
                                SettingsCategory::General => render_general_settings(app, ui, &palette),
                                SettingsCategory::Shortcuts => render_shortcuts_reference(ui, &palette),
                            }
                        });
                    },
                );
            });

            ui.data_mut(|d| d.insert_temp(cat_id, current_cat));

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            // Bottom Action Bar: Reset, Cancel, Save
            ui.horizontal(|ui| {
                if ui.button("Reset Defaults").clicked() {
                    app.pending_settings = Settings::default();
                    theme::configure_theme(ctx, &app.pending_settings.appearance);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(
                        egui::Button::new(RichText::new("Save & Apply").strong().color(Color32::WHITE))
                            .fill(palette.accent),
                    )
                    .clicked()
                    {
                        app.commit_settings(ctx);
                        app.show_settings_modal = false;
                    }

                    if ui.button("Cancel").clicked() {
                        theme::configure_theme(ctx, &app.settings.appearance);
                        app.show_settings_modal = false;
                    }
                });
            });
        });
}

fn render_playback_settings(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    ui.label(RichText::new("PLAYBACK & NAVIGATION").size(11.0).strong().color(palette.accent));
    ui.add_space(6.0);

    // Playhead mode
    ui.label(RichText::new("Timeline Playhead Navigation:").strong().size(11.0).color(palette.text_normal));
    ui.horizontal(|ui| {
        if ui.radio_value(
            &mut app.pending_settings.playback.playhead_mode,
            PlayheadMode::CenterAnchored,
            "Center-Locked (Scrolling Filmstrip)",
        ).clicked() {
            app.settings.playback.playhead_mode = PlayheadMode::CenterAnchored;
        }
    });
    ui.horizontal(|ui| {
        if ui.radio_value(
            &mut app.pending_settings.playback.playhead_mode,
            PlayheadMode::FreeTravelling,
            "Free-Moving (Travelling Playhead Cursor)",
        ).clicked() {
            app.settings.playback.playhead_mode = PlayheadMode::FreeTravelling;
        }
    });

    ui.add_space(10.0);

    // Master volume
    ui.horizontal(|ui| {
        ui.label(RichText::new("Master Volume:").strong().size(11.0).color(palette.text_normal));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(format!("{:.0}%", app.pending_settings.playback.volume * 100.0)).size(11.0).monospace().color(palette.text_muted));
        });
    });
    ui.add(egui::Slider::new(&mut app.pending_settings.playback.volume, 0.0..=1.0).show_value(false));

    ui.add_space(10.0);

    // Waveform dB floor
    ui.horizontal(|ui| {
        ui.label(RichText::new("Waveform Silence Floor:").strong().size(11.0).color(palette.text_normal));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if app.pending_settings.playback.waveform_db_floor >= 0.0 {
                "Linear (0.0)".to_string()
            } else {
                format!("{:.0} dB", app.pending_settings.playback.waveform_db_floor)
            };
            ui.label(RichText::new(label).size(11.0).monospace().color(palette.text_muted));
        });
    });
    ui.add(egui::Slider::new(&mut app.pending_settings.playback.waveform_db_floor, -80.0..=0.0).show_value(false));

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);
    ui.label(RichText::new("STREAM FAULT TOLERANCE").size(10.0).strong().color(palette.accent));
    ui.add_space(6.0);

    // Prolonged dropout behavior
    ui.horizontal(|ui| {
        ui.label(RichText::new("Prolonged Dropout (>3s):").strong().size(11.0).color(palette.text_normal));
        let cur_behavior = app.pending_settings.playback.prolonged_dropout_behavior;
        let behavior_str = match cur_behavior {
            ProlongedDropoutBehavior::CutToBlack => "Cut to Black",
            ProlongedDropoutBehavior::HoldFrame => "Hold Last Good Frame",
        };
        egui::ComboBox::from_id_salt("prolonged_dropout_behavior_combo")
            .selected_text(behavior_str)
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut app.pending_settings.playback.prolonged_dropout_behavior,
                    ProlongedDropoutBehavior::CutToBlack,
                    "Cut to Black",
                );
                ui.selectable_value(
                    &mut app.pending_settings.playback.prolonged_dropout_behavior,
                    ProlongedDropoutBehavior::HoldFrame,
                    "Hold Last Good Frame",
                );
            });
    });

    ui.add_space(4.0);
    ui.checkbox(
        &mut app.pending_settings.playback.show_concealment_badges,
        "Show Concealment Badges",
    );

    ui.add_space(4.0);
    ui.checkbox(
        &mut app.pending_settings.timeline.show_timeline_defect_markers,
        "Show Defect Markers on Timeline",
    );
}

fn render_appearance_settings(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette, ctx: &egui::Context) {
    ui.label(RichText::new("APPEARANCE & STUDIO THEME").size(11.0).strong().color(palette.accent));
    ui.add_space(6.0);

    let themes = [
        (Theme::VideoEditorDark, "DaVinci Slate Dark (Default)"),
        (Theme::SleekMidnight, "Sleek Midnight"),
        (Theme::HighContrastOled, "High Contrast OLED"),
        (Theme::ClassicIndigoDark, "Classic Indigo Dark"),
        (Theme::VideoEditorLight, "Video Editor Light"),
        (Theme::ClassicWarmLight, "Classic Warm Light"),
        (Theme::CustomDynamic, "Custom Dynamic OKLCH"),
    ];

    ui.label(RichText::new("Theme Preset:").strong().size(11.0).color(palette.text_normal));
    for (t, label) in themes {
        let is_selected = app.pending_settings.appearance.theme == t;
        if ui.radio(is_selected, label).clicked() {
            app.pending_settings.appearance.theme = t;
            theme::configure_theme(ctx, &app.pending_settings.appearance);
        }
    }

    if app.pending_settings.appearance.theme == Theme::CustomDynamic {
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(RichText::new("DYNAMIC OKLCH OPTIONS").size(10.0).strong().color(palette.accent));
        ui.add_space(4.0);

        // Custom Mode
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mode:").size(11.0).color(palette.text_muted));
            if ui.radio_value(&mut app.pending_settings.appearance.custom_theme_mode, CustomMode::Dark, "Dark").clicked() {
                theme::configure_theme(ctx, &app.pending_settings.appearance);
            }
            if ui.radio_value(&mut app.pending_settings.appearance.custom_theme_mode, CustomMode::Light, "Light").clicked() {
                theme::configure_theme(ctx, &app.pending_settings.appearance);
            }
        });

        // Accent Color Picker
        ui.horizontal(|ui| {
            ui.label(RichText::new("Accent Color:").size(11.0).color(palette.text_muted));
            let cur_accent = oklch_to_rgb(
                app.pending_settings.appearance.custom_accent_lightness,
                app.pending_settings.appearance.custom_theme_chroma,
                app.pending_settings.appearance.custom_theme_hue,
            );
            let mut srgb = [cur_accent.r(), cur_accent.g(), cur_accent.b()];
            if ui.color_edit_button_srgb(&mut srgb).changed() {
                let (l, c, h) = rgb_to_oklch(srgb[0], srgb[1], srgb[2]);
                app.pending_settings.appearance.custom_accent_lightness = l;
                app.pending_settings.appearance.custom_theme_chroma = c;
                app.pending_settings.appearance.custom_theme_hue = h;
                theme::configure_theme(ctx, &app.pending_settings.appearance);
            }
        });
    }
}

fn render_hardware_settings(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    ui.label(RichText::new("HARDWARE ACCELERATION").size(11.0).strong().color(palette.accent));
    ui.add_space(6.0);

    ui.checkbox(&mut app.pending_settings.hardware.playback_hw_accel, "Enable GPU playback decoding");
    ui.label(RichText::new("Uses VideoToolbox on macOS, VAAPI on Linux, D3D11 on Windows").size(10.0).color(palette.text_muted));

    ui.add_space(8.0);

    ui.checkbox(&mut app.pending_settings.hardware.thumbnails_hw_accel, "Enable GPU thumbnail decoding");
    ui.label(RichText::new("Offloads background filmstrip harvest passes to GPU").size(10.0).color(palette.text_muted));
}

fn render_thumbnails_settings(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    ui.label(RichText::new("CACHE & THUMBNAILS").size(11.0).strong().color(palette.accent));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label(RichText::new("Thumbnail Cache Entries:").size(11.0).color(palette.text_normal));
        ui.add(egui::DragValue::new(&mut app.pending_settings.thumbnails.lru_cache_size).range(50..=2000));
    });

    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label(RichText::new("Thumbnail Worker Threads:").size(11.0).color(palette.text_normal));
        ui.add(egui::DragValue::new(&mut app.pending_settings.thumbnails.decoder_threads).range(1..=16));
    });
}

fn render_general_settings(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    ui.label(RichText::new("GENERAL & FILE PATHS").size(11.0).strong().color(palette.accent));
    ui.add_space(6.0);

    ui.checkbox(&mut app.pending_settings.general.default_cut_mode, "Default to CUT mode for new files");

    ui.add_space(10.0);
    ui.label(RichText::new("Default Open Directory:").size(11.0).color(palette.text_normal));
    ui.text_edit_singleline(&mut app.pending_settings.general.default_open_directory);

    ui.add_space(12.0);
    if ui.button("Clear Recent Files History").clicked() {
        app.pending_settings.general.recent_files.clear();
        app.settings.general.recent_files.clear();
    }
}

fn render_shortcuts_reference(ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    ui.label(RichText::new("KEYBOARD SHORTCUTS REFERENCE").size(11.0).strong().color(palette.accent));
    ui.add_space(6.0);

    let shortcuts = [
        ("Space", "Play / Pause playback"),
        ("Left Arrow", "Step backward 1 frame"),
        ("Right Arrow", "Step forward 1 frame"),
        ("Home", "Jump to start of video"),
        ("End", "Jump to end of video"),
        ("I / [", "Set In point at playhead"),
        ("O / ]", "Set Out point at playhead"),
        ("Enter", "Add In/Out range as snippet"),
        ("X", "Clear In/Out markers"),
        ("C", "Toggle Cut / Keep mode"),
        ("Ctrl+O", "Open video file dialog"),
        ("Ctrl+E", "Smart Export video"),
        ("Ctrl+,", "Open Preferences & Settings"),
        ("Ctrl+D", "Toggle Diagnostics HUD overlay"),
        ("B (Label Mode)", "Add Commercial Break at playhead"),
        ("T (Label Mode)", "Toggle Break Type (AD / PROMO)"),
        ("Del / Backspace", "Delete selected snippet or break"),
        ("Ctrl+Shift+S", "Save ground truth data"),
    ];

    for (key, desc) in shortcuts {
        ui.horizontal(|ui| {
            ui.label(RichText::new(key).size(11.0).strong().monospace().color(palette.accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(desc).size(11.0).color(palette.text_muted));
            });
        });
        ui.separator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_playback_settings_fault_tolerance() {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = MainApp::new(&cc);

        let palette = app.theme_palette();
        let _ = ctx.run_ui(Default::default(), |ui| {
            render_playback_settings(&mut app, ui, &palette);
        });

        // Test mutating the fault tolerance pending settings
        app.pending_settings.playback.prolonged_dropout_behavior = ProlongedDropoutBehavior::HoldFrame;
        app.pending_settings.playback.show_concealment_badges = false;
        app.pending_settings.timeline.show_timeline_defect_markers = false;

        app.commit_settings(&ctx);

        assert_eq!(app.settings.playback.prolonged_dropout_behavior, ProlongedDropoutBehavior::HoldFrame);
        assert!(!app.settings.playback.show_concealment_badges);
        assert!(!app.settings.timeline.show_timeline_defect_markers);
    }
}
