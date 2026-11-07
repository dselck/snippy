pub mod yuv_renderer;
pub mod theme;
pub use theme::{get_palette, ThemePalette};

use crate::media;
use crate::notifications::{Notification, NotificationKind, NotificationQueue};
use crate::settings::Settings;
use eframe::egui;
use eframe::egui::Color32;
use lru::LruCache;
use std::num::NonZeroUsize;


#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportRange {
    pub id: usize,
    pub in_frame: usize,
    pub out_frame: usize,
}

pub enum PlaybackIcon {
    Play,
    Pause,
    PrevFrame,
    NextFrame,
    Start,
    End,
    SetIn,
    SetOut,
    AddSnippet,
    LoadVideo,
    ExportVideo,
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

    // Snippets cut/keep toggle
    pub cut_mode: bool,

    // Channels & Playback



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
}

impl MainApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_fonts(&cc.egui_ctx);

        // Load settings from disk; push any load error as a Notification.
        let (settings, load_err) = Settings::load();

        // Configure theme using loaded settings
        configure_theme(&cc.egui_ctx, &settings.appearance);

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

        // Derive initial zoom level from the default Zoom Interval in settings.
        // Use a placeholder fps of 30 until a file is loaded (zoom will be reset on load).
        let initial_zoom = settings.default_zoom_level(30.0);

        // Thumbnail LRU capacity from settings.
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
            timeline_height: 192.0,
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
        }
    }

    fn frame_to_time(&self, frame: usize) -> String {
        let total_seconds = frame as f64 / self.playback.video_fps();
        let hours = (total_seconds / 3600.0).floor() as usize;
        let minutes = ((total_seconds % 3600.0) / 60.0).floor() as usize;
        let seconds = total_seconds.floor() as usize % 60;
        let centiseconds = ((total_seconds - total_seconds.floor()) * 100.0).floor() as usize;
        format!("{:02}:{:02}:{:02}.{:02}", hours, minutes, seconds, centiseconds)
    }

    pub fn is_frame_cut(&self, frame: usize) -> bool {
        is_frame_cut_helper(frame, &self.ranges, self.cut_mode)
    }

    fn draw_settings_modal(&mut self, ctx: &egui::Context) {
        use crate::settings::ZoomUnit;

        let palette  = get_palette(&self.settings.appearance);
        let accent   = palette.accent;
        let muted    = palette.text_muted;
        let warn_col = palette.warn_color;
        let card_bg  = palette.card_bg;
        let sep_col  = palette.border_color;

        // Restart-required check: settings that only take effect after media reload.
        let needs_restart = self.pending_settings.hardware.playback_hw_accel   != self.settings.hardware.playback_hw_accel
            || self.pending_settings.hardware.thumbnails_hw_accel != self.settings.hardware.thumbnails_hw_accel
            || self.pending_settings.thumbnails.decoder_threads   != self.settings.thumbnails.decoder_threads
            || self.pending_settings.thumbnails.lru_cache_size    != self.settings.thumbnails.lru_cache_size
            || self.pending_settings.advanced.presenter_lookahead_depth != self.settings.advanced.presenter_lookahead_depth;

        let mut close_modal = false;

        egui::Window::new("Settings")
            .id(egui::Id::new("settings_modal"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([480.0, 560.0])
            .frame(egui::Frame::window(&ctx.global_style()).fill(palette.panel_bg).stroke(egui::Stroke::new(1.0, sep_col)))
            .show(ctx, |ui| {
                ui.set_width(480.0);

                // Title row
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("⚙  Settings").size(18.0).color(palette.text_normal).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new("⌘,").size(12.0).color(muted));
                    });
                });
                ui.add_space(4.0);
                ui.add(egui::Separator::default().spacing(1.0));
                ui.add_space(8.0);

                egui::ScrollArea::vertical().max_height(440.0).show(ui, |ui| {
                    // ── Playback ──────────────────────────────────────────
                    ui.label(egui::RichText::new("Playback").size(13.0).color(accent).strong());
                    egui::Frame::NONE.fill(card_bg).corner_radius(8.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(452.0);
                        // Label + percentage on the same row
                        ui.horizontal(|ui| {
                            ui.label("Volume");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(egui::RichText::new(format!("{:.0}%", self.pending_settings.playback.volume * 100.0)).color(muted).size(12.0));
                            });
                        });
                        // egui Sliders use spacing.slider_width from the style rather than
                        // respecting add_sized. Capture available_width then override the style
                        // inside a scope so only this slider is affected.
                        let slider_width = ui.available_width();
                        ui.scope(|ui| {
                            ui.style_mut().spacing.slider_width = slider_width;
                            ui.add(egui::Slider::new(&mut self.pending_settings.playback.volume, 0.0..=1.0).show_value(false));
                        });
                        // Volume applies live
                        self.settings.playback.volume = self.pending_settings.playback.volume;
                    });
                    ui.add_space(12.0);

                    // ── Hardware ──────────────────────────────────────────
                    ui.label(egui::RichText::new("Hardware").size(13.0).color(accent).strong());
                    egui::Frame::NONE.fill(card_bg).corner_radius(8.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(452.0);
                        // Master HW accel toggle writes both fields simultaneously
                        let both_on = self.pending_settings.hardware.playback_hw_accel && self.pending_settings.hardware.thumbnails_hw_accel;
                        let mut master = both_on;
                        if ui.checkbox(&mut master, "Use hardware acceleration").changed() {
                            self.pending_settings.hardware.playback_hw_accel = master;
                            self.pending_settings.hardware.thumbnails_hw_accel = master;
                        }
                        ui.label(egui::RichText::new("Uses VideoToolbox on macOS, D3D11 on Windows, VAAPI on Linux.").size(11.0).color(muted));
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new("Advanced: playback_hw_accel / thumbnails_hw_accel can be set independently in the config file.").size(10.0).color(muted));
                        if needs_restart {
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("⚠  Changes take effect after reloading the file.").size(11.0).color(warn_col));
                        }
                    });
                    ui.add_space(12.0);

                    // ── Appearance ────────────────────────────────────────
                    ui.label(egui::RichText::new("Appearance").size(13.0).color(accent).strong());
                    egui::Frame::NONE.fill(card_bg).corner_radius(8.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(452.0);
                        ui.horizontal(|ui| {
                            ui.label("Theme");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                egui::ComboBox::from_id_salt("theme_selector")
                                    .selected_text(match self.pending_settings.appearance.theme {
                                        crate::settings::Theme::VideoEditorDark => "Video Editor Dark",
                                        crate::settings::Theme::SleekMidnight => "Sleek Midnight",
                                        crate::settings::Theme::HighContrastOled => "High-Contrast OLED",
                                        crate::settings::Theme::ClassicIndigoDark => "Classic Indigo Dark",
                                        crate::settings::Theme::VideoEditorLight => "Video Editor Light",
                                        crate::settings::Theme::ClassicWarmLight => "Classic Warm Light",
                                        crate::settings::Theme::CustomDynamic => "Custom Dynamic",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.pending_settings.appearance.theme, crate::settings::Theme::VideoEditorDark, "Video Editor Dark");
                                        ui.selectable_value(&mut self.pending_settings.appearance.theme, crate::settings::Theme::SleekMidnight, "Sleek Midnight");
                                        ui.selectable_value(&mut self.pending_settings.appearance.theme, crate::settings::Theme::HighContrastOled, "High-Contrast OLED");
                                        ui.selectable_value(&mut self.pending_settings.appearance.theme, crate::settings::Theme::ClassicIndigoDark, "Classic Indigo Dark");
                                        ui.selectable_value(&mut self.pending_settings.appearance.theme, crate::settings::Theme::VideoEditorLight, "Video Editor Light");
                                        ui.selectable_value(&mut self.pending_settings.appearance.theme, crate::settings::Theme::ClassicWarmLight, "Classic Warm Light");
                                        ui.selectable_value(&mut self.pending_settings.appearance.theme, crate::settings::Theme::CustomDynamic, "Custom Dynamic");
                                    });
                            });
                        });
                        if self.pending_settings.appearance.theme == crate::settings::Theme::CustomDynamic {
                            ui.add_space(8.0);
                            
                            // Mode selector: Dark, Light, Custom
                            ui.horizontal(|ui| {
                                ui.label("Mode");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let mode_text = match self.pending_settings.appearance.custom_theme_mode {
                                        crate::settings::CustomMode::Dark => "Dark",
                                        crate::settings::CustomMode::Light => "Light",
                                        crate::settings::CustomMode::Custom => "Custom",
                                    };
                                    egui::ComboBox::from_id_salt("custom_theme_mode")
                                        .selected_text(mode_text)
                                        .width(80.0)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.pending_settings.appearance.custom_theme_mode, crate::settings::CustomMode::Dark, "Dark");
                                            ui.selectable_value(&mut self.pending_settings.appearance.custom_theme_mode, crate::settings::CustomMode::Light, "Light");
                                            ui.selectable_value(&mut self.pending_settings.appearance.custom_theme_mode, crate::settings::CustomMode::Custom, "Custom");
                                        });
                                    });
                            });
                            
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label("Accent Color");
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let accent_l = match self.pending_settings.appearance.custom_theme_mode {
                                        crate::settings::CustomMode::Dark => 0.65,
                                        crate::settings::CustomMode::Light => 0.55,
                                        crate::settings::CustomMode::Custom => self.pending_settings.appearance.custom_accent_lightness,
                                    };
                                    let accent_rgb = crate::ui::theme::oklch_to_rgb(
                                        accent_l,
                                        self.pending_settings.appearance.custom_theme_chroma,
                                        self.pending_settings.appearance.custom_theme_hue,
                                    );
                                    
                                    let mut rgb_array = [accent_rgb.r(), accent_rgb.g(), accent_rgb.b()];
                                    if ui.color_edit_button_srgb(&mut rgb_array).changed() {
                                        let (l, c, h) = crate::ui::theme::rgb_to_oklch(rgb_array[0], rgb_array[1], rgb_array[2]);
                                        self.pending_settings.appearance.custom_theme_hue = h;
                                        self.pending_settings.appearance.custom_theme_chroma = c;
                                        if self.pending_settings.appearance.custom_theme_mode == crate::settings::CustomMode::Custom {
                                            self.pending_settings.appearance.custom_accent_lightness = l;
                                        }
                                    }
                                });
                            });
                            
                            ui.add_space(8.0);
                            egui::CollapsingHeader::new(egui::RichText::new("Color Theory Options").size(12.0).color(accent).strong())
                                .default_open(true)
                                .show(ui, |ui| {
                                    ui.add_space(4.0);
                                    
                                    // Semantic Style: Classic vs Harmonized
                                    ui.horizontal(|ui| {
                                        ui.label("Semantic Style");
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let style_text = match self.pending_settings.appearance.custom_semantic_style {
                                                crate::settings::SemanticStyle::Classic => "Classic (Green/Red)",
                                                crate::settings::SemanticStyle::Harmonized => "Harmonized",
                                            };
                                            egui::ComboBox::from_id_salt("custom_semantic_style")
                                                .selected_text(style_text)
                                                .width(140.0)
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_semantic_style, crate::settings::SemanticStyle::Classic, "Classic (Green/Red)");
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_semantic_style, crate::settings::SemanticStyle::Harmonized, "Harmonized");
                                                });
                                        });
                                    });

                                    ui.add_space(6.0);
                                    // Harmony selector
                                    ui.horizontal(|ui| {
                                        ui.label("Harmony");
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let harmony_text = match self.pending_settings.appearance.custom_harmony {
                                                crate::settings::ColorHarmony::Complementary => "Complementary",
                                                crate::settings::ColorHarmony::SplitComplementary => "Split-Complementary",
                                                crate::settings::ColorHarmony::Analogous => "Analogous",
                                                crate::settings::ColorHarmony::Triadic => "Triadic",
                                                crate::settings::ColorHarmony::Tetradic => "Tetradic",
                                                crate::settings::ColorHarmony::Monochromatic => "Monochromatic",
                                            };
                                            egui::ComboBox::from_id_salt("custom_harmony")
                                                .selected_text(harmony_text)
                                                .width(130.0)
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_harmony, crate::settings::ColorHarmony::Complementary, "Complementary");
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_harmony, crate::settings::ColorHarmony::SplitComplementary, "Split-Complementary");
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_harmony, crate::settings::ColorHarmony::Analogous, "Analogous");
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_harmony, crate::settings::ColorHarmony::Triadic, "Triadic");
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_harmony, crate::settings::ColorHarmony::Tetradic, "Tetradic");
                                                    ui.selectable_value(&mut self.pending_settings.appearance.custom_harmony, crate::settings::ColorHarmony::Monochromatic, "Monochromatic");
                                                });
                                        });
                                    });

                                    if self.pending_settings.appearance.custom_theme_mode != crate::settings::CustomMode::Custom {
                                        ui.add_space(6.0);
                                        ui.horizontal(|ui| {
                                            ui.label("Background Tint");
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let bg_l = match self.pending_settings.appearance.custom_theme_mode {
                                                    crate::settings::CustomMode::Dark => 0.08,
                                                    crate::settings::CustomMode::Light => 0.96,
                                                    _ => unreachable!(),
                                                };
                                                let bg_rgb = crate::ui::theme::oklch_to_rgb(
                                                    bg_l,
                                                    self.pending_settings.appearance.custom_bg_chroma,
                                                    self.pending_settings.appearance.custom_neutral_hue,
                                                );
                                                
                                                let mut rgb_array = [bg_rgb.r(), bg_rgb.g(), bg_rgb.b()];
                                                if ui.color_edit_button_srgb(&mut rgb_array).changed() {
                                                    let (_, c, h) = crate::ui::theme::rgb_to_oklch(rgb_array[0], rgb_array[1], rgb_array[2]);
                                                    self.pending_settings.appearance.custom_bg_chroma = c;
                                                    self.pending_settings.appearance.custom_neutral_hue = h;
                                                }
                                            });
                                        });
                                    }

                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        ui.label("Secondary Color");
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let accent_h = self.pending_settings.appearance.custom_theme_hue;
                                            let accent_l = match self.pending_settings.appearance.custom_theme_mode {
                                                crate::settings::CustomMode::Dark => 0.65,
                                                crate::settings::CustomMode::Light => 0.55,
                                                crate::settings::CustomMode::Custom => self.pending_settings.appearance.custom_accent_lightness,
                                            };
                                            let secondary_h = match self.pending_settings.appearance.custom_harmony {
                                                crate::settings::ColorHarmony::Complementary => (accent_h + 180.0) % 360.0,
                                                crate::settings::ColorHarmony::SplitComplementary => (accent_h + 150.0) % 360.0,
                                                crate::settings::ColorHarmony::Analogous => (accent_h + 30.0) % 360.0,
                                                crate::settings::ColorHarmony::Triadic => (accent_h + 120.0) % 360.0,
                                                crate::settings::ColorHarmony::Tetradic => (accent_h + 90.0) % 360.0,
                                                crate::settings::ColorHarmony::Monochromatic => accent_h,
                                            };
                                            let is_dark = match self.pending_settings.appearance.custom_theme_mode {
                                                crate::settings::CustomMode::Dark => true,
                                                crate::settings::CustomMode::Light => false,
                                                crate::settings::CustomMode::Custom => self.pending_settings.appearance.custom_bg_lightness < 0.5,
                                            };
                                            let secondary_l = if is_dark {
                                                (accent_l - 0.1).max(0.3)
                                            } else {
                                                (accent_l + 0.1).min(0.8)
                                            };
                                            
                                            let sec_rgb = crate::ui::theme::oklch_to_rgb(
                                                secondary_l,
                                                self.pending_settings.appearance.custom_secondary_chroma,
                                                secondary_h,
                                            );
                                            
                                            // A nice rounded color swatch preview on the right
                                            let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 16.0), egui::Sense::hover());
                                            ui.painter().rect_filled(rect, 4.0, sec_rgb);
                                            response.on_hover_text("Preview of the generated secondary color");
                                            
                                            // Slider to control the chroma/intensity of the secondary color
                                            ui.add(egui::Slider::new(&mut self.pending_settings.appearance.custom_secondary_chroma, 0.0..=0.3).show_value(false));
                                        });
                                    });
                                    
                                    if self.pending_settings.appearance.custom_theme_mode == crate::settings::CustomMode::Custom {
                                        ui.add_space(6.0);
                                        ui.horizontal(|ui| {
                                            ui.label("Background Color");
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let bg_rgb = crate::ui::theme::oklch_to_rgb(
                                                    self.pending_settings.appearance.custom_bg_lightness,
                                                    self.pending_settings.appearance.custom_bg_chroma,
                                                    self.pending_settings.appearance.custom_neutral_hue,
                                                );
                                                
                                                let mut rgb_array = [bg_rgb.r(), bg_rgb.g(), bg_rgb.b()];
                                                if ui.color_edit_button_srgb(&mut rgb_array).changed() {
                                                    let (l, c, h) = crate::ui::theme::rgb_to_oklch(rgb_array[0], rgb_array[1], rgb_array[2]);
                                                    self.pending_settings.appearance.custom_bg_lightness = l;
                                                    self.pending_settings.appearance.custom_bg_chroma = c;
                                                    self.pending_settings.appearance.custom_neutral_hue = h;
                                                }
                                            });
                                        });
                                    }
                                });
                        }
                    });
                    ui.add_space(12.0);

                    // ── Timeline ──────────────────────────────────────────
                    ui.label(egui::RichText::new("Timeline").size(13.0).color(accent).strong());
                    egui::Frame::NONE.fill(card_bg).corner_radius(8.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(452.0);
                        // Label and controls all on one row
                        ui.horizontal(|ui| {
                            ui.label("Default zoom interval");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                egui::ComboBox::from_id_salt("default_zoom_unit")
                                    .selected_text(self.pending_settings.timeline.default_zoom_unit.label())
                                    .width(80.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.pending_settings.timeline.default_zoom_unit, ZoomUnit::Seconds, "seconds");
                                        ui.selectable_value(&mut self.pending_settings.timeline.default_zoom_unit, ZoomUnit::Frames, "frames");
                                    });
                                ui.add(egui::DragValue::new(&mut self.pending_settings.timeline.default_zoom_value).range(0.001..=9999.0).speed(0.1).max_decimals(2));
                            });
                        });
                        ui.label(egui::RichText::new("Min/max zoom and curve shape are set in the config file.").size(11.0).color(muted));
                    });
                    ui.add_space(12.0);

                    // ── General ───────────────────────────────────────────
                    ui.label(egui::RichText::new("General").size(13.0).color(accent).strong());
                    egui::Frame::NONE.fill(card_bg).corner_radius(8.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                        ui.set_width(452.0);
                        
                        // Default snippets edit mode
                        ui.horizontal(|ui| {
                            ui.label("Default edit mode");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                egui::ComboBox::from_id_salt("default_cut_mode")
                                    .selected_text(if self.pending_settings.general.default_cut_mode { "Cut" } else { "Keep" })
                                    .width(80.0)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.pending_settings.general.default_cut_mode, true, "Cut");
                                        ui.selectable_value(&mut self.pending_settings.general.default_cut_mode, false, "Keep");
                                    });
                            });
                        });
                        ui.add_space(8.0);

                        ui.label("Default open directory");

                        // Build a meaningful hint showing the actual OS default path
                        let os_default_hint = dirs::document_dir()
                            .or_else(dirs::home_dir)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| "OS default".to_string());

                        // Row: TextEdit + Browse button at matched height
                        ui.horizontal(|ui| {
                            let btn_height = 28.0;
                            let text_width = ui.available_width() - 80.0; // leave room for button
                            ui.add_sized(
                                [text_width, btn_height],
                                egui::TextEdit::singleline(&mut self.pending_settings.general.default_open_directory)
                                    .hint_text(&os_default_hint),
                            );
                            if ui.add_sized([72.0, btn_height], egui::Button::new("Browse…")).clicked() {
                                let start_dir = if self.pending_settings.general.default_open_directory.is_empty() {
                                    dirs::home_dir()
                                } else {
                                    Some(std::path::PathBuf::from(&self.pending_settings.general.default_open_directory))
                                };
                                let mut dlg = rfd::FileDialog::new();
                                if let Some(d) = start_dir { dlg = dlg.set_directory(d); }
                                if let Some(dir) = dlg.pick_folder() {
                                    self.pending_settings.general.default_open_directory = dir.to_string_lossy().to_string();
                                }
                            }
                        });

                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Recent files").size(12.0).color(palette.text_normal));
                        if self.pending_settings.general.recent_files.is_empty() {
                            ui.label(egui::RichText::new("No recent files.").size(11.0).color(muted));
                        } else {
                            for path in &self.pending_settings.general.recent_files {
                                let short = std::path::Path::new(path)
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_else(|| path.clone());
                                ui.label(egui::RichText::new(format!("• {short}")).size(11.0).color(muted));
                            }
                            if ui.small_button("Clear recent files").clicked() {
                                self.pending_settings.general.recent_files.clear();
                            }
                        }
                    });
                    ui.add_space(12.0);

                    // ── Advanced / Config file ────────────────────────────
                    ui.add(egui::Separator::default().spacing(1.0));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Advanced settings are configured directly in the settings file.").size(11.0).color(muted));
                    if let Some(path) = crate::settings::config_path() {
                        let path_str = path.display().to_string();
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(&path_str).size(10.5).color(muted).monospace());
                        if ui.small_button("Open in editor").clicked() {
                            let _ = open::that(&path_str);
                        }
                    } else {
                        ui.label(egui::RichText::new("Config path unavailable on this system.").size(11.0).color(muted));
                    }
                }); // end ScrollArea

                ui.add_space(8.0);
                ui.add(egui::Separator::default().spacing(1.0));
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let done_btn = egui::Button::new(egui::RichText::new("Done").color(Color32::WHITE))
                            .fill(accent)
                            .corner_radius(6.0);
                        if ui.add(done_btn).clicked() {
                            close_modal = true;
                        }
                    });
                });
            });

        // If the appearance settings changed while the modal is open, apply them instantly in real time.
        if self.settings.appearance != self.pending_settings.appearance {
            self.settings.appearance = self.pending_settings.appearance.clone();
            configure_theme(ctx, &self.settings.appearance);
            self.settings.save();
        }

        if close_modal {
            // Commit remaining pending settings → live settings
            self.settings = self.pending_settings.clone();
            self.settings.save();
            self.show_settings_modal = false;
        }
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    // Install Outfit-Regular and Outfit-Medium
    fonts.font_data.insert(
        "outfit_regular".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../resources/fonts/Outfit-Regular.ttf"))),
    );
    fonts.font_data.insert(
        "outfit_medium".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../../resources/fonts/Outfit-Medium.ttf"))),
    );

    // Set Outfit as the default proportional font
    fonts.families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "outfit_regular".to_owned());
    
    // Fallback to Outfit-Medium for bold text styles if needed
    fonts.families
        .insert(egui::FontFamily::Name("medium".into()), vec!["outfit_medium".to_owned()]);

    ctx.set_fonts(fonts);
}

