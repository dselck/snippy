use eframe::egui::{self, Color32, Rect, RichText, Stroke, StrokeKind, pos2, vec2};
use crate::media;
use crate::ui::icons::{self, IconType};
use crate::ui::yuv_renderer;
use crate::ui::MainApp;

pub struct YuvCallback {
    pub frame: media::DecodedFrame,
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
                    match descriptor {
                        #[cfg(target_os = "macos")]
                        crate::media::GpuFrameDescriptor::MacOsHwFrame(wrapper) => {
                            renderer.update_textures_from_hw_frame(device, queue, wrapper.clone(), *color_space, *color_range);
                        }
                        #[cfg(target_os = "linux")]
                        crate::media::GpuFrameDescriptor::LinuxHwFrame(wrapper) => {
                            renderer.update_textures_from_hw_frame(device, queue, wrapper.clone(), *color_space, *color_range);
                        }
                        #[allow(unreachable_patterns)]
                        _ => {}
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
                    let h = viewport.height_px as f32;
                    let w = h * video_ratio;
                    let x = viewport.left_px as f32 + (viewport.width_px as f32 - w) / 2.0;
                    (x, viewport.top_px as f32, w, h)
                } else {
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

pub fn render_viewport(app: &mut MainApp, ui: &mut egui::Ui) {
    let palette = app.theme_palette();
    let transport_height = 42.0;
    let canvas_height = (ui.available_height() - transport_height - 6.0).max(100.0);

    // 1. Studio Monitor Frame
    egui::Frame::NONE
        .fill(palette.timeline_track_bg)
        .stroke(Stroke::new(1.0_f32, palette.border_color))
        .corner_radius(2.0)
        .show(ui, |ui| {
            let available_size = vec2(ui.available_width(), canvas_height);
            let (rect, _response) = ui.allocate_exact_size(available_size, egui::Sense::hover());
            let painter = ui.painter_at(rect);

            let mut rendered = false;
            if let Some(frame) = app.playback.latest_frame() {
                let (video_w, video_h) = match frame {
                    media::DecodedFrame::Cpu { planes, .. } => (planes.width as f32, planes.height as f32),
                    media::DecodedFrame::Gpu { descriptor, .. } => {
                        match descriptor {
                            #[cfg(target_os = "macos")]
                            media::GpuFrameDescriptor::MacOsHwFrame(wrapper) => {
                                (wrapper.frame.width() as f32, wrapper.frame.height() as f32)
                            }
                            #[cfg(target_os = "linux")]
                            media::GpuFrameDescriptor::LinuxHwFrame(wrapper) => {
                                (wrapper.frame.width() as f32, wrapper.frame.height() as f32)
                            }
                            #[allow(unreachable_patterns)]
                            _ => (0.0, 0.0),
                        }
                    }
                };

                if video_w > 0.0 && video_h > 0.0 {
                    let video_aspect = video_w / video_h;
                    let container_w = rect.width();
                    let container_h = rect.height();
                    let container_aspect = container_w / container_h;

                    let target_rect = if video_aspect > container_aspect {
                        let h = container_w / video_aspect;
                        let top = rect.top() + (container_h - h) * 0.5;
                        Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), top + h))
                    } else {
                        let w = container_h * video_aspect;
                        let left = rect.left() + (container_w - w) * 0.5;
                        Rect::from_min_max(pos2(left, rect.top()), pos2(left + w, rect.bottom()))
                    };

                    let callback = eframe::egui_wgpu::Callback::new_paint_callback(
                        target_rect,
                        YuvCallback { frame: frame.clone() },
                    );
                    painter.add(callback);
                    rendered = true;

                    // Subtle monitor bezel border around the active video area
                    painter.rect_stroke(
                        target_rect,
                        0.0,
                        Stroke::new(1.0_f32, Color32::from_black_alpha(120)),
                        StrokeKind::Inside,
                    );
                }
            }

            if !rendered {
                if app.video_path.is_none() {
                    // Precision Broadcast Slate / Empty State
                    draw_broadcast_empty_slate(&painter, rect, &palette);
                    
                    // Centered Open Action Button
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(20.0);
                                ui.label(
                                    RichText::new("NO MEDIA LOADED")
                                        .strong()
                                        .size(14.0)
                                        .color(palette.text_muted)
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Drag and drop video file here or open from disk")
                                        .size(11.0)
                                        .color(palette.text_muted)
                                );
                                ui.add_space(14.0);

                                let open_btn = egui::Button::new(
                                    RichText::new("Open Video File")
                                        .size(12.0)
                                        .strong()
                                        .color(Color32::WHITE)
                                )
                                .fill(palette.accent)
                                .corner_radius(3.0)
                                .min_size(vec2(140.0, 28.0));

                                if ui.add(open_btn).clicked() {
                                    app.open_file_dialog();
                                }
                            });
                        });
                    });
                } else {
                    painter.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Decoding frame...",
                        egui::FontId::proportional(13.0),
                        palette.text_muted,
                    );
                }
            }

            // 2. Floating On-Screen Display (OSD) Overlay
            if app.video_path.is_some() {
                render_osd_overlay(&painter, rect, app, &palette);
            }

            // 3. Diagnostics Telemetry HUD Overlay
            if app.show_debug_hud {
                let hud_width = 300.0;
                let hud_height = 295.0;
                let margin = 12.0;
                let hud_rect = Rect::from_min_max(
                    pos2(rect.right() - hud_width - margin, rect.top() + margin),
                    pos2(rect.right() - margin, rect.top() + hud_height + margin),
                );

                ui.scope_builder(egui::UiBuilder::new().max_rect(hud_rect), |ui| {
                    egui::Frame::NONE
                        .fill(palette.card_bg)
                        .stroke(Stroke::new(1.0_f32, palette.border_color))
                        .corner_radius(3.0)
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("⚡ AV SYNC TELEMETRY").strong().color(palette.secondary));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let active_color = if app.playback.is_playing() { palette.keep_color } else { palette.text_muted };
                                        ui.label(RichText::new(if app.playback.is_playing() { "● ACTIVE" } else { "■ PAUSED" }).color(active_color).font(egui::FontId::monospace(10.0)));
                                    });
                                });

                                ui.separator();

                                let master_time = app.playback.master_clock().map_or(0.0, |mc| mc.current_time());
                                let seq_id = app.playback.latest_frame().map_or(0, |f| f.sequence_id());
                                let video_time = app.playback.latest_frame()
                                    .filter(|f| f.sequence_id() == seq_id)
                                    .map_or(app.playback.current_frame() as f64 / app.playback.video_fps(), |f| f.pts_sec());
                                let drift = master_time - video_time;

                                ui.horizontal(|ui| {
                                    ui.label("Master Clock:");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{:.3}s", master_time)).monospace());
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Video Clock :");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{:.3}s", video_time)).monospace());
                                    });
                                });

                                let drift_color = if drift.abs() > 0.15 { palette.cut_color } else { palette.keep_color };
                                ui.horizontal(|ui| {
                                    ui.label("AV Sync Drift:");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{:.1} ms ({})", drift * 1000.0, if drift > 0.0 { "Video Lag" } else { "Video Lead" })).monospace().color(drift_color));
                                    });
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Sequence ID :");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{}", seq_id)).monospace());
                                    });
                                });

                                ui.horizontal(|ui| {
                                    ui.label("CPAL Rate   :");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{} Hz", app.playback.master_clock().map_or(0, |mc| mc.sample_rate()))).monospace());
                                    });
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Decode FPS  :");
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let fps_color = if app.decode_fps >= 20.0 { palette.keep_color } else { palette.cut_color };
                                        ui.label(RichText::new(format!("{:.1} fps", app.decode_fps)).monospace().color(fps_color));
                                    });
                                });

                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Flush Buffer").clicked() {
                                        app.playback.seek_internal(app.playback.current_frame(), false, true);
                                    }
                                    if ui.button("Force Resync").clicked() {
                                        let current = app.playback.current_frame();
                                        app.playback.seek_internal(current, false, true);
                                    }
                                });
                            });
                        });
                });
            }
        });

    ui.add_space(6.0);

    // 2. Dedicated Broadcast Transport Deck
    render_transport_deck(app, ui, &palette);
}