fn configure_theme(ctx: &egui::Context, appearance: &crate::settings::AppearanceSettings) {
    let palette = get_palette(appearance);
    let mut style = (*ctx.global_style()).clone();
    
    let mut visuals = if palette.is_dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    
    visuals.window_fill = palette.panel_bg;
    visuals.panel_fill = palette.panel_bg;
    
    // Configure buttons, inputs, slider styles
    visuals.widgets.noninteractive.bg_fill = palette.card_bg;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, palette.border_color);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette.text_normal);
    visuals.widgets.noninteractive.expansion = 0.0;

    visuals.widgets.inactive.bg_fill = palette.widget_inactive_bg;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, palette.border_color);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette.text_normal);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.inactive.expansion = 0.0;
 
    // Calculate a contrasting text color for active/selected items based on accent color luminance
    let accent_lum = 0.2126 * (palette.accent.r() as f32) + 0.7152 * (palette.accent.g() as f32) + 0.0722 * (palette.accent.b() as f32);
    let active_text_color = if accent_lum > 128.0 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    };

    visuals.widgets.hovered.bg_fill = palette.widget_hovered_bg;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, palette.widget_hovered_bg);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, if palette.is_dark { egui::Color32::WHITE } else { egui::Color32::BLACK });
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.hovered.expansion = 0.0;
 
    visuals.widgets.active.bg_fill = palette.accent;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, palette.accent);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, active_text_color);
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = palette.accent;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, palette.accent);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, active_text_color);
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.open.expansion = 0.0;

    // Selection styling
    visuals.selection.bg_fill = palette.accent;
    visuals.selection.stroke = egui::Stroke::new(0.0, active_text_color);

    // Global layout roundings
    visuals.window_corner_radius = egui::CornerRadius::same(12);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 15,
        spread: 0,
        color: egui::Color32::from_black_alpha(40),
    };

    visuals.extreme_bg_color = palette.timeline_track_bg;

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(10.0, 2.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.scroll.floating = true;
    ctx.set_global_style(style);
}

fn draw_playback_icon(painter: &egui::Painter, rect: egui::Rect, icon: &PlaybackIcon, color: Color32) {
    let center = rect.center();
    let size = rect.height() * 0.30; // Slimmer icon footprint

    match icon {
        PlaybackIcon::Play => {
            let p1 = egui::pos2(center.x - size * 0.35, center.y - size * 0.5);
            let p2 = egui::pos2(center.x - size * 0.35, center.y + size * 0.5);
            let p3 = egui::pos2(center.x + size * 0.55, center.y);
            painter.add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                color,
                egui::Stroke::NONE,
            ));
        }
        PlaybackIcon::Pause => {
            let w = size * 0.12; // Thinner pause bars
            let h = size * 0.8;
            let gap = size * 0.14;
            let left_rect = egui::Rect::from_center_size(egui::pos2(center.x - gap, center.y), egui::vec2(w, h));
            let right_rect = egui::Rect::from_center_size(egui::pos2(center.x + gap, center.y), egui::vec2(w, h));
            painter.rect_filled(left_rect, 1.0, color);
            painter.rect_filled(right_rect, 1.0, color);
        }
        PlaybackIcon::PrevFrame => {
            let p1 = egui::pos2(center.x + size * 0.45, center.y - size * 0.45);
            let p2 = egui::pos2(center.x + size * 0.45, center.y + size * 0.45);
            let p3 = egui::pos2(center.x - size * 0.15, center.y);
            painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], color, egui::Stroke::NONE));
            let bar = egui::Rect::from_center_size(egui::pos2(center.x - size * 0.38, center.y), egui::vec2(size * 0.12, size * 0.9));
            painter.rect_filled(bar, 1.0, color);
        }
        PlaybackIcon::NextFrame => {
            let p1 = egui::pos2(center.x - size * 0.45, center.y - size * 0.45);
            let p2 = egui::pos2(center.x - size * 0.45, center.y + size * 0.45);
            let p3 = egui::pos2(center.x + size * 0.15, center.y);
            painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], color, egui::Stroke::NONE));
            let bar = egui::Rect::from_center_size(egui::pos2(center.x + size * 0.38, center.y), egui::vec2(size * 0.12, size * 0.9));
            painter.rect_filled(bar, 1.0, color);
        }
        PlaybackIcon::Start => {
            let p1 = egui::pos2(center.x + size * 0.25, center.y - size * 0.4);
            let p2 = egui::pos2(center.x + size * 0.25, center.y + size * 0.4);
            let p3 = egui::pos2(center.x - size * 0.15, center.y);
            painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], color, egui::Stroke::NONE));

            let p1_2 = egui::pos2(center.x + size * 0.65, center.y - size * 0.4);
            let p2_2 = egui::pos2(center.x + size * 0.65, center.y + size * 0.4);
            let p3_2 = egui::pos2(center.x + size * 0.25, center.y);
            painter.add(egui::Shape::convex_polygon(vec![p1_2, p2_2, p3_2], color, egui::Stroke::NONE));

            let bar = egui::Rect::from_center_size(egui::pos2(center.x - size * 0.38, center.y), egui::vec2(size * 0.12, size * 0.8));
            painter.rect_filled(bar, 1.0, color);
        }
        PlaybackIcon::End => {
            let p1 = egui::pos2(center.x - size * 0.25, center.y - size * 0.4);
            let p2 = egui::pos2(center.x - size * 0.25, center.y + size * 0.4);
            let p3 = egui::pos2(center.x + size * 0.15, center.y);
            painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], color, egui::Stroke::NONE));

            let p1_2 = egui::pos2(center.x - size * 0.65, center.y - size * 0.4);
            let p2_2 = egui::pos2(center.x - size * 0.65, center.y + size * 0.4);
            let p3_2 = egui::pos2(center.x - size * 0.25, center.y);
            painter.add(egui::Shape::convex_polygon(vec![p1_2, p2_2, p3_2], color, egui::Stroke::NONE));

            let bar = egui::Rect::from_center_size(egui::pos2(center.x + size * 0.38, center.y), egui::vec2(size * 0.12, size * 0.8));
            painter.rect_filled(bar, 1.0, color);
        }
        PlaybackIcon::SetIn => {
            let w = size * 0.35;
            let h = size * 0.8;
            let thickness = size * 0.12;

            let left = center.x - w * 0.5;
            let right = center.x + w * 0.5;
            let top = center.y - h * 0.5;
            let bottom = center.y + h * 0.5;

            // Left vertical bar
            let vertical_bar = egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(left + thickness, bottom),
            );
            painter.rect_filled(vertical_bar, 1.0, color);

            // Top horizontal bar
            let top_bar = egui::Rect::from_min_max(
                egui::pos2(left + thickness, top),
                egui::pos2(right, top + thickness),
            );
            painter.rect_filled(top_bar, 1.0, color);

            // Bottom horizontal bar
            let bottom_bar = egui::Rect::from_min_max(
                egui::pos2(left + thickness, bottom - thickness),
                egui::pos2(right, bottom),
            );
            painter.rect_filled(bottom_bar, 1.0, color);
        }
        PlaybackIcon::SetOut => {
            let w = size * 0.35;
            let h = size * 0.8;
            let thickness = size * 0.12;

            let left = center.x - w * 0.5;
            let right = center.x + w * 0.5;
            let top = center.y - h * 0.5;
            let bottom = center.y + h * 0.5;

            // Right vertical bar
            let vertical_bar = egui::Rect::from_min_max(
                egui::pos2(right - thickness, top),
                egui::pos2(right, bottom),
            );
            painter.rect_filled(vertical_bar, 1.0, color);

            // Top horizontal bar
            let top_bar = egui::Rect::from_min_max(
                egui::pos2(left, top),
                egui::pos2(right - thickness, top + thickness),
            );
            painter.rect_filled(top_bar, 1.0, color);

            // Bottom horizontal bar
            let bottom_bar = egui::Rect::from_min_max(
                egui::pos2(left, bottom - thickness),
                egui::pos2(right - thickness, bottom),
            );
            painter.rect_filled(bottom_bar, 1.0, color);
        }
        PlaybackIcon::AddSnippet => {
            let thickness = size * 0.15;
            let len = size * 0.7;
            
            // Draw horizontal bar of the "+"
            let h_bar = egui::Rect::from_center_size(center, egui::vec2(len, thickness));
            painter.rect_filled(h_bar, 1.0, color);
            
            // Draw vertical bar of the "+"
            let v_bar = egui::Rect::from_center_size(center, egui::vec2(thickness, len));
            painter.rect_filled(v_bar, 1.0, color);
        }
        PlaybackIcon::LoadVideo => {
            let thickness = size * 0.12;
            let tray_left = egui::Rect::from_min_max(
                egui::pos2(center.x - size * 0.5, center.y + size * 0.1),
                egui::pos2(center.x - size * 0.5 + thickness, center.y + size * 0.5),
            );
            let tray_bottom = egui::Rect::from_min_max(
                egui::pos2(center.x - size * 0.5, center.y + size * 0.5 - thickness),
                egui::pos2(center.x + size * 0.5, center.y + size * 0.5),
            );
            let tray_right = egui::Rect::from_min_max(
                egui::pos2(center.x + size * 0.5 - thickness, center.y + size * 0.1),
                egui::pos2(center.x + size * 0.5, center.y + size * 0.5),
            );
            painter.rect_filled(tray_left, 1.0, color);
            painter.rect_filled(tray_bottom, 1.0, color);
            painter.rect_filled(tray_right, 1.0, color);

            let stem = egui::Rect::from_min_max(
                egui::pos2(center.x - thickness * 0.5, center.y - size * 0.5),
                egui::pos2(center.x + thickness * 0.5, center.y + size * 0.1),
            );
            painter.rect_filled(stem, 1.0, color);

            let p1 = egui::pos2(center.x - size * 0.3, center.y - size * 0.1);
            let p2 = egui::pos2(center.x + size * 0.3, center.y - size * 0.1);
            let p3 = egui::pos2(center.x, center.y + size * 0.25);
            painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], color, egui::Stroke::NONE));
        }
        PlaybackIcon::ExportVideo => {
            let thickness = size * 0.12;
            let tray_left = egui::Rect::from_min_max(
                egui::pos2(center.x - size * 0.5, center.y + size * 0.1),
                egui::pos2(center.x - size * 0.5 + thickness, center.y + size * 0.5),
            );
            let tray_bottom = egui::Rect::from_min_max(
                egui::pos2(center.x - size * 0.5, center.y + size * 0.5 - thickness),
                egui::pos2(center.x + size * 0.5, center.y + size * 0.5),
            );
            let tray_right = egui::Rect::from_min_max(
                egui::pos2(center.x + size * 0.5 - thickness, center.y + size * 0.1),
                egui::pos2(center.x + size * 0.5, center.y + size * 0.5),
            );
            painter.rect_filled(tray_left, 1.0, color);
            painter.rect_filled(tray_bottom, 1.0, color);
            painter.rect_filled(tray_right, 1.0, color);

            let stem = egui::Rect::from_min_max(
                egui::pos2(center.x - thickness * 0.5, center.y - size * 0.1),
                egui::pos2(center.x + thickness * 0.5, center.y + size * 0.3),
            );
            painter.rect_filled(stem, 1.0, color);

            let p1 = egui::pos2(center.x - size * 0.3, center.y - size * 0.1);
            let p2 = egui::pos2(center.x + size * 0.3, center.y - size * 0.1);
            let p3 = egui::pos2(center.x, center.y - size * 0.45);
            painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], color, egui::Stroke::NONE));
        }
    }
}