fn draw_broadcast_empty_slate(p: &egui::Painter, r: Rect, palette: &crate::ui::ThemePalette) {
    let c = r.center();
    let stroke = Stroke::new(1.0_f32, palette.border_color.linear_multiply(0.5));
    
    // Crosshair reticle
    p.line_segment([pos2(c.x - 30.0, c.y - 40.0), pos2(c.x + 30.0, c.y - 40.0)], stroke);
    p.line_segment([pos2(c.x, c.y - 70.0), pos2(c.x, c.y - 10.0)], stroke);
    p.circle_stroke(pos2(c.x, c.y - 40.0), 16.0, stroke);
    p.circle_stroke(pos2(c.x, c.y - 40.0), 8.0, stroke);
}

fn render_osd_overlay(
    p: &egui::Painter,
    r: Rect,
    app: &MainApp,
    palette: &crate::ui::ThemePalette,
) {
    let current_frame = app.playback.current_frame();
    let total_frames = app.playback.total_frames();
    let fps = app.playback.video_fps();
    
    // Top-left Timecode & Frame Pill
    let tc_str = app.frame_to_smpte_timecode(current_frame, fps);
    let frame_str = format!("F: {:06} / {:06}", current_frame, total_frames);
    
    let pill_pos = pos2(r.left() + 10.0, r.top() + 10.0);
    let pill_rect = Rect::from_min_size(pill_pos, vec2(170.0, 42.0));
    
    p.rect_filled(pill_rect, 2.0, Color32::from_black_alpha(170));
    p.rect_stroke(pill_rect, 2.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)), StrokeKind::Inside);
    
    p.text(
        pos2(pill_rect.left() + 8.0, pill_rect.top() + 5.0),
        egui::Align2::LEFT_TOP,
        &tc_str,
        egui::FontId::monospace(14.0),
        Color32::WHITE,
    );
    p.text(
        pos2(pill_rect.left() + 8.0, pill_rect.top() + 24.0),
        egui::Align2::LEFT_TOP,
        &frame_str,
        egui::FontId::monospace(10.0),
        palette.text_muted,
    );

    // Top-right Telemetry Pill (Sync & FPS)
    let fps_text = format!("{:.1} FPS", app.decode_fps);
    let right_pill_pos = pos2(r.right() - 95.0, r.top() + 10.0);
    let right_rect = Rect::from_min_size(right_pill_pos, vec2(85.0, 24.0));
    p.rect_filled(right_rect, 2.0, Color32::from_black_alpha(170));
    p.rect_stroke(right_rect, 2.0, Stroke::new(1.0_f32, Color32::from_white_alpha(30)), StrokeKind::Inside);
    p.text(
        right_rect.center(),
        egui::Align2::CENTER_CENTER,
        &fps_text,
        egui::FontId::monospace(11.0),
        if app.decode_fps >= 24.0 { palette.keep_color } else { palette.warn_color },
    );
}

fn render_transport_deck(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    ui.horizontal(|ui| {
        let total_w = ui.available_width();
        let has_video = app.video_path.is_some();

        // 1. Left: Timecode readout in Mono
        ui.allocate_ui_with_layout(
            vec2(180.0, 32.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                if has_video {
                    let cur_str = app.frame_to_time(app.playback.current_frame());
                    let tot_str = app.frame_to_time(app.playback.total_frames());
                    ui.label(
                        RichText::new(format!("{} / {}", cur_str, tot_str))
                            .monospace()
                            .size(12.0)
                            .color(palette.text_normal)
                    );
                }
            },
        );

        // 2. Center: Symmetric Hardware Transport Controls
        let controls_w = 260.0;
        let left_pad = ((total_w - controls_w) * 0.5 - 180.0).max(0.0);
        ui.add_space(left_pad);

        ui.add_enabled_ui(has_video, |ui| {
            ui.allocate_ui_with_layout(
                vec2(controls_w, 32.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 0.0);

                    // Jump Start
                    if icons::icon_button(ui, IconType::JumpStart, vec2(28.0, 28.0), false, "Jump to start (Home)").clicked() {
                        app.playback.seek_internal(0, false, true);
                    }

                    // Step Backward
                    if icons::icon_button(ui, IconType::StepBackward, vec2(28.0, 28.0), false, "Step backward 1 frame (←)").clicked() {
                        app.playback.step_backward();
                    }

                    // Mark In
                    let is_in = app.in_marker == Some(app.playback.current_frame());
                    if icons::icon_button(ui, IconType::MarkIn, vec2(30.0, 28.0), is_in, "Mark In point ([)").clicked() {
                        app.in_marker = Some(app.playback.current_frame());
                    }

                    // Primary Play / Pause Button (Hero button)
                    let is_playing = app.playback.is_playing();
                    let play_icon = if is_playing { IconType::Pause } else { IconType::Play };
                    let (play_rect, play_resp) = ui.allocate_exact_size(vec2(44.0, 30.0), egui::Sense::click());
                    if ui.is_rect_visible(play_rect) {
                        let fill = if play_resp.hovered() { palette.accent_hover } else { palette.accent };
                        ui.painter().rect_filled(play_rect, 3.0, fill);
                        icons::draw_icon(ui.painter(), play_rect.shrink(4.0), play_icon, Color32::WHITE);
                    }
                    if play_resp.on_hover_text("Play / Pause (Space)").clicked() {
                        app.playback.toggle_playback();
                    }

                    // Mark Out
                    let is_out = app.out_marker == Some(app.playback.current_frame());
                    if icons::icon_button(ui, IconType::MarkOut, vec2(30.0, 28.0), is_out, "Mark Out point (])").clicked() {
                        app.out_marker = Some(app.playback.current_frame());
                    }

                    // Step Forward
                    if icons::icon_button(ui, IconType::StepForward, vec2(28.0, 28.0), false, "Step forward 1 frame (→)").clicked() {
                        app.playback.step_forward();
                    }

                    // Jump End
                    if icons::icon_button(ui, IconType::JumpEnd, vec2(28.0, 28.0), false, "Jump to end (End)").clicked() {
                        app.playback.seek_internal(app.playback.total_frames(), false, true);
                    }
                },
            );
        });

        // 3. Right: Mode Toggle Switch + Add Snippet Button
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 0.0);

            // Add Snippet Button
            let can_add = app.in_marker.is_some() && app.out_marker.is_some()
                && app.in_marker.unwrap() < app.out_marker.unwrap();

            ui.add_enabled_ui(can_add, |ui| {
                let add_btn = egui::Button::new(
                    RichText::new("+ Add Snippet")
                        .size(11.0)
                        .strong()
                        .color(if can_add { Color32::WHITE } else { palette.text_muted })
                )
                .fill(if can_add { palette.accent } else { palette.widget_inactive_bg })
                .corner_radius(3.0)
                .min_size(vec2(90.0, 26.0));

                if ui.add(add_btn).on_hover_text("Add current In/Out range as snippet (Enter)").clicked() {
                    app.add_snippet_from_markers();
                }
            });

            // Cut / Keep Mode Switcher Pill
            let mode_color = if app.cut_mode { palette.cut_color } else { palette.keep_color };
            let mode_label = if app.cut_mode { "CUT MODE" } else { "KEEP MODE" };
            let mode_btn = egui::Button::new(
                RichText::new(mode_label)
                    .size(10.0)
                    .strong()
                    .color(mode_color)
            )
            .fill(palette.widget_inactive_bg)
            .stroke(Stroke::new(1.0_f32, mode_color.linear_multiply(0.5)))
            .corner_radius(3.0)
            .min_size(vec2(76.0, 22.0));

            if ui.add(mode_btn).on_hover_text("Toggle between CUT mode (crop snippets out) and KEEP mode (keep only snippets)").clicked() {
                app.cut_mode = !app.cut_mode;
            }
        });
    });
}