fn draw_scissor_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    let w = rect.width();
    let h = rect.height();
    
    // Scale everything relative to design space (24x24)
    let scale_x = w / 24.0;
    let scale_y = h / 24.0;
    
    let scale_pos = |x: f32, y: f32| {
        egui::pos2(rect.left() + x * scale_x, rect.top() + y * scale_y)
    };

    // Draw the two handles (circles at (6, 6) and (6, 18) with radius 3)
    let handle1_center = scale_pos(6.0, 6.0);
    let handle2_center = scale_pos(6.0, 18.0);
    let r = 3.0 * scale_x;
    
    let stroke_width = 2.5 * scale_x;
    painter.circle_stroke(handle1_center, r, egui::Stroke::new(stroke_width, color));
    painter.circle_stroke(handle2_center, r, egui::Stroke::new(stroke_width, color));
    
    // Draw the two blades (lines from (9.8, 8.2) to (21, 19.4) and (9.8, 15.8) to (21, 4.6))
    let blade1_start = scale_pos(9.8, 8.2);
    let blade1_end = scale_pos(21.0, 19.4);
    let blade2_start = scale_pos(9.8, 15.8);
    let blade2_end = scale_pos(21.0, 4.6);
    
    painter.line_segment([blade1_start, blade1_end], egui::Stroke::new(stroke_width, color));
    painter.line_segment([blade2_start, blade2_end], egui::Stroke::new(stroke_width, color));
}

fn toggle_switch(ui: &mut egui::Ui, appearance: &crate::settings::AppearanceSettings, cut_mode: &mut bool) -> egui::Response {
    let desired_size = egui::vec2(34.0, 18.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    
    if response.clicked() {
        *cut_mode = !*cut_mode;
        response.mark_changed();
    }
    
    let painter = ui.painter();
    let palette = get_palette(appearance);
    
    // Background color: red if cut_mode, green if keep_mode
    let bg_color = if *cut_mode {
        palette.cut_color
    } else {
        palette.keep_color
    };
    
    // Draw capsule background
    painter.rect_filled(rect, 9.0, bg_color);
    
    // Draw circular handle
    let circle_radius = 6.0;
    let x = if *cut_mode {
        rect.left() + circle_radius + 3.0 // Left position for CUT
    } else {
        rect.right() - circle_radius - 3.0 // Right position for KEEP
    };
    let center = egui::pos2(x, rect.center().y);
    painter.circle_filled(center, circle_radius, Color32::WHITE);
    
    response
}

fn playback_control_button(
    ui: &mut egui::Ui,
    appearance: &crate::settings::AppearanceSettings,
    icon: PlaybackIcon,
    is_play_btn: bool,
    tooltip: &str,
) -> egui::Response {
    let button_size = if is_play_btn { 34.0 } else { 26.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(button_size, button_size),
        egui::Sense::click(),
    );

    let response = response.on_hover_text(tooltip);

    let painter = ui.painter();
    let palette = get_palette(appearance);
    
    // Determine background color based on hover, click states
    let bg_color = if response.clicked() || response.is_pointer_button_down_on() {
        if is_play_btn {
            palette.accent
        } else if matches!(icon, PlaybackIcon::AddSnippet) {
            palette.keep_color
        } else {
            palette.widget_active_bg
        }
    } else if response.hovered() {
        if is_play_btn {
            palette.accent_hover
        } else if matches!(icon, PlaybackIcon::AddSnippet) {
            palette.keep_color
        } else {
            palette.widget_hovered_bg
        }
    } else {
        if is_play_btn {
            palette.accent
        } else if matches!(icon, PlaybackIcon::AddSnippet) {
            palette.keep_color
        } else {
            palette.widget_inactive_bg
        }
    };

    let border_color = if is_play_btn {
        palette.accent_hover
    } else if matches!(icon, PlaybackIcon::AddSnippet) {
        palette.keep_color
    } else {
        palette.border_color
    };

    // Draw circular button background
    painter.circle(
        rect.center(),
        button_size * 0.5,
        bg_color,
        egui::Stroke::new(1.0, border_color),
    );

    // Draw custom vector icon inside the button
    let icon_color = if is_play_btn || matches!(icon, PlaybackIcon::AddSnippet) {
        egui::Color32::WHITE
    } else if response.hovered() {
        palette.text_normal
    } else {
        palette.text_muted
    };

    draw_playback_icon(painter, rect, &icon, icon_color);

    response
}

impl MainApp {
    fn progress_bar(
        &mut self,
        ui: &mut egui::Ui,
    ) -> egui::Response {
        let palette = get_palette(&self.settings.appearance);
        let height = 12.0; // Interaction height
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click_and_drag(),
        );

        let response = response.on_hover_text("Drag or click to seek timeline");

        let painter = ui.painter();
        
        let track_height = 5.0; // Constant track height
        let track_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), track_height));

        // Draw basic track background and return if no video loaded
        if self.playback.total_frames() == 0 {
            painter.rect_filled(
                track_rect,
                track_height * 0.5,
                palette.timeline_track_bg,
            );
            return response;
        }

        // 1. Draw track background color dynamically based on cut/keep mode
        let track_color = if self.cut_mode {
            palette.keep_color.linear_multiply(0.25)
        } else {
            palette.cut_color.linear_multiply(0.25)
        };
        painter.rect_filled(
            track_rect,
            track_height * 0.5,
            track_color,
        );

        // 2. Draw keep/cut regions (saved export ranges) on the track
        let snippet_color = if self.cut_mode {
            palette.cut_color.linear_multiply(0.5)
        } else {
            palette.keep_color
        };
        for range in &self.ranges {
            let left_pct = range.in_frame as f32 / self.playback.total_frames() as f32;
            let right_pct = range.out_frame as f32 / self.playback.total_frames() as f32;
            let left_x = rect.left() + left_pct * rect.width();
            let right_x = rect.left() + right_pct * rect.width();
            
            let range_rect = egui::Rect::from_min_max(
                egui::pos2(left_x, track_rect.min.y),
                egui::pos2(right_x, track_rect.max.y),
            );
            painter.rect_filled(
                range_rect,
                track_height * 0.5,
                snippet_color,
            );
        }

        // 3. Draw active selection range (current in/out markers) on the track
        if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker)
            && inf < outf {
                let left_pct = inf as f32 / self.playback.total_frames() as f32;
                let right_pct = outf as f32 / self.playback.total_frames() as f32;
                let left_x = rect.left() + left_pct * rect.width();
                let right_x = rect.left() + right_pct * rect.width();
                
                let selection_rect = egui::Rect::from_min_max(
                    egui::pos2(left_x, track_rect.min.y),
                    egui::pos2(right_x, track_rect.max.y),
                );
                painter.rect_filled(
                    selection_rect,
                    track_height * 0.5,
                    palette.accent, // Active selection color
                );
            }

        // 4. Draw In / Out marker flags (each independent — shown even if only one is set)
        let marker_line_h = rect.height() + 6.0; // extends slightly above/below the hit rect
        let flag_w = 6.0;   // width of the L-shaped bracket cap
        let flag_h = 5.0;   // height of the bracket cap
        let label_font = egui::FontId::monospace(9.0);

        // Helper: draw one marker flag + floating timecode label.
        // `side` = -1.0 for In (cap extends right), +1.0 for Out (cap extends left).
        let draw_marker = |painter: &egui::Painter,
                           frame: usize,
                           color: Color32,
                           label: &str,
                           side: f32| {
            let pct = (frame as f32 / self.playback.total_frames() as f32).clamp(0.0, 1.0);
            let mx = rect.left() + pct * rect.width();
            let top_y = rect.center().y - marker_line_h * 0.5;
            let bot_y = rect.center().y + marker_line_h * 0.5;

            // Vertical line
            painter.line_segment(
                [egui::pos2(mx, top_y), egui::pos2(mx, bot_y)],
                egui::Stroke::new(1.5, color),
            );

            // Bracket cap (top horizontal arm)
            painter.line_segment(
                [egui::pos2(mx, top_y), egui::pos2(mx + side * flag_w, top_y)],
                egui::Stroke::new(1.5, color),
            );
            // Bracket cap (short vertical arm on the end)
            painter.line_segment(
                [
                    egui::pos2(mx + side * flag_w, top_y),
                    egui::pos2(mx + side * flag_w, top_y + flag_h),
                ],
                egui::Stroke::new(1.5, color),
            );

            // Floating timecode label above the marker line
            // Align left for In, right for Out so they don't overlap the line
            let label_align = if side > 0.0 {
                egui::Align2::RIGHT_BOTTOM
            } else {
                egui::Align2::LEFT_BOTTOM
            };
            painter.text(
                egui::pos2(mx + side * 2.0, top_y - 2.0),
                label_align,
                label,
                label_font.clone(),
                color,
            );
        };

        // In marker — amber yellow, bracket opens rightward
        if let Some(inf) = self.in_marker {
            let label = format!("[ {}", self.frame_to_time(inf));
            draw_marker(&painter, inf, palette.warn_color, &label, 1.0);
        }

        // Out marker — orange, bracket opens leftward
        if let Some(outf) = self.out_marker {
            let label = format!("{} ]", self.frame_to_time(outf));
            draw_marker(&painter, outf, palette.accent, &label, -1.0);
        }

        // 5. Always draw circular playhead handle (always visible)
        let fraction = (self.playback.current_frame() as f32 / self.playback.total_frames() as f32).clamp(0.0, 1.0);
        let progress_width = rect.width() * fraction;
        let handle_x = rect.left() + progress_width;
        let handle_center = egui::pos2(handle_x, rect.center().y);
        painter.circle_filled(
            handle_center,
            6.0,
            palette.accent, // Playhead handle
        );

        if response.clicked() {
            if let Some(mouse_pos) = response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos())) {
                let rel_x = (mouse_pos.x - rect.left()).clamp(0.0, rect.width());
                let pct = rel_x / rect.width();
                let target_frame = (pct * self.playback.total_frames() as f32) as usize;
                self.playback.set_playing(false);
                self.playback.seek_internal(target_frame, false, true);
            }
        } else {
            if response.drag_started() {
                self.playback.handle_input(media::InputEvent::ScrubDragStart);
            }
            if response.dragged()
                && let Some(mouse_pos) = response.interact_pointer_pos() {
                    let rel_x = (mouse_pos.x - rect.left()).clamp(0.0, rect.width());
                    let pct = rel_x / rect.width();
                    let target_frame = (pct * self.playback.total_frames() as f32) as usize;
                    self.playback.handle_input(media::InputEvent::ScrubDragUpdate { target_frame });
                }
            if response.drag_stopped() {
                self.playback.handle_input(media::InputEvent::ScrubDragEnd);
            }
        }

        response
    }

    fn draw_export_setup_modal(&mut self, ctx: &egui::Context) {
        let palette = theme::get_palette(&self.settings.appearance);
        
        egui::Window::new("Export Settings")
            .id(egui::Id::new("export_setup_modal"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([500.0, 240.0])
            .frame(egui::Frame::window(&ctx.global_style())
                .fill(palette.panel_bg)
                .stroke(egui::Stroke::new(1.0, palette.border_color))
                .inner_margin(egui::Margin::same(16)))
            .show(ctx, |ui| {
                ui.set_width(468.0);
                
                // Title
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("📤  Export Video").size(18.0).color(palette.text_normal).strong());
                });
                ui.add_space(8.0);
                ui.add(egui::Separator::default().spacing(1.0));
                ui.add_space(12.0);
                
                // Format selection
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Format (Container):").strong().color(palette.text_normal));
                    ui.add_space(8.0);
                    
                    for fmt in &["mp4", "mkv", "ts", "mov"] {
                        if ui.radio_value(&mut self.export_format, fmt.to_string(), fmt.to_uppercase()).clicked() {
                            // If format changed, update the extension of the output path
                            if !self.export_output_path.is_empty() {
                                let path = std::path::Path::new(&self.export_output_path);
                                if let Some(parent) = path.parent() {
                                    if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                                        let new_filename = format!("{}.{}", file_stem, self.export_format);
                                        self.export_output_path = parent.join(new_filename).to_string_lossy().to_string();
                                    }
                                }
                            }
                        }
                    }
                });
                ui.add_space(12.0);
                
                // Output path selection
                ui.label(egui::RichText::new("Destination File:").strong().color(palette.text_normal));
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.export_output_path)
                            .desired_width(ui.available_width() - 80.0)
                            .hint_text("Choose destination file...")
                    );
                    
                    if ui.button("Browse...").clicked() {
                        let mut dlg = rfd::FileDialog::new()
                            .add_filter(&format!("{} Video", self.export_format.to_uppercase()), &[&self.export_format]);
                        
                        if !self.export_output_path.is_empty() {
                            let path = std::path::Path::new(&self.export_output_path);
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
                            self.export_output_path = path.to_string_lossy().to_string();
                            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                                let ext_lower = ext.to_lowercase();
                                if ["mp4", "mkv", "ts", "mov"].contains(&ext_lower.as_str()) {
                                    self.export_format = ext_lower;
                                }
                            }
                        }
                    }
                });
                
                ui.add_space(16.0);
                ui.add(egui::Separator::default().spacing(1.0));
                ui.add_space(12.0);
                
                // Action buttons
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let has_path = !self.export_output_path.is_empty();
                        let start_export = ui.add_enabled(has_path, egui::Button::new(
                            egui::RichText::new("Start Export").strong().color(palette.panel_bg)
                        ).fill(palette.accent));
                        
                        if start_export.clicked() {
                            self.show_export_setup_modal = false;
                            
                            // Enforce correct extension
                            let mut final_output_path = self.export_output_path.clone();
                            let path = std::path::Path::new(&final_output_path);
                            let has_correct_ext = path.extension()
                                .and_then(|e| e.to_str())
                                .map(|ext| ext.to_lowercase() == self.export_format)
                                .unwrap_or(false);
                                
                            if !has_correct_ext {
                                let has_any_video_ext = path.extension()
                                    .and_then(|e| e.to_str())
                                    .map(|ext| ["mp4", "mkv", "ts", "mov"].contains(&ext.to_lowercase().as_str()))
                                    .unwrap_or(false);
                                    
                                if has_any_video_ext {
                                    if let Some(parent) = path.parent() {
                                        if let Some(file_stem) = path.file_stem().and_then(|s| s.to_str()) {
                                            final_output_path = parent.join(format!("{}.{}", file_stem, self.export_format)).to_string_lossy().to_string();
                                        }
                                    }
                                } else {
                                    final_output_path = format!("{}.{}", final_output_path, self.export_format);
                                }
                            }
                            
                            let input_path = self.video_path.clone().unwrap();
                            let ranges = self.ranges.clone();
                            let cut_mode = self.cut_mode;
                            
                            if let Some(metadata) = self.playback.metadata() {
                                let status_clone = self.export_status.clone();
                                let cancel_clone = self.export_cancel_flag.clone();
                                
                                {
                                    let mut status_guard = status_clone.lock().unwrap();
                                    *status_guard = crate::media::types::ExportStatus::Exporting {
                                        progress: 0.0,
                                        message: "Starting export...".to_string(),
                                        is_encoding: false,
                                        current_frame: 0,
                                    };
                                }
                                cancel_clone.store(false, std::sync::atomic::Ordering::Relaxed);
                                
                                std::thread::spawn(move || {
                                    if let Err(e) = crate::media::export::export_video(
                                        &input_path,
                                        &final_output_path,
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
                        
                        if ui.button("Cancel").clicked() {
                            self.show_export_setup_modal = false;
                        }
                    });
                });
            });
    }
}

impl eframe::App for MainApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let palette = get_palette(&self.settings.appearance);
        
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
        let ctx = ui.ctx().clone();
        let wants_kb = ctx.egui_wants_keyboard_input();
        let mut prev_pressed = !wants_kb && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft));
        let mut next_pressed = !wants_kb && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight));
        let mut prev_down = !wants_kb && ctx.input(|i| i.key_down(egui::Key::ArrowLeft));
        let mut next_down = !wants_kb && ctx.input(|i| i.key_down(egui::Key::ArrowRight));
        let prev_released = !wants_kb && ctx.input(|i| i.key_released(egui::Key::ArrowLeft));
        let next_released = !wants_kb && ctx.input(|i| i.key_released(egui::Key::ArrowRight));

        if !wants_kb && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.playback.toggle_playback();
        }

        let ctrl_d = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::D);
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_d)) {
            self.show_debug_hud = !self.show_debug_hud;
        }

        // ⌘, (macOS) / Ctrl+, (other) opens Settings
        let settings_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma);
        if ctx.input_mut(|i| i.consume_shortcut(&settings_shortcut)) {
            if !self.show_settings_modal {
                self.pending_settings = self.settings.clone();
                self.show_settings_modal = true;
            }
        }

        // --- Keyboard shortcuts for Label Mode ---
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
                let current_frame = self.playback.current_frame() as u64;
                if ctx.input(|i| i.key_pressed(egui::Key::B)) {
                    session.add_break_at(current_frame);
                    ctx.request_repaint();
                }
                if ctx.input(|i| i.key_pressed(egui::Key::Delete)) || ctx.input(|i| i.key_pressed(egui::Key::Backspace)) {
                    if let Some(idx) = session.selected_break {
                        session.remove_break(idx);
                        ctx.request_repaint();
                    }
                }
                if ctx.input(|i| i.key_pressed(egui::Key::N)) {
                    if let Some(idx) = session.next_break_index(current_frame) {
                        session.selected_break = Some(idx);
                        let target = session.breaks[idx].start_cut_frame as usize;
                        self.playback.seek_internal(target, false, false);
                        ctx.request_repaint();
                    }
                }
                if ctx.input(|i| i.key_pressed(egui::Key::P)) {
                    if let Some(idx) = session.prev_break_index(current_frame) {
                        session.selected_break = Some(idx);
                        let target = session.breaks[idx].start_cut_frame as usize;
                        self.playback.seek_internal(target, false, false);
                        ctx.request_repaint();
                    }
                }
                if ctx.input(|i| i.key_pressed(egui::Key::T)) {
                    if let Some(idx) = session.selected_break {
                        session.toggle_break_type(idx);
                        ctx.request_repaint();
                    }
                }
                if ctx.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                    if let Some(idx) = session.selected_break {
                        let new_start = session.breaks[idx].start_cut_frame.saturating_sub(1);
                        session.set_start_frame(idx, new_start);
                        ctx.request_repaint();
                    }
                }
                if ctx.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                    if let Some(idx) = session.selected_break {
                        let new_start = session.breaks[idx].start_cut_frame.saturating_add(1);
                        session.set_start_frame(idx, new_start);
                        ctx.request_repaint();
                    }
                }
            }
        }


        // Receive frames from the Media Engine
        let (latest_frame, frames_popped, loudness_updated) = self.playback.update();
        if loudness_updated {
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

        if latest_frame.is_some() {
            ctx.request_repaint();
        }

        if self.playback.is_playing() || self.playback.is_stepping() || self.playback.is_scrubbing() {
            ctx.request_repaint();
        }

        // Polling decoded thumbnails
        if let Some(rx) = self.playback.thumbnail_rx.as_ref() {
            while let Ok(resp) = rx.try_recv() {
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [resp.width as usize, resp.height as usize],
                    &resp.rgba_data,
                );
                // Load once — TextureHandle is Arc-backed; clones share one GPU allocation.
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

        // Detect zoom change — LRU handles eviction naturally; no cache wipe needed.
        let has_zoomed = (self.zoom_level - self.last_zoom_level).abs() > 0.01;
        if has_zoomed {
            self.last_zoom_level = self.zoom_level;
            self.in_flight_requests.clear();
            self.thumbnails_dirty = true;
        }

        // --- Notification banners ---
        // Collect data first so the rendering closure doesn't hold a live borrow on self.notifications.
        let notif_data: Vec<(Color32, Color32, Color32, &'static str, String)> = self
            .notifications
            .items
            .iter()
            .map(|n| {
                let (bg, border, text_color, icon) = if palette.is_dark {
                    match n.kind {
                        NotificationKind::Error   => (Color32::from_rgb(60, 15, 15), Color32::from_rgb(200, 50, 50), Color32::from_rgb(230, 230, 230), "✕ "),
                        NotificationKind::Warning => (Color32::from_rgb(50, 35, 5),  Color32::from_rgb(200, 150, 50), Color32::from_rgb(230, 230, 230), "⚠ "),
                        NotificationKind::Info    => (Color32::from_rgb(10, 25, 50), Color32::from_rgb(80, 140, 220), Color32::from_rgb(230, 230, 230), "ℹ "),
                    }
                } else {
                    match n.kind {
                        NotificationKind::Error   => (Color32::from_rgb(254, 226, 226), Color32::from_rgb(239, 68, 68), Color32::from_rgb(153, 27, 27), "✕ "),
                        NotificationKind::Warning => (Color32::from_rgb(254, 243, 199), Color32::from_rgb(245, 158, 11), Color32::from_rgb(146, 64, 14), "⚠ "),
                        NotificationKind::Info    => (Color32::from_rgb(219, 234, 254), Color32::from_rgb(59, 130, 246), Color32::from_rgb(30, 64, 175), "ℹ "),
                    }
                };
                (bg, border, text_color, icon, n.message.clone())
            })
            .collect();
        let mut dismiss_idx: Option<usize> = None;
        for (idx, (bg, border, text_color, icon, message)) in notif_data.iter().enumerate() {
            egui::Panel::top(format!("notif_{idx}"))
                .frame(egui::Frame::NONE.fill(*bg).stroke(egui::Stroke::new(1.0, *border)).inner_margin(egui::Margin::symmetric(14, 8)))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(format!("{}{}", icon, message)).color(*text_color).size(13.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").on_hover_text("Dismiss").clicked() {
                                dismiss_idx = Some(idx);
                            }
                        });
                    });
                });
        }
        if let Some(idx) = dismiss_idx {
            self.notifications.dismiss(idx);
        }

        // --- Settings modal ---
        if self.show_settings_modal {
            self.draw_settings_modal(&ctx);
        }

        // --- Export Setup modal ---
        if self.show_export_setup_modal {
            self.draw_export_setup_modal(&ctx);
        }

        // 2. Bottom Panel: Timeline and Waveform Deck (Full width)
        let panel_response = egui::Panel::bottom("timeline_deck")
            .resizable(true)
            .default_size(self.timeline_height)
            .size_range(160.0..=350.0)
            .frame(egui::Frame::NONE
                .fill(palette.panel_bg)
                .inner_margin(egui::Margin::symmetric(16, 12)))
            .show_inside(ui, |ui| {
                ui.set_min_height(ui.max_rect().height());
                if self.video_path.is_some() {
                    ui.vertical(|ui| {
                    // Timeline Time Indicators & Zoom Controls
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(self.frame_to_time(self.playback.current_frame()))
                                .size(16.0)
                                .color(palette.text_normal)
                        );
                        ui.label(format!("/ {}", self.frame_to_time(self.playback.total_frames())));
                        
                        // Zoom controls aligned right
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Zoom In button (+)
                            if ui.small_button("+").on_hover_text("Zoom in filmstrip (show fewer frames)").clicked() {
                                self.zoom_level = (self.zoom_level + 0.1).clamp(0.0, 1.0);
                            }
                            
                            // Zoom slider
                            let old_active_bg = ui.style().visuals.widgets.active.bg_fill;
                            let old_hovered_bg = ui.style().visuals.widgets.hovered.bg_fill;
                            ui.style_mut().visuals.widgets.active.bg_fill = palette.secondary;
                            ui.style_mut().visuals.widgets.hovered.bg_fill = palette.secondary_hover;
                            ui.add(egui::Slider::new(&mut self.zoom_level, 0.0..=1.0)
                                .show_value(false)
                                .text("Zoom"));
                            ui.style_mut().visuals.widgets.active.bg_fill = old_active_bg;
                            ui.style_mut().visuals.widgets.hovered.bg_fill = old_hovered_bg;

                            // Zoom Out button (-)
                            if ui.small_button("−").on_hover_text("Zoom out filmstrip (show more overview)").clicked() {
                                self.zoom_level = (self.zoom_level - 0.1).clamp(0.0, 1.0);
                            }

                            ui.add_space(8.0);
                            ui.label(format!("Frame {} / {}", self.playback.current_frame(), self.playback.total_frames()));
                        });
                    });

                    ui.add_space(4.0);

                    // 2. Sprocket Filmstrip Viewport
                    let total_width = ui.available_width();
                    let filmstrip_height = (self.timeline_height - 128.0).max(30.0);

                    let (filmstrip_rect, filmstrip_response) = ui.allocate_exact_size(
                        egui::vec2(total_width, filmstrip_height),
                        egui::Sense::click_and_drag(),
                    );
                    
                    let painter = ui.painter_at(filmstrip_rect);
                    // Draw filmstrip track card background
                    painter.rect_filled(filmstrip_rect, 6.0, palette.timeline_track_bg);
                    painter.rect_stroke(filmstrip_rect, 6.0, egui::Stroke::new(1.0, palette.border_color), egui::StrokeKind::Inside);

                    let sprocket_height = 8.0;
                    let inner_height = (filmstrip_height - (sprocket_height * 2.0)).max(10.0);

                    // Bucket the thumbnail height to increments of 64 pixels to prevent constant cache clearing on resize
                    let bucketed_height = (((inner_height / 64.0).round() as u32) * 64).clamp(64, 256) as f32;

                    if (bucketed_height - self.last_thumbnail_height).abs() > 0.01 {
                        self.last_thumbnail_height = bucketed_height;
                        self.in_flight_requests.clear();
                        self.thumbnails_dirty = true;
                    }

                    let inner_rect = egui::Rect::from_min_max(
                        egui::pos2(filmstrip_rect.left(), filmstrip_rect.top() + sprocket_height),
                        egui::pos2(filmstrip_rect.right(), filmstrip_rect.bottom() - sprocket_height),
                    );

                    // Draw celluloid borders (top and bottom margins)
                    let celluloid_bg = Color32::from_rgb(18, 18, 22);
                    let top_margin = egui::Rect::from_min_max(
                        egui::pos2(filmstrip_rect.left(), filmstrip_rect.top()),
                        egui::pos2(filmstrip_rect.right(), filmstrip_rect.top() + sprocket_height),
                    );
                    let bottom_margin = egui::Rect::from_min_max(
                        egui::pos2(filmstrip_rect.left(), filmstrip_rect.bottom() - sprocket_height),
                        egui::pos2(filmstrip_rect.right(), filmstrip_rect.bottom()),
                    );
                    painter.rect_filled(top_margin, 0.0, celluloid_bg);
                    painter.rect_filled(bottom_margin, 0.0, celluloid_bg);

                    // Draw sprockets on top and bottom margins (stretching full width)
                    let sprocket_width = 8.0;
                    let sprocket_h = 5.0;
                    let num_sprockets = ((total_width / 16.0) as usize).max(5);
                    let sprocket_gap = (total_width - (sprocket_width * num_sprockets as f32)) / (num_sprockets - 1) as f32;
                    let sprocket_hole_fill = palette.panel_bg; 

                    for s in 0..num_sprockets {
                        let sx = filmstrip_rect.left() + s as f32 * (sprocket_width + sprocket_gap);
                        
                        // Top sprocket hole
                        let top_sprocket_rect = egui::Rect::from_min_size(
                            egui::pos2(sx, filmstrip_rect.top() + (sprocket_height - sprocket_h) * 0.5),
                            egui::vec2(sprocket_width, sprocket_h),
                        );
                        painter.rect_filled(top_sprocket_rect, 1.0, sprocket_hole_fill);

                        // Bottom sprocket hole
                        let bottom_sprocket_rect = egui::Rect::from_min_size(
                            egui::pos2(sx, filmstrip_rect.bottom() - (sprocket_height + sprocket_h) * 0.5),
                            egui::vec2(sprocket_width, sprocket_h),
                        );
                        painter.rect_filled(bottom_sprocket_rect, 1.0, sprocket_hole_fill);
                    }

                    // Calculate logarithmic delta step
                    let fps = self.playback.video_fps();
                    let total_frames = self.playback.total_frames();
                    let duration_sec = total_frames as f64 / fps;
                    
                    // Interpolate time delta represented by a single thumbnail step.
                    // Bounds and curve shape are driven by Settings (see [timeline] group).
                    let interval = self.settings.zoom_level_to_interval(self.zoom_level, fps);
                    
                    // Step spacing logic
                    let spacing_x = 6.0;
                    let thumb_height = inner_height; // Dynamically scale to fit filmstrip height
                    let aspect_ratio = 16.0 / 9.0;
                    let actual_thumb_width = thumb_height * aspect_ratio;
                    let step_width = actual_thumb_width + spacing_x;
                    let pixels_per_second = step_width as f64 / interval;

                    // Stationary Playhead Center position
                    let playhead_x = total_width * 0.5;
                    let current_time_sec = self.playback.current_frame() as f64 / fps;
                    let scroll_offset_x = playhead_x as f64 - (current_time_sec * pixels_per_second);

                    // Compute indices of visible step slots
                    let left_bound = -scroll_offset_x - actual_thumb_width as f64;
                    let right_bound = -scroll_offset_x + total_width as f64;
                    let first_k = (left_bound / step_width as f64).floor() as isize;
                    let last_k = (right_bound / step_width as f64).ceil() as isize;

                    let now = std::time::Instant::now();

                    // --- Render visible slots, collect uncached frame indices ---
                    let mut visible_frames: Vec<usize> = Vec::new();
                    for k in first_k..=last_k {
                        if k < 0 { continue; }
                        let target_time = k as f64 * interval;
                        if target_time > duration_sec { break; }
                        let target_frame = (target_time * fps).round() as usize;
                        if target_frame >= total_frames { continue; }

                        let tx_px = scroll_offset_x + (k as f64 * step_width as f64);
                        let thub_rect = egui::Rect::from_min_size(
                            egui::pos2(tx_px as f32, inner_rect.top()),
                            egui::vec2(actual_thumb_width, thumb_height),
                        );

                        let is_cut = self.is_frame_cut(target_frame);
                        let tint = if is_cut {
                            Color32::from_rgba_unmultiplied(255, 255, 255, 100) // Dimmed with opacity multiplier
                        } else {
                            Color32::WHITE // Full original colors
                        };

                        if let Some(texture) = self.thumbnail_cache.get(&target_frame) {
                            // Draw decoded thumbnail image
                            painter.image(
                                texture.id(),
                                thub_rect,
                                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                tint,
                            );

                            if is_cut {
                                // Draw red transparent tint
                                painter.rect_filled(
                                    thub_rect,
                                    0.0,
                                    Color32::from_rgba_unmultiplied(palette.cut_color.r(), palette.cut_color.g(), palette.cut_color.b(), 20),
                                );
                                // Draw diagonal hatching lines
                                draw_hatching(
                                    &painter,
                                    thub_rect,
                                    Color32::from_rgba_unmultiplied(palette.cut_color.r(), palette.cut_color.g(), palette.cut_color.b(), 45),
                                    1.0,
                                    12.0,
                                );
                            }
                        } else {
                            // Placeholder loading card with animated spinner
                            let placeholder_bg = palette.timeline_track_bg;
                            painter.rect_filled(thub_rect, 4.0, placeholder_bg);
                            painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(1.0, palette.border_color), egui::StrokeKind::Inside);
                            if inner_height > 16.0 {
                                // Spinning arc: one full revolution every 1.2 s
                                let t = ui.ctx().input(|i| i.time) as f32;
                                let angle = t * (2.0 * std::f32::consts::PI / 1.2);
                                let center = thub_rect.center();
                                let r = (thub_rect.size().min_elem() * 0.22).max(4.0);
                                let arc_fraction = 0.65_f32;
                                let n_segments = 20usize;
                                let arc_color = palette.accent;
                                let points: Vec<egui::Pos2> = (0..=n_segments)
                                    .map(|k| {
                                        let a = angle
                                            + k as f32 / n_segments as f32
                                                * arc_fraction
                                                * std::f32::consts::TAU;
                                        egui::pos2(
                                            center.x + r * a.cos(),
                                            center.y + r * a.sin(),
                                        )
                                    })
                                    .collect();
                                for seg in points.windows(2) {
                                    painter.line_segment(
                                        [seg[0], seg[1]],
                                        egui::Stroke::new(1.5, arc_color),
                                    );
                                }
                                ui.ctx().request_repaint();
                            }
                            if is_cut {
                                draw_hatching(
                                    &painter,
                                    thub_rect,
                                    palette.cut_color.linear_multiply(0.2),
                                    1.0,
                                    12.0,
                                );
                            }
                            if !self.in_flight_requests.contains_key(&target_frame) {
                                visible_frames.push(target_frame);
                                self.in_flight_requests.insert(target_frame, now);
                            }
                        }

                        // Draw active selection border (blue outline)
                        if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker) {
                            if target_frame >= inf && target_frame <= outf {
                                painter.rect_stroke(
                                    thub_rect,
                                    4.0,
                                    egui::Stroke::new(2.0, palette.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }

                        // Underlay timecode — drawn with the unclipped panel painter so it
                        // appears in the ruler strip below the filmstrip rect.
                        if inner_height > 12.0 {
                            let time_text = self.frame_to_time(target_frame);
                            ui.painter().text(
                                egui::pos2(thub_rect.center().x, filmstrip_rect.bottom() + 10.0),
                                egui::Align2::CENTER_CENTER,
                                time_text,
                                egui::FontId::monospace(8.0),
                                palette.text_muted,
                            );
                        }
                    }

                    // --- Collect padding frame slots (±2 beyond visible) ---
                    let padding = 2isize;
                    let mut padding_frames: Vec<usize> = Vec::new();
                    for k in ((first_k - padding).max(0)..first_k)
                        .chain((last_k + 1)..=(last_k + padding))
                    {
                        if k < 0 { continue; }
                        let target_time = k as f64 * interval;
                        if target_time > duration_sec { continue; }
                        let target_frame = (target_time * fps).round() as usize;
                        if target_frame >= total_frames { continue; }
                        if self.thumbnail_cache.peek(&target_frame).is_some() { continue; }
                        if self.in_flight_requests.contains_key(&target_frame) { continue; }
                        padding_frames.push(target_frame);
                        self.in_flight_requests.insert(target_frame, now);
                    }

                    // --- Expire timed-out in-flight requests (allow retry after 2s) ---
                    self.in_flight_requests.retain(|_, t| now.duration_since(*t) < std::time::Duration::from_secs(2));

                    // --- Dirty flag: set if there is uncached work to dispatch ---
                    if !visible_frames.is_empty() || !padding_frames.is_empty() {
                        self.thumbnails_dirty = true;
                    }

                    // --- Send ONE request batch per dirty repaint (debounce) ---
                    if self.thumbnails_dirty {
                        if let Some(tx) = self.playback.thumbnail_tx.as_ref() {
                            let _ = tx.send(crate::media::ThumbnailCommand::Request {
                                visible_frames,
                                padding_frames,
                                height: bucketed_height as u32,
                            });
                        }
                        self.thumbnails_dirty = false;
                    }

                    // Step 5: Drag-to-Scrub Mouse Interaction
                    
                    if filmstrip_response.dragged() || filmstrip_response.clicked() {
                        self.playback.set_playing(false);
                        if let Some(pointer_pos) = filmstrip_response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos())) {
                            let dx_pixels = pointer_pos.x - playhead_x as f32;
                            let dt_seconds = dx_pixels as f64 / pixels_per_second;
                            let target_time = current_time_sec + dt_seconds;
                            let target_frame = (target_time * fps).round().max(0.0).min(total_frames as f64 - 1.0) as usize;
                            self.playback.seek_internal(target_frame, false, true);
                        }
                    }

                    // Playhead Triangle pointing down at the top of the filmstrip
                    let center_x = playhead_x;
                    let p1 = egui::pos2(center_x - 6.0, filmstrip_rect.top() - 2.0);
                    let p2 = egui::pos2(center_x + 6.0, filmstrip_rect.top() - 2.0);
                    let p3 = egui::pos2(center_x, filmstrip_rect.top() + 6.0);
                    painter.add(egui::Shape::convex_polygon(
                        vec![p1, p2, p3],
                        palette.accent,
                        egui::Stroke::NONE,
                    ));

                    ui.add_space(6.0);

                    // 3. Compact Synced Waveform Component
                    let wave_height = 32.0;
                    let (wave_rect, _wave_response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), wave_height),
                        egui::Sense::hover(),
                    );

                    let painter = ui.painter_at(wave_rect);
                    painter.rect(
                        wave_rect,
                        4.0,
                        palette.card_bg,
                        egui::Stroke::new(1.0, palette.border_color),
                        egui::StrokeKind::Inside,
                    );

                    // Synced frame calculations matching filmstrip
                    let total_width_d = total_width as f64;
                    let visible_duration = total_width_d / pixels_per_second;
                    let visible_range_width_d = visible_duration * fps;
                    let start_frame_d = (current_time_sec - (total_width_d * 0.5) / pixels_per_second) * fps;

                    // Render real loudness profile bars — one bar per pixel column
                    let loudness = self.playback.loudness_profile();
                    let total_buckets = loudness.len();
                    let bar_count = wave_rect.width() as usize;
                    let bar_width = 1.0_f32;

                    let total_frames = self.playback.total_frames();
                    let db_floor = self.settings.playback.waveform_db_floor;

                    for i in 0..bar_count {
                        let pct_start = i as f64 / bar_count as f64;
                        let pct_end = (i + 1) as f64 / bar_count as f64;

                        let frame_start_d = start_frame_d + pct_start * visible_range_width_d;
                        let frame_end_d = start_frame_d + pct_end * visible_range_width_d;

                        // Use center of column for frame cut check
                        let frame_center_idx = (frame_start_d + frame_end_d) * 0.5;
                        let frame_idx_isize = frame_center_idx as isize;

                        let out_of_bounds = frame_idx_isize < 0 || frame_idx_isize >= total_frames as isize;

                        let mut amplitude_opt: Option<f32> = None;

                        if !out_of_bounds {
                            let b_start_d = frame_start_d * 4.0;
                            let b_end_d = frame_end_d * 4.0;

                            let b_start = b_start_d.floor() as isize;
                            let b_end = b_end_d.ceil() as isize;

                            let b_start_clamped = b_start.max(0) as usize;
                            let b_end_clamped = b_end.max(0) as usize;

                            if b_start < total_buckets as isize && b_end >= 0 {
                                let limit_start = b_start_clamped.min(total_buckets);
                                let limit_end = b_end_clamped.max(limit_start + 1).min(total_buckets);

                                let mut max_amp: Option<f32> = None;
                                for bi in limit_start..limit_end {
                                    if let Some(Some(amp)) = loudness.get(bi) {
                                        max_amp = Some(max_amp.map_or(*amp, |m| m.max(*amp)));
                                    }
                                }
                                amplitude_opt = max_amp;
                            }
                        }

                        if let Some(amplitude) = amplitude_opt {
                            let scaled_amplitude = if db_floor >= 0.0 {
                                amplitude
                            } else if amplitude <= 0.0 {
                                0.0
                            } else {
                                let db = 20.0 * amplitude.log10();
                                let pct = (db - db_floor) / -db_floor;
                                pct.clamp(0.0, 1.0)
                            };

                            let bar_h = scaled_amplitude * (wave_rect.height() - 6.0);
                            let x = wave_rect.left() + i as f32 * bar_width;

                            let is_cut = if out_of_bounds {
                                false
                            } else {
                                self.is_frame_cut(frame_idx_isize as usize)
                            };

                            let bar_color = if out_of_bounds {
                                palette.timeline_track_bg
                            } else if is_cut {
                                palette.accent.linear_multiply(0.35)
                            } else {
                                palette.accent
                            };

                            painter.line_segment(
                                [
                                    egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0),
                                    egui::pos2(x + bar_width * 0.5, wave_rect.bottom() - 3.0 - bar_h),
                                ],
                                egui::Stroke::new(bar_width, bar_color),
                            );
                        }
                    }

                    // Draw scrolling In/Out markers on the filmstrip & waveform
                    let draw_timeline_marker = |painter: &egui::Painter, frame: usize, color: Color32, direction: f32| {
                        let frame_time = frame as f64 / fps;
                        let x = scroll_offset_x + frame_time * pixels_per_second;
                        let x_f32 = x as f32;
                        
                        // Draw only if horizontally visible inside the filmstrip rect
                        if x_f32 >= filmstrip_rect.left() && x_f32 <= filmstrip_rect.right() {
                            // 1. Draw continuous vertical line through both filmstrip and waveform
                            painter.line_segment(
                                [egui::pos2(x_f32, filmstrip_rect.top()), egui::pos2(x_f32, wave_rect.bottom())],
                                egui::Stroke::new(1.5, color),
                            );
                            
                            // 2. Top Bracket Caps at filmstrip_rect.top()
                            painter.line_segment(
                                [egui::pos2(x_f32, filmstrip_rect.top()), egui::pos2(x_f32 + direction * 8.0, filmstrip_rect.top())],
                                egui::Stroke::new(1.5, color),
                            );
                            painter.line_segment(
                                [egui::pos2(x_f32 + direction * 8.0, filmstrip_rect.top()), egui::pos2(x_f32 + direction * 8.0, filmstrip_rect.top() + 6.0)],
                                egui::Stroke::new(1.5, color),
                            );
                            
                            // 3. Bottom Bracket Caps at wave_rect.bottom()
                            painter.line_segment(
                                [egui::pos2(x_f32, wave_rect.bottom()), egui::pos2(x_f32 + direction * 8.0, wave_rect.bottom())],
                                egui::Stroke::new(1.5, color),
                            );
                            painter.line_segment(
                                [egui::pos2(x_f32 + direction * 8.0, wave_rect.bottom()), egui::pos2(x_f32 + direction * 8.0, wave_rect.bottom() - 6.0)],
                                egui::Stroke::new(1.5, color),
                            );
                        }
                    };

                    if let Some(session) = &mut self.label_session {
                        let mut new_start_frame = None;
                        let mut new_end_frame = None;
                        
                        let breaks_len = session.breaks.len();
                        for i in 0..breaks_len {
                            let brk = session.breaks[i].clone();
                            let start_frame = brk.start_cut_frame;
                            let end_frame = brk.end_cut_frame;

                            let start_time = start_frame as f64 / fps;
                            let end_time = end_frame as f64 / fps;

                            let start_x = scroll_offset_x + start_time * pixels_per_second;
                            let end_x = scroll_offset_x + end_time * pixels_per_second;

                            let band_left = (start_x as f32).max(filmstrip_rect.left());
                            let band_right = (end_x as f32).min(filmstrip_rect.right());

                            // Draw background colored band if visible
                            if band_left < band_right {
                                let band_color = match brk.break_type {
                                    snippy_schema::BreakType::Commercial => {
                                        if session.selected_break == Some(i) {
                                            Color32::from_rgba_unmultiplied(230, 100, 0, 70)
                                        } else {
                                            Color32::from_rgba_unmultiplied(230, 100, 0, 35)
                                        }
                                    }
                                    snippy_schema::BreakType::Promo => {
                                        if session.selected_break == Some(i) {
                                            Color32::from_rgba_unmultiplied(140, 50, 230, 70)
                                        } else {
                                            Color32::from_rgba_unmultiplied(140, 50, 230, 35)
                                        }
                                    }
                                };
                                let band_rect = egui::Rect::from_min_max(
                                    egui::pos2(band_left, filmstrip_rect.top()),
                                    egui::pos2(band_right, wave_rect.bottom())
                                );
                                ui.painter().rect_filled(band_rect, 0.0, band_color);

                                // If selected, draw a border outline
                                if session.selected_break == Some(i) {
                                    ui.painter().rect_stroke(
                                        band_rect,
                                        0.0,
                                        egui::Stroke::new(1.5, palette.accent),
                                        egui::StrokeKind::Inside
                                    );
                                }

                                // Interactive selection of the band itself
                                let band_id = ui.make_persistent_id(format!("band_{}", i));
                                let band_resp = ui.interact(band_rect, band_id, egui::Sense::click());
                                if band_resp.clicked() {
                                    session.selected_break = Some(i);
                                }
                            }

                            // Start Handle
                            let start_x_f32 = start_x as f32;
                            if start_x_f32 >= filmstrip_rect.left() && start_x_f32 <= filmstrip_rect.right() {
                                let handle_color = if session.selected_break == Some(i) {
                                    palette.accent
                                } else {
                                    palette.text_muted
                                };

                                // Vertical line
                                ui.painter().line_segment(
                                    [egui::pos2(start_x_f32, filmstrip_rect.top()), egui::pos2(start_x_f32, wave_rect.bottom())],
                                    egui::Stroke::new(1.5, handle_color),
                                );

                                // Handle knob at the top
                                let knob_pos = egui::pos2(start_x_f32, filmstrip_rect.top() + 6.0);
                                ui.painter().circle_filled(knob_pos, 5.0, handle_color);

                                let handle_rect = egui::Rect::from_center_size(knob_pos, egui::vec2(16.0, 16.0));
                                let handle_id = ui.make_persistent_id(format!("handle_{}_start", i));
                                let handle_resp = ui.interact(handle_rect, handle_id, egui::Sense::drag());

                                if handle_resp.dragged() {
                                    if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) {
                                        let relative_x_scroll = pointer_pos.x as f64 - scroll_offset_x;
                                        let new_time = relative_x_scroll / pixels_per_second;
                                        let new_frame = (new_time * fps).round().max(0.0) as u64;
                                        new_start_frame = Some((i, new_frame.min(total_frames as u64 - 1)));
                                    }
                                }
                            }

                            // End Handle
                            let end_x_f32 = end_x as f32;
                            if end_x_f32 >= filmstrip_rect.left() && end_x_f32 <= filmstrip_rect.right() {
                                let handle_color = if session.selected_break == Some(i) {
                                    palette.accent
                                } else {
                                    palette.text_muted
                                };

                                // Vertical line
                                ui.painter().line_segment(
                                    [egui::pos2(end_x_f32, filmstrip_rect.top()), egui::pos2(end_x_f32, wave_rect.bottom())],
                                    egui::Stroke::new(1.5, handle_color),
                                );

                                // Handle knob at the top
                                let knob_pos = egui::pos2(end_x_f32, filmstrip_rect.top() + 6.0);
                                ui.painter().circle_filled(knob_pos, 5.0, handle_color);

                                let handle_rect = egui::Rect::from_center_size(knob_pos, egui::vec2(16.0, 16.0));
                                let handle_id = ui.make_persistent_id(format!("handle_{}_end", i));
                                let handle_resp = ui.interact(handle_rect, handle_id, egui::Sense::drag());

                                if handle_resp.dragged() {
                                    if let Some(pointer_pos) = ui.input(|i| i.pointer.interact_pos()) {
                                        let relative_x_scroll = pointer_pos.x as f64 - scroll_offset_x;
                                        let new_time = relative_x_scroll / pixels_per_second;
                                        let new_frame = (new_time * fps).round().max(0.0) as u64;
                                        new_end_frame = Some((i, new_frame.min(total_frames as u64 - 1)));
                                    }
                                }
                            }
                        }

                        // Apply updates safely after loop to prevent borrow checker conflicts
                        if let Some((idx, frame)) = new_start_frame {
                            session.set_start_frame(idx, frame);
                            ctx.request_repaint();
                        }
                        if let Some((idx, frame)) = new_end_frame {
                            session.set_end_frame(idx, frame);
                            ctx.request_repaint();
                        }
                    } else {
                        if let Some(inf) = self.in_marker {
                            draw_timeline_marker(&ui.painter(), inf, palette.warn_color, 1.0);
                        }
                        if let Some(outf) = self.out_marker {
                            draw_timeline_marker(&ui.painter(), outf, palette.accent, -1.0);
                        }
                    }

                });
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("No timeline available — load a video to begin editing")
                                .size(14.0)
                                .color(palette.text_muted)
                        );
                    });
                }
            });

        let new_height = panel_response.response.rect.height();

        if ui.ctx().input(|i| i.pointer.any_pressed()) {
            if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                let dist_to_top = (pos.y - panel_response.response.rect.top()).abs();
                if dist_to_top < 15.0 {
                    self.is_resizing_timeline = true;
                }
            }
        }

        if self.is_resizing_timeline {
            if ui.ctx().input(|i| i.pointer.any_down()) {
                self.timeline_height = new_height;
            } else {
                self.is_resizing_timeline = false;
            }
        }

        egui::Panel::right("sidebar_deck")
            .resizable(false)
            .default_size(300.0)
            .frame(egui::Frame::NONE
                .fill(palette.card_bg)
                .stroke(egui::Stroke::new(1.0, palette.border_color))
                .inner_margin(egui::Margin::same(16)))
            .show_inside(ui, |ui| {
                ui.vertical(|ui| {
                    if let Some(session) = &mut self.label_session {
                        // Title: [breaks] 🏷
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                            ui.label(egui::RichText::new("[").strong().color(palette.accent).size(18.0));
                            ui.label(egui::RichText::new("breaks").strong().color(palette.text_normal).size(18.0));
                            ui.label(egui::RichText::new("]").strong().color(palette.accent).size(18.0));
                            ui.add_space(6.0);
                            ui.label(egui::RichText::new("🏷").size(16.0));
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Metadata Fields
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Channel:").color(palette.text_muted));
                            if ui.text_edit_singleline(&mut session.channel).changed() {
                                session.dirty = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Region:").color(palette.text_muted));
                            if ui.text_edit_singleline(&mut session.region).changed() {
                                session.dirty = true;
                            }
                        });

                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);

                        // Scrollable List of Breaks
                        let mut to_delete = None;
                        let mut type_toggle = None;
                        let mut notes_change = None;
                        let mut seek_to_frame = None;
                        
                        let fps_val = self.playback.video_fps();
                        let frame_to_time_local = |frame: usize| -> String {
                            let total_seconds = frame as f64 / fps_val;
                            let hours = (total_seconds / 3600.0).floor() as usize;
                            let minutes = ((total_seconds % 3600.0) / 60.0).floor() as usize;
                            let seconds = total_seconds.floor() as usize % 60;
                            let centiseconds = ((total_seconds - total_seconds.floor()) * 100.0).floor() as usize;
                            format!("{:02}:{:02}:{:02}.{:02}", hours, minutes, seconds, centiseconds)
                        };

                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let breaks_len = session.breaks.len();
                            for i in 0..breaks_len {
                                let brk = &session.breaks[i];
                                let is_selected = session.selected_break == Some(i);

                                // Draw break item in a frame/group
                                let frame_color = if is_selected {
                                    palette.widget_active_bg
                                } else {
                                    Color32::TRANSPARENT
                                };

                                egui::Frame::NONE
                                    .fill(frame_color)
                                    .corner_radius(4.0)
                                    .inner_margin(egui::Margin::same(6))
                                    .show(ui, |ui| {
                                        ui.vertical(|ui| {
                                            ui.horizontal(|ui| {
                                                // ID Bracket
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                                                    ui.label(egui::RichText::new("[").strong().color(palette.accent));
                                                    ui.label(egui::RichText::new(format!("{:02}", i + 1)).strong().color(palette.text_normal));
                                                    ui.label(egui::RichText::new("]").strong().color(palette.accent));
                                                });

                                                // Time Range
                                                let start_time_str = frame_to_time_local(brk.start_cut_frame as usize);
                                                let end_time_str = frame_to_time_local(brk.end_cut_frame as usize);
                                                let time_lbl = egui::Label::new(
                                                    egui::RichText::new(format!("{}-{}", start_time_str, end_time_str))
                                                        .monospace()
                                                        .color(palette.text_normal)
                                                ).sense(egui::Sense::click());

                                                if ui.add(time_lbl).clicked() {
                                                    seek_to_frame = Some((i, brk.start_cut_frame as usize));
                                                }

                                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                    // Delete button
                                                    if ui.small_button("🗑").clicked() {
                                                        to_delete = Some(i);
                                                    }

                                                    // Type Toggle
                                                    let type_str = match brk.break_type {
                                                        snippy_schema::BreakType::Commercial => "AD",
                                                        snippy_schema::BreakType::Promo => "PROMO",
                                                    };
                                                    let type_color = match brk.break_type {
                                                        snippy_schema::BreakType::Commercial => palette.cut_color,
                                                        snippy_schema::BreakType::Promo => palette.accent,
                                                    };
                                                    let type_btn = egui::Button::new(
                                                        egui::RichText::new(type_str).strong().size(10.0).color(Color32::WHITE)
                                                    ).fill(type_color);
                                                    if ui.add(type_btn).clicked() {
                                                        type_toggle = Some(i);
                                                    }
                                                });
                                            });

                                            // Notes text box
                                            ui.horizontal(|ui| {
                                                ui.label(egui::RichText::new("✍").size(12.0));
                                                let mut n = brk.notes.clone();
                                                if ui.text_edit_singleline(&mut n).changed() {
                                                    notes_change = Some((i, n));
                                                }
                                            });
                                        });
                                    });
                                ui.add_space(4.0);
                            }
                        });

                        // Apply changes outside iterate loop
                        if let Some(idx) = to_delete {
                            session.remove_break(idx);
                        }
                        if let Some(idx) = type_toggle {
                            session.toggle_break_type(idx);
                        }
                        if let Some((idx, new_notes)) = notes_change {
                            session.breaks[idx].notes = new_notes;
                            session.dirty = true;
                        }
                        if let Some((i, frame)) = seek_to_frame {
                            session.selected_break = Some(i);
                            self.playback.seek_internal(frame, false, false);
                        }
                    } else {
                        // Title: [snippets] ✂ and Inline Toggle Switch
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                            ui.label(egui::RichText::new("[").strong().color(palette.accent).size(18.0));
                            ui.label(egui::RichText::new("snippets").strong().color(palette.text_normal).size(18.0));
                            ui.label(egui::RichText::new("]").strong().color(palette.accent).size(18.0));
                            ui.add_space(6.0);
                            
                            let (scissor_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                            draw_scissor_icon(&ui.painter_at(scissor_rect), scissor_rect, palette.accent);

                            // Inline Mode Toggle Switch on the right
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let switch_resp = toggle_switch(ui, &self.settings.appearance, &mut self.cut_mode);
                                let tooltip_text = if self.cut_mode {
                                    "Mode: CUT (Snippets define what to crop out; rest is kept)"
                                } else {
                                    "Mode: KEEP (Snippets define what to keep; rest is cut)"
                                };
                                let _switch_resp = switch_resp.on_hover_text(tooltip_text);

                                ui.add_space(4.0);

                                let label_text = if self.cut_mode { "CUT" } else { "KEEP" };
                                let label_color = if self.cut_mode { palette.cut_color } else { palette.keep_color };
                                ui.label(egui::RichText::new(label_text).strong().size(11.0).color(label_color));
                            });
                        });

                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(12.0);

                        // Scrollable List of Snippets (Bracket Table Concept 10)
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let mut to_delete = None;
                            
                            egui::Grid::new("snippets_grid")
                                .num_columns(4)
                                .spacing(egui::vec2(8.0, 12.0))
                                .show(ui, |ui| {
                                    for (idx, range) in self.ranges.iter().enumerate() {
                                        // Column 1: Bracketed ID [01]
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                                            ui.label(egui::RichText::new("[").strong().color(palette.accent));
                                            ui.label(egui::RichText::new(format!("{:02}", range.id)).strong().color(palette.text_normal));
                                            ui.label(egui::RichText::new("]").strong().color(palette.accent));
                                        });

                                        // Column 2: In - Out times (monospaced)
                                        let time_str = format!("{}-{}", self.frame_to_time(range.in_frame), self.frame_to_time(range.out_frame));
                                        ui.label(egui::RichText::new(time_str).monospace().color(palette.text_normal));

                                        // Column 3: Progress mini track
                                        let (rect, _response) = ui.allocate_exact_size(
                                            egui::vec2(40.0, 4.0),
                                            egui::Sense::hover(),
                                        );
                                        let painter = ui.painter();
                                        
                                        // Draw background track
                                        painter.rect_filled(rect, 2.0, palette.widget_inactive_bg);
                                        
                                        // Draw fill segment
                                        if self.playback.total_frames() > 0 {
                                            let start_pct = range.in_frame as f32 / self.playback.total_frames() as f32;
                                            let len_pct = (range.out_frame - range.in_frame) as f32 / self.playback.total_frames() as f32;
                                            
                                            let fill_left = rect.left() + start_pct * rect.width();
                                            let fill_width = len_pct * rect.width();
                                            let fill_rect = egui::Rect::from_min_max(
                                                egui::pos2(fill_left, rect.top()),
                                                egui::pos2(fill_left + fill_width.max(1.0), rect.bottom()),
                                            );
                                            
                                            let fill_color = if self.cut_mode {
                                                palette.cut_color // cut is red
                                            } else {
                                                palette.keep_color // keep is green
                                            };
                                            painter.rect_filled(fill_rect, 2.0, fill_color);
                                        }

                                        // Column 4: Delete button
                                        if ui.small_button("🗑").on_hover_text("Remove this snippet").clicked() {
                                            to_delete = Some(idx);
                                        }

                                        ui.end_row();
                                    }
                                });

                            if let Some(idx) = to_delete {
                                self.ranges.remove(idx);
                                coalesce_and_reindex_ranges(&mut self.ranges);
                                self.next_range_id = self.ranges.len() + 1;
                            }
                        });
                    }


                });
            });

        // 4. Central Panel: The Video Viewport Deck (Remaining center space)
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE
                .fill(palette.panel_bg)
                .inner_margin(egui::Margin::same(16)))
            .show_inside(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

                        // Left Bracket [
                        ui.label(
                            egui::RichText::new("[")
                                .font(egui::FontId::new(20.0, egui::FontFamily::Name("medium".into())))
                                .color(palette.accent)
                        );
                        // Word mark "snippy"
                        ui.label(
                            egui::RichText::new("snippy")
                                .font(egui::FontId::new(18.0, egui::FontFamily::Name("medium".into())))
                                .color(palette.text_normal)
                        );

                        ui.add_space(2.0);

                        // Scissor Icon
                        let (scissor_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        draw_scissor_icon(&ui.painter_at(scissor_rect), scissor_rect, palette.accent);

                        ui.add_space(2.0);

                        // Right Bracket ]
                        ui.label(
                            egui::RichText::new("]")
                                .font(egui::FontId::new(20.0, egui::FontFamily::Name("medium".into())))
                                .color(palette.accent)
                        );

                        ui.add_space(8.0);

                        if let Some(path) = &self.video_path {
                            let filename = std::path::Path::new(path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(path);
                            ui.label(
                                egui::RichText::new(format!("—  {}", filename))
                                    .color(palette.text_muted)
                                    .font(egui::FontId::proportional(14.0))
                            );
                        }

                        // Right-justified Load, Export, and Settings buttons
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);

                            // Settings gear button
                            let gear_btn = egui::Button::new(
                                egui::RichText::new("⚙").size(18.0).color(palette.text_muted)
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(6.0)
                            .min_size(egui::vec2(36.0, 36.0));

                            let gear_resp = ui.add(gear_btn).on_hover_text("Settings (⌘,)");
                            if gear_resp.clicked() {
                                if !self.show_settings_modal {
                                    self.pending_settings = self.settings.clone();
                                    self.show_settings_modal = true;
                                }
                            }

                            ui.add_space(4.0);

                            if let Some(session) = &mut self.label_session {
                                // Save Ground Truth button
                                let save_btn = egui::Button::new(
                                    egui::RichText::new(if session.dirty { "💾 Save GT*" } else { "💾 Save GT" })
                                        .color(if session.dirty { palette.warn_color } else { palette.text_normal })
                                );
                                if ui.add(save_btn).on_hover_text("Save Ground Truth (⌘⇧S)").clicked() {
                                    match session.save() {
                                        Ok(_) => self.notifications.push(Notification::info("Ground truth saved successfully!".to_string())),
                                        Err(e) => self.notifications.push(Notification::error(format!("Failed to save: {}", e))),
                                    }
                                }

                                ui.add_space(4.0);

                                // Load Detection sidecar button
                                if session.detection_output_path.is_none() {
                                    let load_det_btn = egui::Button::new("🔍 Load Detection");
                                    if ui.add(load_det_btn).on_hover_text("Seed from a .snippy-detect.json file").clicked() {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .add_filter("Detection JSON", &["json"])
                                            .pick_file()
                                        {
                                            match session.load_detection_output(&path) {
                                                Ok(_) => self.notifications.push(Notification::info("Imported detected breaks".to_string())),
                                                Err(e) => self.notifications.push(Notification::error(format!("Failed to load detection JSON: {}", e))),
                                            }
                                        }
                                    }
                                    ui.add_space(4.0);
                                }

                                // Exit Label Mode button
                                let exit_btn = egui::Button::new("❌ Exit Label Mode");
                                if ui.add(exit_btn).clicked() {
                                    if session.dirty {
                                        let _ = session.save(); // autosave
                                    }
                                    self.label_session = None;
                                }
                            } else {
                                // Export button (Edit Mode only)
                                let can_export = self.video_path.is_some() && (!self.ranges.is_empty() || self.cut_mode);
                                ui.add_enabled_ui(can_export, |ui| {
                                    if playback_control_button(
                                        ui,
                                        &self.settings.appearance,
                                        PlaybackIcon::ExportVideo,
                                        false,
                                        "export video",
                                    ).clicked() {
                                        self.show_export_setup_modal = true;
                                        
                                        // Pre-populate export settings based on current video
                                        if let Some(ref input_path) = self.video_path {
                                            let path = std::path::Path::new(input_path);
                                            let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
                                            let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("export");
                                            
                                            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_lowercase();
                                            let format = if ["mp4", "mkv", "ts", "mov"].contains(&ext.as_str()) {
                                                ext
                                            } else {
                                                "mp4".to_string()
                                            };
                                            self.export_format = format.clone();
                                            
                                            let export_name = format!("{}_export.{}", file_stem, format);
                                            let default_out = parent.join(export_name);
                                            self.export_output_path = default_out.to_string_lossy().to_string();
                                        }
                                    }
                                });

                                ui.add_space(4.0);

                                // Enter Label Mode button (only if video is loaded)
                                if let Some(ref path_str) = self.video_path {
                                    let enter_btn = egui::Button::new("🏷 Label Mode");
                                    if ui.add(enter_btn).on_hover_text("Switch to training data labelling mode").clicked() {
                                        let total_f = self.playback.total_frames() as u64;
                                        let fps_val = self.playback.video_fps();
                                        let p = std::path::PathBuf::from(path_str);
                                        let mut sess = crate::label_session::LabelSession::new(p, total_f, fps_val);
                                        
                                        // Try auto-loading existing ground-truth sidecar
                                        let gt_path = sess.ground_truth_path.clone();
                                        if gt_path.exists() {
                                            if let Err(e) = sess.load_ground_truth(&gt_path) {
                                                self.notifications.push(Notification::error(format!("Failed to load existing ground truth: {}", e)));
                                            } else {
                                                self.notifications.push(Notification::info("Resumed existing ground truth session".to_string()));
                                            }
                                        }
                                        self.label_session = Some(sess);
                                    }
                                    ui.add_space(4.0);
                                }
                            }

                            ui.add_space(4.0);

                            // Load button
                            if playback_control_button(
                                ui,
                                &self.settings.appearance,
                                PlaybackIcon::LoadVideo,
                                false,
                                "Load video file",
                            ).clicked()
                                && let Some(path) = {
                                    let mut dlg = rfd::FileDialog::new()
                                        .add_filter("Video", &["mp4", "mkv", "avi", "mov"]);
                                    if !self.settings.general.default_open_directory.is_empty() {
                                        dlg = dlg.set_directory(&self.settings.general.default_open_directory);
                                    }
                                    dlg.pick_file()
                                }
                            {
                                    let path_str = path.to_string_lossy().to_string();
                                    self.playback.load_video(&path_str, &self.settings);
                                    self.settings.add_recent_file(&path_str);
                                    self.pending_settings.general.recent_files = self.settings.general.recent_files.clone();
                                    self.video_path = Some(path_str.clone());
                                    self.in_marker = None;
                                    self.out_marker = None;
                                    self.label_session = None; // Reset label session when a new video is loaded
                                }
                        });
                    });
                    
                    ui.add_space(10.0);

                    if self.label_session.is_some() {
                        egui::Frame::NONE
                            .fill(Color32::from_rgba_unmultiplied(230, 100, 0, 20))
                            .stroke(egui::Stroke::new(1.0, palette.warn_color))
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("🏷").size(14.0));
                                    ui.label(
                                        egui::RichText::new("LABEL MODE — Adjust boundaries, tag promos/commercials, or add missing breaks. Changes affect training data, not the video file.")
                                            .color(palette.text_normal)
                                            .font(egui::FontId::proportional(13.0))
                                    );
                                });
                            });
                        ui.add_space(8.0);
                    }


                    // Video canvas viewport drawing card
                    let controls_height = 80.0; // Expanded to accommodate progress slider
                    let canvas_height = ui.available_height() - controls_height - 10.0;

                    egui::Frame::NONE
                        .fill(palette.timeline_track_bg)
                        .stroke(egui::Stroke::new(1.0, palette.border_color))
                        .corner_radius(8.0)
                        .show(ui, |ui| {
                            let available_space = egui::vec2(ui.available_width(), canvas_height);
                            let (rect, _response) = ui.allocate_exact_size(
                                available_space,
                                egui::Sense::hover(),
                            );

                            let painter = ui.painter_at(rect);

                            let mut rendered = false;
                            if let Some(frame) = self.playback.latest_frame() {
                                let (video_w, video_h) = match frame {
                                    media::DecodedFrame::Cpu { planes, .. } => (planes.width as f32, planes.height as f32),
                                    media::DecodedFrame::Gpu { descriptor, .. } => {
                                        if let media::GpuFrameDescriptor::MacOsHwFrame(wrapper) = descriptor {
                                            (wrapper.frame.width() as f32, wrapper.frame.height() as f32)
                                        } else {
                                            (0.0, 0.0)
                                        }
                                    }
                                };

                                if video_w > 0.0 && video_h > 0.0 {
                                    let video_aspect = video_w / video_h;
                                    
                                    let container_w = rect.width();
                                    let container_h = rect.height();
                                    let container_aspect = container_w / container_h;
                                    
                                    let target_rect = if video_aspect > container_aspect {
                                        // Fit-to-width (pillarbox)
                                        let h = container_w / video_aspect;
                                        let top = rect.top() + (container_h - h) * 0.5;
                                        egui::Rect::from_min_max(
                                            egui::pos2(rect.left(), top),
                                            egui::pos2(rect.right(), top + h),
                                        )
                                    } else {
                                        // Fit-to-height (letterbox)
                                        let w = container_h * video_aspect;
                                        let left = rect.left() + (container_w - w) * 0.5;
                                        egui::Rect::from_min_max(
                                            egui::pos2(left, rect.top()),
                                            egui::pos2(left + w, rect.bottom()),
                                        )
                                    };

                                    let callback = eframe::egui_wgpu::Callback::new_paint_callback(
                                        target_rect,
                                        YuvCallback { frame: frame.clone() },
                                    );
                                    painter.add(callback);
                                    rendered = true;
                                }
                            }

                            if !rendered {
                                if self.video_path.is_none() {
                                    // Premium empty state design
                                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                        ui.centered_and_justified(|ui| {
                                            ui.vertical_centered(|ui| {
                                                let content_height = 160.0;
                                                let pad = (rect.height() - content_height) * 0.5;
                                                ui.add_space(pad.max(0.0));
                                                
                                                // 1. Draw a beautiful video symbol or icon (custom drawing)
                                                let icon_size = 48.0;
                                                let (icon_rect, icon_resp) = ui.allocate_exact_size(
                                                    egui::vec2(icon_size, icon_size),
                                                    egui::Sense::click()
                                                );
                                                let icon_painter = ui.painter_at(icon_rect);
                                                
                                                let icon_color = if icon_resp.hovered() {
                                                    palette.accent_hover
                                                } else {
                                                    palette.accent
                                                };

                                                icon_painter.rect_stroke(
                                                    icon_rect.shrink(6.0),
                                                    4.0,
                                                    egui::Stroke::new(2.0, icon_color),
                                                    egui::StrokeKind::Inside,
                                                );
                                                // Draw sprocket holes
                                                for y in 0..4 {
                                                    let offset_y = 8.0 + y as f32 * 10.0;
                                                    icon_painter.rect_filled(
                                                        egui::Rect::from_min_size(
                                                            egui::pos2(icon_rect.left() + 8.0, icon_rect.top() + offset_y),
                                                            egui::vec2(3.0, 3.0),
                                                        ),
                                                        0.5,
                                                        icon_color,
                                                    );
                                                    icon_painter.rect_filled(
                                                        egui::Rect::from_min_size(
                                                            egui::pos2(icon_rect.right() - 11.0, icon_rect.top() + offset_y),
                                                            egui::vec2(3.0, 3.0),
                                                        ),
                                                        0.5,
                                                        icon_color,
                                                    );
                                                }
                                                // Draw play triangle in the center
                                                let tri_center = icon_rect.center();
                                                let tri_p1 = egui::pos2(tri_center.x - 3.0, tri_center.y - 5.0);
                                                let tri_p2 = egui::pos2(tri_center.x - 3.0, tri_center.y + 5.0);
                                                let tri_p3 = egui::pos2(tri_center.x + 5.0, tri_center.y);
                                                icon_painter.add(egui::Shape::convex_polygon(
                                                    vec![tri_p1, tri_p2, tri_p3],
                                                    icon_color,
                                                    egui::Stroke::NONE,
                                                ));

                                                ui.add_space(16.0);

                                                // 2. Beautiful Title
                                                ui.label(
                                                    egui::RichText::new("No Video Source")
                                                        .size(20.0)
                                                        .strong()
                                                        .color(palette.text_normal)
                                                );

                                                ui.add_space(8.0);

                                                // 3. Helpful description
                                                ui.label(
                                                    egui::RichText::new("Import a video file to begin cutting snippets")
                                                        .size(13.0)
                                                        .color(Color32::from_rgb(142, 144, 166))
                                                );

                                                ui.add_space(20.0);

                                                // 4. Premium Upload / Load button
                                                let btn = egui::Button::new(
                                                    egui::RichText::new("Open Video File")
                                                        .font(egui::FontId::proportional(13.0))
                                                        .strong()
                                                        .color(Color32::WHITE)
                                                )
                                                .fill(palette.accent)
                                                .corner_radius(6.0)
                                                .min_size(egui::vec2(150.0, 32.0));

                                                let btn_resp = ui.add(btn);
                                                
                                                // Trigger load logic if clicked
                                                if btn_resp.clicked() || icon_resp.clicked() {
                                                    if let Some(path) = {
                                                        let mut dlg = rfd::FileDialog::new()
                                                            .add_filter("Video", &["mp4", "mkv", "avi", "mov"]);
                                                        if !self.settings.general.default_open_directory.is_empty() {
                                                            dlg = dlg.set_directory(&self.settings.general.default_open_directory);
                                                        }
                                                        dlg.pick_file()
                                                    } {
                                                        let path_str = path.to_string_lossy().to_string();
                                                        self.playback.load_video(&path_str, &self.settings);
                                                        self.settings.add_recent_file(&path_str);
                                                        self.pending_settings.general.recent_files = self.settings.general.recent_files.clone();
                                                        self.video_path = Some(path_str.clone());
                                                        self.in_marker = None;
                                                        self.out_marker = None;
                                                    }
                                                }
                                            });
                                        });
                                    });
                                } else {
                                    // Decoding state
                                    painter.text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "Decoding video frame...",
                                        egui::FontId::proportional(15.0),
                                        palette.text_muted,
                                    );
                                }
                            }

                            // Draw a subtle border glow inside the viewport
                            painter.rect_stroke(
                                rect,
                                8.0,
                                egui::Stroke::new(1.0, palette.accent.linear_multiply(0.1)),
                                egui::StrokeKind::Inside,
                            );

                            // Draw floating HUD overlay if enabled
                            if self.show_debug_hud {
                                let hud_width = 300.0;
                                let hud_height = 295.0;
                                let margin = 12.0;
                                let hud_rect = egui::Rect::from_min_max(
                                    egui::pos2(rect.right() - hud_width - margin, rect.top() + margin),
                                    egui::pos2(rect.right() - margin, rect.top() + hud_height + margin),
                                );
                                
                                ui.scope_builder(egui::UiBuilder::new().max_rect(hud_rect), |ui| {
                                    egui::Frame::NONE
                                        .fill(palette.card_bg)
                                        .stroke(egui::Stroke::new(1.0, palette.border_color))
                                        .corner_radius(8.0)
                                        .inner_margin(egui::Margin::same(10))
                                        .show(ui, |ui| {
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new("⚡ AV SYNC TELEMETRY").strong().color(palette.secondary));
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        let active_color = if self.playback.is_playing() { palette.keep_color } else { palette.text_muted };
                                                        ui.label(egui::RichText::new(if self.playback.is_playing() { "● ACTIVE" } else { "■ PAUSED" }).color(active_color).font(egui::FontId::monospace(10.0)));
                                                    });
                                                });
                                                
                                                ui.separator();
                                                
                                                                                let master_time = self.playback.master_clock().map_or(0.0, |mc| mc.current_time());
                                                let seq_id = self.playback.latest_frame().map_or(0, |f| f.sequence_id());
                                                let video_time = self.playback.latest_frame()
                                                    .filter(|f| f.sequence_id() == seq_id)
                                                    .map_or(self.playback.current_frame() as f64 / self.playback.video_fps(), |f| f.pts_sec());
                                                let drift = master_time - video_time;
                                                
                                                ui.horizontal(|ui| {
                                                    ui.label("Master Clock:");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{:.3}s", master_time)).monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("Video Clock :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{:.3}s", video_time)).monospace());
                                                    });
                                                });
                                                
                                                let drift_color = if drift.abs() > 0.15 { palette.cut_color } else { palette.keep_color };
                                                ui.horizontal(|ui| {
                                                    ui.label("AV Sync Drift:");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{:.1} ms ({})", drift * 1000.0, if drift > 0.0 { "Video Lag" } else { "Video Lead" })).monospace().color(drift_color));
                                                    });
                                                });
                                                
                                                ui.horizontal(|ui| {
                                                    ui.label("Sequence ID :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{}", seq_id)).monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("Stale Drops :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new("N/A").monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("Sync Pauses :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new("N/A").monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("CPAL Rate   :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                         ui.label(egui::RichText::new(format!("{} Hz", self.playback.master_clock().map_or(0, |mc| mc.sample_rate()))).monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("Decode FPS  :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        let fps_color = if self.decode_fps >= 20.0 { palette.keep_color } else { palette.cut_color };
                                                        ui.label(egui::RichText::new(format!("{:.1} fps", self.decode_fps)).monospace().color(fps_color));
                                                    });
                                                });
                                                

                                                
                                                // Task 5: Diagnostic Buttons
                                                ui.add_space(8.0);
                                                ui.horizontal(|ui| {
                                                     if ui.button("Flush Buffer").on_hover_text("Flush/drain audio ring buffer").clicked() {
                                                         self.playback.seek_internal(self.playback.current_frame(), false, true);
                                                         println!("HUD: Manual flush of audio buffer triggered");
                                                     }
                                                     if ui.button("Force Resync").on_hover_text("Force resync clocks").clicked() {
                                                         let current = self.playback.current_frame();
                                                         self.playback.seek_internal(current, false, true);
                                                         println!("HUD: Manual resync triggered to frame {}", current);
                                                     }
                                                 });
                                             });
                                         });
                                 });
                             }
                         });

                    ui.add_space(10.0);

                    // Sleek progress slider
                    let _progress_response = self.progress_bar(ui);

                    ui.add_space(10.0);

                    // Grouped Controls bar directly below video viewport
                    ui.horizontal(|ui| {
                        let full_width = ui.available_width();

                        // 1. Playback controls centered relative to the full_width
                        let controls_width = 238.0; // 6 * 26 + 34 (play) + 6 * 8 (spacing) = 238
                        let offset = (full_width - controls_width) * 0.5;
                        ui.add_space(offset.max(0.0));

                        ui.add_enabled_ui(self.video_path.is_some(), |ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(controls_width, 34.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

                                    if playback_control_button(ui, &self.settings.appearance, PlaybackIcon::Start, false, "Go to start of video").clicked() {
                                        self.playback.seek_internal(0, false, true);
                                    }
                                    let prev_resp = playback_control_button(ui, &self.settings.appearance, PlaybackIcon::PrevFrame, false, "Step backward one frame");
                                    if prev_resp.clicked() {
                                        prev_pressed = true;
                                    }
                                    if prev_resp.is_pointer_button_down_on() {
                                        prev_down = true;
                                    }
                                    if playback_control_button(ui, &self.settings.appearance, PlaybackIcon::SetIn, false, "Set In marker at current frame [").clicked() {
                                        self.in_marker = Some(self.playback.current_frame());
                                    }

                                    // Play/pause button
                                    let play_icon = if self.playback.is_playing() { PlaybackIcon::Pause } else { PlaybackIcon::Play };
                                    let play_tooltip = if self.playback.is_playing() { "Pause playback" } else { "Start playback" };
                                    if playback_control_button(ui, &self.settings.appearance, play_icon, true, play_tooltip).clicked() {
                                        self.playback.toggle_playback();
                                    }

                                    if playback_control_button(ui, &self.settings.appearance, PlaybackIcon::SetOut, false, "Set Out marker at current frame ]").clicked() {
                                        self.out_marker = Some(self.playback.current_frame());
                                    }
                                    let next_resp = playback_control_button(ui, &self.settings.appearance, PlaybackIcon::NextFrame, false, "Step forward one frame");
                                    if next_resp.clicked() {
                                        next_pressed = true;
                                    }
                                    if next_resp.is_pointer_button_down_on() {
                                        next_down = true;
                                    }
                                    if playback_control_button(ui, &self.settings.appearance, PlaybackIcon::End, false, "Go to end of video").clicked() {
                                        self.playback.seek_internal(self.playback.total_frames(), false, true);
                                    }
                                },
                            );
                        });

                        // 2. Right-justified Add Snippet button (placed at the end of the row)
                        let mut clicked = false;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let enabled = self.in_marker.is_some() && self.out_marker.is_some();
                            ui.add_enabled_ui(enabled, |ui| {
                                if playback_control_button(ui, &self.settings.appearance, PlaybackIcon::AddSnippet, false, "Add current selection as a new snippet").clicked() {
                                    clicked = true;
                                }
                            });
                        });

                        if clicked
                            && let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker)
                                && inf < outf {
                                    self.ranges.push(ExportRange {
                                        id: self.next_range_id,
                                        in_frame: inf,
                                        out_frame: outf,
                                    });
                                    coalesce_and_reindex_ranges(&mut self.ranges);
                                    self.next_range_id = self.ranges.len() + 1;
                                }
                    });
                });
            });

        if prev_pressed {
            self.playback.handle_input(media::InputEvent::ArrowLeftPressed);
        }
        if prev_down {
            self.playback.handle_input(media::InputEvent::ArrowLeftDown);
        }
        if prev_released {
            self.playback.handle_input(media::InputEvent::ArrowLeftReleased);
        }
        if next_pressed {
            self.playback.handle_input(media::InputEvent::ArrowRightPressed);
        }
        if next_down {
            self.playback.handle_input(media::InputEvent::ArrowRightDown);
        }
        if next_released {
            self.playback.handle_input(media::InputEvent::ArrowRightReleased);
        }

        // Render Export Progress Modal
        let export_status = self.export_status.clone();
        let status_guard = export_status.lock().unwrap();
        match &*status_guard {
            crate::media::types::ExportStatus::Exporting { progress, message, is_encoding, current_frame } => {
                egui::Window::new("Exporting Video")
                    .id(egui::Id::new("export_modal"))
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .fixed_size([460.0, 220.0])
                    .frame(egui::Frame::window(&ui.style()).fill(palette.panel_bg).stroke(egui::Stroke::new(1.0, palette.border_color)))
                    .show(ui.ctx(), |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("Exporting video snippet...").strong().size(16.0).color(palette.text_normal));
                            ui.add_space(12.0);
                            
                            let bar_color = if *is_encoding {
                                palette.warn_color
                            } else {
                                palette.keep_color
                            };
                            
                            // 1. Refined numeric progress bar
                            ui.add(egui::ProgressBar::new(*progress)
                                .text(format!("{:.0}%", progress * 100.0))
                                .fill(bar_color)
                            );
                            
                            ui.add_space(10.0);

                            // 2. Custom Segmented GOP Timeline Bar
                            if let Some(ref meta) = self.playback.metadata() {
                                let keyframes = &meta.keyframes;
                                let total_frames = meta.total_frames;
                                
                                if total_frames > 0 && !keyframes.is_empty() {
                                    let desired_size = egui::vec2(ui.available_width(), 16.0);
                                    let (rect, _response) = ui.allocate_at_least(desired_size, egui::Sense::hover());
                                    
                                    let painter = ui.painter();
                                    
                                    // Draw base track background (cut / ignored by default)
                                    painter.rect_filled(rect, 4.0, palette.timeline_track_bg);
                                    
                                    // Classify and draw GOP segments
                                    let mut current_type: Option<(bool, egui::Color32)> = None; // (is_kept, color)
                                    let mut segment_start = 0;
                                    
                                    for i in 0..keyframes.len() {
                                        let start_frame = keyframes[i];
                                        let end_frame = if i + 1 < keyframes.len() {
                                            keyframes[i + 1]
                                        } else {
                                            total_frames
                                        };
                                        
                                        // Classify GOP
                                        let mut kept_count = 0;
                                        let mut cut_count = 0;
                                        for f in start_frame..end_frame {
                                            if is_frame_cut_helper(f, &self.ranges, self.cut_mode) {
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
                                            let is_frame_cut = is_frame_cut_helper(f, &self.ranges, self.cut_mode);
                                            let color = match gop_type {
                                                crate::media::export::GopType::Copy => palette.keep_color,
                                                crate::media::export::GopType::Cut => palette.timeline_track_bg,
                                                crate::media::export::GopType::Transcode => {
                                                    if is_frame_cut {
                                                        palette.timeline_track_bg
                                                    } else {
                                                        palette.warn_color
                                                    }
                                                }
                                            };
                                            let is_kept = !is_frame_cut && gop_type != crate::media::export::GopType::Cut;
                                            
                                            if let Some((last_kept_state, last_color)) = current_type {
                                                if last_kept_state != is_kept || last_color != color {
                                                    // Draw previous segment
                                                    if last_kept_state {
                                                        let start_pct = segment_start as f32 / total_frames as f32;
                                                        let end_pct = f as f32 / total_frames as f32;
                                                        let seg_rect = egui::Rect::from_min_max(
                                                            egui::pos2(rect.left() + rect.width() * start_pct, rect.top()),
                                                            egui::pos2(rect.left() + rect.width() * end_pct, rect.bottom()),
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
                                    
                                    // Draw final segment
                                    if let Some((last_kept_state, last_color)) = current_type {
                                        if last_kept_state {
                                            let start_pct = segment_start as f32 / total_frames as f32;
                                            let end_pct = total_frames as f32 / total_frames as f32;
                                            let seg_rect = egui::Rect::from_min_max(
                                                egui::pos2(rect.left() + rect.width() * start_pct, rect.top()),
                                                egui::pos2(rect.left() + rect.width() * end_pct, rect.bottom()),
                                            );
                                            painter.rect_filled(seg_rect, 0.0, last_color);
                                        }
                                    }
                                    
                                    // Draw border around the track
                                    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, palette.border_color), egui::StrokeKind::Inside);
                                    
                                    // Draw playhead at current_frame
                                    let playhead_pct = (*current_frame as f32 / total_frames as f32).clamp(0.0, 1.0);
                                    let playhead_x = rect.left() + rect.width() * playhead_pct;
                                    painter.line_segment(
                                        [egui::pos2(playhead_x, rect.top() - 2.0), egui::pos2(playhead_x, rect.bottom() + 2.0)],
                                        egui::Stroke::new(2.5, palette.accent),
                                    );
                                }
                            }
                            
                            // 3. Timeline Legend
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_justify(false), |ui| {
                                    let total_width = 360.0;
                                    let space = (ui.available_width() - total_width).max(0.0) / 2.0;
                                    ui.add_space(space);
                                    
                                    let (rect, _) = ui.allocate_at_least(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                    ui.painter().circle_filled(rect.center(), 4.0, palette.keep_color);
                                    ui.label(egui::RichText::new("Pass-through").size(10.0).color(palette.text_muted));
                                    
                                    ui.add_space(10.0);
                                    
                                    let (rect, _) = ui.allocate_at_least(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                    ui.painter().circle_filled(rect.center(), 4.0, palette.warn_color);
                                    ui.label(egui::RichText::new("Re-encode").size(10.0).color(palette.text_muted));
                                    
                                    ui.add_space(10.0);
                                    
                                    let (rect, _) = ui.allocate_at_least(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                    ui.painter().circle_filled(rect.center(), 4.0, palette.timeline_track_bg);
                                    ui.label(egui::RichText::new("Cut / Ignored").size(10.0).color(palette.text_muted));
                                });
                            });
                            
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new(message).size(12.0).color(palette.text_muted));
                            ui.add_space(12.0);
                            
                            if ui.button(egui::RichText::new("Cancel").color(palette.warn_color)).clicked() {
                                self.export_cancel_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        });
                    });
            }
            _ => {}
        }
    }
}

struct YuvCallback {
    frame: media::DecodedFrame,
}

impl eframe::egui_wgpu::CallbackTrait for YuvCallback {
    fn prepare(
        &self,
        device: &eframe::wgpu::Device,
        queue: &eframe::wgpu::Queue,
        _screen_descriptor: &eframe::egui_wgpu::ScreenDescriptor,
        _encoder: &mut eframe::wgpu::CommandEncoder,
        callback_resources: &mut eframe::egui_wgpu::CallbackResources,
    ) -> Vec<eframe::wgpu::CommandBuffer> {
        if let Some(renderer) = callback_resources.get_mut::<yuv_renderer::YuvRenderer>() {
            match &self.frame {
                crate::media::DecodedFrame::Cpu { planes, color_space, color_range, .. } => {
                    renderer.update_textures(device, queue, planes, *color_space, *color_range);
                }
                crate::media::DecodedFrame::Gpu { descriptor, color_space, color_range, .. } => {
                    if let crate::media::GpuFrameDescriptor::MacOsHwFrame(wrapper) = descriptor {
                        renderer.update_textures_from_hw_frame(device, queue, wrapper.clone(), *color_space, *color_range);
                    }
                }
            }
        }
        vec![]
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut eframe::wgpu::RenderPass<'static>,
        callback_resources: &eframe::egui_wgpu::CallbackResources,
    ) {
        let viewport = info.viewport_in_pixels();
        let _clip_rect = info.clip_rect_in_pixels();

        if let Some(renderer) = callback_resources.get::<yuv_renderer::YuvRenderer>() {
            let (draw_x, draw_y, draw_width, draw_height) = if let Some((video_width, video_height)) = renderer.video_size() {
                let view_ratio = viewport.width_px as f32 / viewport.height_px as f32;
                let video_ratio = video_width as f32 / video_height as f32;

                if view_ratio > video_ratio {
                    // view is wider than video, pillarbox
                    let h = viewport.height_px as f32;
                    let w = h * video_ratio;
                    let x = viewport.left_px as f32 + (viewport.width_px as f32 - w) / 2.0;
                    (x, viewport.top_px as f32, w, h)
                } else {
                    // view is taller than video, letterbox
                    let w = viewport.width_px as f32;
                    let h = w / video_ratio;
                    let y = viewport.top_px as f32 + (viewport.height_px as f32 - h) / 2.0;
                    (viewport.left_px as f32, y, w, h)
                }
            } else {
                (viewport.left_px as f32, viewport.top_px as f32, viewport.width_px as f32, viewport.height_px as f32)
            };

            render_pass.set_viewport(
                draw_x,
                draw_y,
                draw_width,
                draw_height,
                0.0,
                1.0,
            );

            renderer.render(render_pass);
        }
    }
}

fn draw_hatching(painter: &egui::Painter, rect: egui::Rect, color: Color32, stroke_width: f32, spacing: f32) {
    let clipped_painter = painter.with_clip_rect(rect);
    let mut x = rect.min.x - rect.height();
    while x < rect.max.x {
        let p1 = egui::pos2(x, rect.min.y);
        let p2 = egui::pos2(x + rect.height(), rect.max.y);
        clipped_painter.line_segment([p1, p2], egui::Stroke::new(stroke_width, color));
        x += spacing;
    }
}

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
    
    // Sort ranges by start frame to ensure left-to-right evaluation
    ranges.sort_by_key(|r| r.in_frame);
    
    let mut merged: Vec<ExportRange> = Vec::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        if let Some(last) = merged.last_mut() {
            // If the current range overlaps or is contiguous with the last one
            if range.in_frame <= last.out_frame + 1 {
                last.out_frame = last.out_frame.max(range.out_frame);
            } else {
                merged.push(range);
            }
        } else {
            merged.push(range);
        }
    }
    
    // Re-assign IDs to be 1..N
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
        // 1. Empty case
        let mut ranges = vec![];
        coalesce_and_reindex_ranges(&mut ranges);
        assert!(ranges.is_empty());

        // 2. Overlapping, out-of-order, and contiguous case
        let mut ranges = vec![
            ExportRange { id: 99, in_frame: 21, out_frame: 30 }, // contiguous to 10..20
            ExportRange { id: 100, in_frame: 10, out_frame: 20 },
            ExportRange { id: 101, in_frame: 15, out_frame: 25 }, // overlaps both
            ExportRange { id: 102, in_frame: 40, out_frame: 50 }, // distinct
        ];
        coalesce_and_reindex_ranges(&mut ranges);
        
        assert_eq!(ranges.len(), 2);
        
        // The first 3 should be merged into 10..30
        assert_eq!(ranges[0].id, 1);
        assert_eq!(ranges[0].in_frame, 10);
        assert_eq!(ranges[0].out_frame, 30);
        
        // The last one should be 40..50, re-indexed to 2
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

    fn oklch_to_rgb_and_back(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
        let (l, c, h) = crate::ui::theme::rgb_to_oklch(r, g, b);
        let rgb_back = crate::ui::theme::oklch_to_rgb(l, c, h);
        let dist = ((r as f32 - rgb_back.r() as f32).powi(2)
            + (g as f32 - rgb_back.g() as f32).powi(2)
            + (b as f32 - rgb_back.b() as f32).powi(2))
            .sqrt();
        assert!(dist < 15.0, "Dist between original and round-tripped is too large: {dist} (original: {r},{g},{b}, returned: {rgb_back:?})");
        (l, c, h)
    }
}



