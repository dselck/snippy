use eframe::egui::{self, Color32, Rect, RichText, Shape, Stroke, StrokeKind, pos2, vec2};
use crate::settings::PlayheadMode;
use crate::ui::icons::{self, IconType};
use crate::ui::MainApp;

pub fn render_timeline(app: &mut MainApp, ui: &mut egui::Ui, anim_height: f32) {
    let palette = app.theme_palette();
    let total_width = ui.available_width();
    
    if app.video_path.is_none() {
        ui.centered_and_justified(|ui| {
            ui.label(
                RichText::new("No media loaded — Open a video to activate timeline")
                    .size(13.0)
                    .color(palette.text_muted)
            );
        });
        return;
    }

    let fps = app.playback.video_fps();
    let total_frames = app.playback.total_frames();
    if fps <= 0.0 || total_frames == 0 {
        return;
    }

    let current_frame = app.playback.current_frame();
    let total_duration = total_frames as f64 / fps;
    let current_time_sec = current_frame as f64 / fps;

    // --- Mode 1: Slim Resting Overview Mode (macOS Dock Resting State) ---
    if anim_height < 68.0 {
        render_slim_timeline_overview(app, ui, &palette, total_width, current_frame, total_frames, fps);
        return;
    }

    // --- Mode 2: Expanded Studio Workstation Mode (macOS Dock Active State) ---
    // Zoom and pixel scaling calculations
    let min_pps = total_width as f64 / total_duration.max(0.001);
    let max_pps = min_pps * 30.0;
    let pps = min_pps + (max_pps - min_pps) * (app.zoom_level as f64);

    let ruler_height = 22.0;
    let footer_height = 30.0;
    let track_gap = 4.0;
    let avail_track_h = (anim_height - ruler_height - footer_height - track_gap - 12.0).max(40.0);
    let filmstrip_height = (avail_track_h * 0.62).clamp(32.0, 160.0);
    let wave_height = (avail_track_h * 0.38).clamp(18.0, 80.0);

    let playhead_mode = app.settings.playback.playhead_mode;
    let (scroll_offset_x, playhead_x) = match playhead_mode {
        PlayheadMode::CenterAnchored => {
            let offset = (total_width as f64 * 0.5) - (current_time_sec * pps);
            let px = total_width as f32 * 0.5;
            (offset, px)
        }
        PlayheadMode::FreeTravelling => {
            let visible_duration = total_width as f64 / pps;
            let page_start = if current_time_sec > visible_duration * 0.8 {
                (current_time_sec - visible_duration * 0.8).max(0.0)
            } else {
                0.0
            };
            let offset = -page_start * pps;
            let px = (offset + current_time_sec * pps) as f32;
            (offset, px)
        }
    };

    // 1. Timecode Ruler Track
    let (ruler_rect, ruler_response) = ui.allocate_exact_size(
        vec2(total_width, ruler_height),
        egui::Sense::click_and_drag(),
    );
    let ruler_painter = ui.painter_at(ruler_rect);

    ruler_painter.rect_filled(ruler_rect, 0.0, palette.ruler_bg);
    ruler_painter.line_segment(
        [pos2(ruler_rect.left(), ruler_rect.bottom()), pos2(ruler_rect.right(), ruler_rect.bottom())],
        Stroke::new(1.0_f32, palette.border_color),
    );

    // Draw major and minor time ticks on ruler
    let tick_interval_sec = if pps > 100.0 {
        1.0
    } else if pps > 30.0 {
        5.0
    } else if pps > 10.0 {
        15.0
    } else if pps > 3.0 {
        30.0
    } else {
        60.0
    };

    let start_sec = ((-scroll_offset_x) / pps).max(0.0);
    let end_sec = ((total_width as f64 - scroll_offset_x) / pps).min(total_duration);

    let first_tick = (start_sec / tick_interval_sec).floor() as usize;
    let last_tick = (end_sec / tick_interval_sec).ceil() as usize;

    for t in first_tick..=last_tick {
        let sec = t as f64 * tick_interval_sec;
        let x = ruler_rect.left() + (scroll_offset_x + sec * pps) as f32;

        if x >= ruler_rect.left() - 20.0 && x <= ruler_rect.right() + 20.0 {
            // Major tick
            ruler_painter.line_segment(
                [pos2(x, ruler_rect.bottom() - 8.0), pos2(x, ruler_rect.bottom())],
                Stroke::new(1.0_f32, palette.ruler_tick),
            );

            // Timecode label
            let tc_str = app.format_seconds(sec);
            ruler_painter.text(
                pos2(x + 4.0, ruler_rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                tc_str,
                egui::FontId::monospace(10.0),
                palette.ruler_text,
            );

            // Minor ticks between majors
            let minor_count = 4;
            for m in 1..minor_count {
                let m_sec = sec + (m as f64 * tick_interval_sec / minor_count as f64);
                let mx = ruler_rect.left() + (scroll_offset_x + m_sec * pps) as f32;
                if mx >= ruler_rect.left() && mx <= ruler_rect.right() {
                    ruler_painter.line_segment(
                        [pos2(mx, ruler_rect.bottom() - 4.0), pos2(mx, ruler_rect.bottom())],
                        Stroke::new(1.0_f32, palette.ruler_tick.linear_multiply(0.6)),
                    );
                }
            }
        }
    }

    // Ruler scrub interaction
    if ruler_response.dragged() || ruler_response.clicked() {
        app.playback.set_playing(false);
        if let Some(pos) = ruler_response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos())) {
            let relative_x = pos.x - ruler_rect.left();
            let target_sec = (relative_x as f64 - scroll_offset_x) / pps;
            let target_frame = (target_sec * fps).round().clamp(0.0, total_frames as f64 - 1.0) as usize;
            app.playback.seek_internal(target_frame, false, true);
        }
    }

    ui.add_space(2.0);

    // 2. Video Filmstrip Track
    let (filmstrip_rect, filmstrip_response) = ui.allocate_exact_size(
        vec2(total_width, filmstrip_height),
        egui::Sense::click_and_drag(),
    );
    let filmstrip_painter = ui.painter_at(filmstrip_rect);

    // Track bed background
    filmstrip_painter.rect_filled(filmstrip_rect, 2.0, palette.timeline_track_bg);
    filmstrip_painter.rect_stroke(filmstrip_rect, 2.0, Stroke::new(1.0_f32, palette.border_color), StrokeKind::Inside);

    // Bucket height to 64px increments for thumbnail decoding stability
    let bucketed_height = (((filmstrip_height / 64.0).round() as u32) * 64).clamp(64, 256) as f32;
    if (bucketed_height - app.last_thumbnail_height).abs() > 0.01 {
        app.last_thumbnail_height = bucketed_height;
        app.in_flight_requests.clear();
        app.thumbnails_dirty = true;
    }

    // Render continuous video thumbnails
    let thumb_aspect = 16.0 / 9.0;
    let thumb_width = filmstrip_height * thumb_aspect;
    let thumb_interval_sec = (thumb_width as f64 / pps).max(0.1);

    let first_thumb_idx = (start_sec / thumb_interval_sec).floor() as isize;
    let last_thumb_idx = (end_sec / thumb_interval_sec).ceil() as isize;

    for i in first_thumb_idx..=last_thumb_idx {
        let t_sec = i as f64 * thumb_interval_sec;
        let frame_idx = (t_sec * fps).round() as usize;

        if frame_idx < total_frames {
            let x = filmstrip_rect.left() + (scroll_offset_x + t_sec * pps) as f32;
            let thumb_rect = Rect::from_min_size(pos2(x, filmstrip_rect.top() + 1.0), vec2(thumb_width, filmstrip_height - 2.0));

            // Render cached thumbnail or placeholder
            if let Some(texture) = app.thumbnail_cache.get(&frame_idx) {
                filmstrip_painter.image(
                    texture.id(),
                    thumb_rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                // Background subtle tile
                filmstrip_painter.rect_filled(thumb_rect, 0.0, palette.card_bg);
                // Dispatch decode request if not in flight
                if !app.in_flight_requests.contains_key(&frame_idx) {
                    if let Some(tx) = &app.playback.thumbnail_tx {
                        let _ = tx.send(crate::media::ThumbnailCommand::Request {
                            visible_frames: vec![frame_idx],
                            padding_frames: vec![],
                            height: bucketed_height as u32,
                        });
                        app.in_flight_requests.insert(frame_idx, std::time::Instant::now());
                    }
                }
            }

            // Divider between thumbnail frames
            filmstrip_painter.line_segment(
                [pos2(thumb_rect.right(), thumb_rect.top()), pos2(thumb_rect.right(), thumb_rect.bottom())],
                Stroke::new(1.0_f32, Color32::from_black_alpha(80)),
            );
        }
    }

    // Defect Warning Markers on Filmstrip
    if app.settings.timeline.show_timeline_defect_markers {
        let defect_ranges = app.playback.defect_map().read().unwrap().ranges().to_vec();
        for range in &defect_ranges {
            let start_x = (filmstrip_rect.left() as f64 + scroll_offset_x + range.start_pts_sec * pps) as f32;
            let end_x = (filmstrip_rect.left() as f64 + scroll_offset_x + range.end_pts_sec * pps) as f32;
            let left = start_x.max(filmstrip_rect.left());
            let right = end_x.max(start_x + 2.0).min(filmstrip_rect.right());
            if left < right {
                let defect_rect = Rect::from_min_max(
                    pos2(left, filmstrip_rect.top()),
                    pos2(right, filmstrip_rect.top() + 5.0),
                );
                filmstrip_painter.rect_filled(defect_rect, 0.0, Color32::from_rgba_unmultiplied(230, 160, 30, 180));
            }
        }
    }

    // Scrub handling on filmstrip
    if filmstrip_response.dragged() || filmstrip_response.clicked() {
        app.playback.set_playing(false);
        if let Some(pos) = filmstrip_response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos())) {
            let relative_x = pos.x - filmstrip_rect.left();
            let target_sec = (relative_x as f64 - scroll_offset_x) / pps;
            let target_frame = (target_sec * fps).round().clamp(0.0, total_frames as f64 - 1.0) as usize;
            app.playback.seek_internal(target_frame, false, true);
        }
    }

    ui.add_space(track_gap);

    // 3. Synchronized Audio Waveform Track
    let (wave_rect, _wave_response) = ui.allocate_exact_size(
        vec2(total_width, wave_height),
        egui::Sense::hover(),
    );
    let wave_painter = ui.painter_at(wave_rect);

    wave_painter.rect_filled(wave_rect, 2.0, palette.card_bg);
    wave_painter.rect_stroke(wave_rect, 2.0, Stroke::new(1.0_f32, palette.border_color), StrokeKind::Inside);

    // Render RMS loudness bars
    let loudness = app.playback.loudness_profile();
    let total_buckets = loudness.len();
    let bar_count = wave_rect.width() as usize;
    let bar_width = 1.0_f32;
    let db_floor = app.settings.playback.waveform_db_floor;

    for i in 0..bar_count {
        let x = wave_rect.left() + i as f32 * bar_width;
        let sec = ((x - wave_rect.left()) as f64 - scroll_offset_x) / pps;
        let frame_d = sec * fps;
        let frame_idx = frame_d.round() as isize;

        if frame_idx >= 0 && frame_idx < total_frames as isize {
            let b_idx = (frame_d * 4.0).floor() as usize;
            if b_idx < total_buckets {
                if let Some(Some(amp)) = loudness.get(b_idx) {
                    let scaled_amp = if db_floor >= 0.0 {
                        *amp
                    } else if *amp <= 0.0 {
                        0.0
                    } else {
                        let db = 20.0 * amp.log10();
                        ((db - db_floor) / -db_floor).clamp(0.0, 1.0)
                    };

                    let bar_h = scaled_amp * (wave_rect.height() - 4.0);
                    let is_cut = app.is_frame_cut(frame_idx as usize);
                    let bar_col = if is_cut {
                        palette.cut_color.linear_multiply(0.4)
                    } else {
                        palette.accent
                    };

                    wave_painter.line_segment(
                        [pos2(x + 0.5, wave_rect.bottom() - 2.0), pos2(x + 0.5, wave_rect.bottom() - 2.0 - bar_h)],
                        Stroke::new(1.0_f32, bar_col),
                    );
                }
            }
        }
    }

    // 4. Draw Editorial Cut / Keep Bands across Filmstrip & Waveform
    let total_timeline_rect = Rect::from_min_max(
        pos2(filmstrip_rect.left(), filmstrip_rect.top()),
        pos2(wave_rect.right(), wave_rect.bottom()),
    );

    for range in &app.ranges {
        let r_start_sec = range.in_frame as f64 / fps;
        let r_end_sec = range.out_frame as f64 / fps;

        let start_x = (filmstrip_rect.left() as f64 + scroll_offset_x + r_start_sec * pps) as f32;
        let end_x = (filmstrip_rect.left() as f64 + scroll_offset_x + r_end_sec * pps) as f32;

        let left = start_x.max(total_timeline_rect.left());
        let right = end_x.min(total_timeline_rect.right());

        if left < right {
            let band_rect = Rect::from_min_max(pos2(left, total_timeline_rect.top()), pos2(right, total_timeline_rect.bottom()));
            let band_fill = if app.cut_mode {
                palette.timeline_snippet_cut_bg
            } else {
                palette.timeline_snippet_keep_bg
            };
            ui.painter().rect_filled(band_rect, 0.0, band_fill);

            // Subtle top and bottom border
            let edge_color = if app.cut_mode { palette.cut_color } else { palette.keep_color };
            ui.painter().line_segment([pos2(left, band_rect.top()), pos2(right, band_rect.top())], Stroke::new(1.5_f32, edge_color));
            ui.painter().line_segment([pos2(left, band_rect.bottom()), pos2(right, band_rect.bottom())], Stroke::new(1.5_f32, edge_color));
        }
    }

    // 5. In / Out Bracket Markers
    let draw_marker = |p: &egui::Painter, frame: usize, color: Color32, dir: f32| {
        let sec = frame as f64 / fps;
        let x = (filmstrip_rect.left() as f64 + scroll_offset_x + sec * pps) as f32;
        if x >= filmstrip_rect.left() && x <= filmstrip_rect.right() {
            // Vertical hairline
            p.line_segment(
                [pos2(x, filmstrip_rect.top()), pos2(x, wave_rect.bottom())],
                Stroke::new(1.8_f32, color),
            );
            // Top bracket cap
            p.line_segment(
                [pos2(x, filmstrip_rect.top()), pos2(x + dir * 8.0, filmstrip_rect.top())],
                Stroke::new(1.8_f32, color),
            );
            p.line_segment(
                [pos2(x + dir * 8.0, filmstrip_rect.top()), pos2(x + dir * 8.0, filmstrip_rect.top() + 6.0)],
                Stroke::new(1.8_f32, color),
            );
            // Bottom bracket cap
            p.line_segment(
                [pos2(x, wave_rect.bottom()), pos2(x + dir * 8.0, wave_rect.bottom())],
                Stroke::new(1.8_f32, color),
            );
            p.line_segment(
                [pos2(x + dir * 8.0, wave_rect.bottom()), pos2(x + dir * 8.0, wave_rect.bottom() - 6.0)],
                Stroke::new(1.8_f32, color),
            );
        }
    };

    if let Some(inf) = app.in_marker {
        draw_marker(&ui.painter(), inf, palette.warn_color, 1.0);
    }
    if let Some(outf) = app.out_marker {
        draw_marker(&ui.painter(), outf, palette.accent, -1.0);
    }

    // 6. Commercial Breaks Layer (When in Label Mode)
    if let Some(session) = &mut app.label_session {
        let mut new_start = None;
        let mut new_end = None;

        for (idx, brk) in session.breaks.iter().enumerate() {
            let b_start_sec = brk.start_cut_frame as f64 / fps;
            let b_end_sec = brk.end_cut_frame as f64 / fps;

            let start_x = (filmstrip_rect.left() as f64 + scroll_offset_x + b_start_sec * pps) as f32;
            let end_x = (filmstrip_rect.left() as f64 + scroll_offset_x + b_end_sec * pps) as f32;

            let left = start_x.max(total_timeline_rect.left());
            let right = end_x.min(total_timeline_rect.right());

            if left < right {
                let is_sel = session.selected_break == Some(idx);
                let (fill, stroke_col) = match brk.break_type {
                    snippy_schema::BreakType::Commercial => {
                        (Color32::from_rgba_unmultiplied(230, 100, 0, if is_sel { 65 } else { 35 }), palette.warn_color)
                    }
                    snippy_schema::BreakType::Promo => {
                        (Color32::from_rgba_unmultiplied(140, 50, 230, if is_sel { 65 } else { 35 }), palette.secondary)
                    }
                };

                let band_rect = Rect::from_min_max(pos2(left, total_timeline_rect.top()), pos2(right, total_timeline_rect.bottom()));
                ui.painter().rect_filled(band_rect, 0.0, fill);
                if is_sel {
                    ui.painter().rect_stroke(band_rect, 0.0, Stroke::new(1.5_f32, stroke_col), StrokeKind::Inside);
                }

                let band_resp = ui.interact(band_rect, ui.make_persistent_id(format!("brk_{}", idx)), egui::Sense::click());
                if band_resp.clicked() {
                    session.selected_break = Some(idx);
                }
            }

            // Interactive Start Handle
            if start_x >= filmstrip_rect.left() && start_x <= filmstrip_rect.right() {
                let knob_pos = pos2(start_x, filmstrip_rect.top() + 6.0);
                ui.painter().circle_filled(knob_pos, 4.5, palette.warn_color);
                let handle_resp = ui.interact(Rect::from_center_size(knob_pos, vec2(14.0, 14.0)), ui.make_persistent_id(format!("h_s_{}", idx)), egui::Sense::drag());
                if handle_resp.dragged() {
                    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                        let new_sec = ((pos.x - filmstrip_rect.left()) as f64 - scroll_offset_x) / pps;
                        new_start = Some((idx, (new_sec * fps).round().max(0.0) as u64));
                    }
                }
            }

            // Interactive End Handle
            if end_x >= filmstrip_rect.left() && end_x <= filmstrip_rect.right() {
                let knob_pos = pos2(end_x, filmstrip_rect.top() + 6.0);
                ui.painter().circle_filled(knob_pos, 4.5, palette.warn_color);
                let handle_resp = ui.interact(Rect::from_center_size(knob_pos, vec2(14.0, 14.0)), ui.make_persistent_id(format!("h_e_{}", idx)), egui::Sense::drag());
                if handle_resp.dragged() {
                    if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                        let new_sec = ((pos.x - filmstrip_rect.left()) as f64 - scroll_offset_x) / pps;
                        new_end = Some((idx, (new_sec * fps).round().max(0.0) as u64));
                    }
                }
            }
        }

        if let Some((idx, frame)) = new_start {
            session.set_start_frame(idx, frame);
        }
        if let Some((idx, frame)) = new_end {
            session.set_end_frame(idx, frame);
        }
    }

    // 7. Master Playhead Needle
    let px = playhead_x;
    if px >= ruler_rect.left() && px <= ruler_rect.right() {
        // Triangular playhead cap in ruler
        let p_top = ruler_rect.top();
        let cap_points = vec![
            pos2(px - 6.0, p_top),
            pos2(px + 6.0, p_top),
            pos2(px + 6.0, p_top + 10.0),
            pos2(px, p_top + 16.0),
            pos2(px - 6.0, p_top + 10.0),
        ];
        ui.painter().add(Shape::convex_polygon(cap_points, palette.playhead_color, Stroke::NONE));

        // Hairline line traversing all tracks down to wave_rect.bottom()
        ui.painter().line_segment(
            [pos2(px, p_top + 16.0), pos2(px, wave_rect.bottom())],
            Stroke::new(1.5_f32, palette.playhead_color),
        );
    }

    // 7.5. macOS Dock-Style Magnification Loupe on Hover
    if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
        let inside_tracks = ruler_rect.contains(mouse_pos)
            || filmstrip_rect.contains(mouse_pos)
            || wave_rect.contains(mouse_pos);

        if inside_tracks && !ui.input(|i| i.pointer.any_down()) {
            let hover_sec = (-scroll_offset_x + (mouse_pos.x - filmstrip_rect.left()) as f64) / pps;
            if hover_sec >= 0.0 && hover_sec <= total_duration {
                let hover_frame = (hover_sec * fps).round().clamp(0.0, (total_frames.saturating_sub(1)) as f64) as usize;

                // Vertical hairline indicator tracing from ruler down through waveform
                ui.painter().line_segment(
                    [pos2(mouse_pos.x, ruler_rect.top()), pos2(mouse_pos.x, wave_rect.bottom())],
                    Stroke::new(1.0_f32, palette.accent.linear_multiply(0.8)),
                );

                // Dock Magnification Loupe Card
                let card_w = 110.0_f32;
                let card_h = 72.0_f32;
                let card_x = (mouse_pos.x - card_w * 0.5).clamp(ruler_rect.left() + 4.0, ruler_rect.right() - card_w - 4.0);
                let card_y = (ruler_rect.top() - card_h - 10.0).max(4.0);
                let card_rect = Rect::from_min_size(pos2(card_x, card_y), vec2(card_w, card_h));

                let p = ui.painter();
                p.rect_filled(card_rect, 4.0, Color32::from_black_alpha(240));
                p.rect_stroke(card_rect, 4.0, Stroke::new(1.0_f32, palette.accent), StrokeKind::Inside);

                // Thumbnail image inside card
                let img_rect = Rect::from_min_size(pos2(card_rect.left() + 6.0, card_rect.top() + 6.0), vec2(card_w - 12.0, 44.0));
                if let Some(texture) = app.thumbnail_cache.get(&hover_frame) {
                    p.image(
                        texture.id(),
                        img_rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    p.rect_filled(img_rect, 2.0, palette.widget_inactive_bg);
                    if !app.in_flight_requests.contains_key(&hover_frame) {
                        if let Some(tx) = &app.playback.thumbnail_tx {
                            let _ = tx.send(crate::media::ThumbnailCommand::Request {
                                visible_frames: vec![hover_frame],
                                padding_frames: vec![],
                                height: 50,
                            });
                            app.in_flight_requests.insert(hover_frame, std::time::Instant::now());
                        }
                    }
                }

                // Timecode & Frame text in card
                let tc_label = format!("{} (F{})", app.format_seconds(hover_sec), hover_frame);
                p.text(
                    pos2(card_rect.center().x, card_rect.bottom() - 10.0),
                    egui::Align2::CENTER_CENTER,
                    tc_label,
                    egui::FontId::monospace(9.0),
                    Color32::WHITE,
                );
            }
        }
    }

    ui.add_space(6.0);

    // 8. Timeline Footer Bar (Playhead Mode, Time Status, Zoom Controls)
    render_timeline_footer(app, ui, &palette);
}

fn render_timeline_footer(app: &mut MainApp, ui: &mut egui::Ui, palette: &crate::ui::ThemePalette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);

        // Frame status
        let cur_f = app.playback.current_frame();
        let tot_f = app.playback.total_frames();
        ui.label(
            RichText::new(format!("Frame {} / {}", cur_f, tot_f))
                .size(11.0)
                .monospace()
                .color(palette.text_muted)
        );

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Playhead Mode Toggle Button
        let current_mode = app.settings.playback.playhead_mode;
        let mode_label = match current_mode {
            PlayheadMode::CenterAnchored => "🔒 Center-Locked",
            PlayheadMode::FreeTravelling => "↔ Free-Moving",
        };
        let mode_btn = egui::Button::new(
            RichText::new(mode_label)
                .size(11.0)
                .color(palette.text_normal)
        )
        .fill(palette.widget_inactive_bg)
        .stroke(Stroke::new(1.0_f32, palette.border_color))
        .corner_radius(2.0);

        if ui.add(mode_btn).on_hover_text("Toggle timeline playhead mode: Center-locked scrolling vs Free-moving cursor").clicked() {
            let next = match current_mode {
                PlayheadMode::CenterAnchored => PlayheadMode::FreeTravelling,
                PlayheadMode::FreeTravelling => PlayheadMode::CenterAnchored,
            };
            app.settings.playback.playhead_mode = next;
            app.pending_settings.playback.playhead_mode = next;
        }

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        // Pin Timeline Toggle Button (keeps expanded vs auto-expand on hover)
        let pin_text = if app.timeline_pinned_expanded { "📌 Pinned Open" } else { "📌 Auto-Expand" };
        let pin_col = if app.timeline_pinned_expanded { palette.accent } else { palette.text_muted };
        let pin_btn = egui::Button::new(
            RichText::new(pin_text)
                .size(11.0)
                .color(pin_col)
        )
        .fill(if app.timeline_pinned_expanded { palette.widget_hovered_bg } else { palette.widget_inactive_bg })
        .stroke(Stroke::new(1.0_f32, palette.border_color))
        .corner_radius(2.0);

        if ui.add(pin_btn).on_hover_text("Toggle whether timeline stays pinned open or auto-expands on hover (like macOS dock)").clicked() {
            app.timeline_pinned_expanded = !app.timeline_pinned_expanded;
        }

        // Right-aligned Zoom Controls
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 0.0);

            // Fit to window button
            if ui.small_button("Fit").on_hover_text("Fit entire video to window (Ctrl+0)").clicked() {
                app.zoom_level = 0.0;
            }

            // Zoom In button (+)
            if ui.small_button("+").on_hover_text("Zoom in filmstrip").clicked() {
                app.zoom_level = (app.zoom_level + 0.1).clamp(0.0, 1.0);
            }

            // Zoom Slider
            ui.add(
                egui::Slider::new(&mut app.zoom_level, 0.0..=1.0)
                    .show_value(false)
                    .text("Zoom")
            );

            // Zoom Out button (-)
            if ui.small_button("−").on_hover_text("Zoom out filmstrip").clicked() {
                app.zoom_level = (app.zoom_level - 0.1).clamp(0.0, 1.0);
            }
        });
    });
}

fn render_slim_timeline_overview(
    app: &mut MainApp,
    ui: &mut egui::Ui,
    palette: &crate::ui::ThemePalette,
    total_width: f32,
    current_frame: usize,
    total_frames: usize,
    fps: f64,
) {
    let dur_sec = if fps > 0.0 { total_frames as f64 / fps } else { 0.0 };
    let cur_sec = if fps > 0.0 { current_frame as f64 / fps } else { 0.0 };

    ui.vertical(|ui| {
        // Top micro-bar: Timecode and Quick Stats
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 0.0);

            // Scissor Icon
            let (icon_r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
            icons::draw_icon(ui.painter(), icon_r, IconType::Scissor, palette.accent);

            // Timecode Readout
            let cur_str = app.format_seconds(cur_sec);
            let tot_str = app.format_seconds(dur_sec);
            ui.label(RichText::new(format!("{} / {}", cur_str, tot_str)).monospace().size(11.0).color(Color32::WHITE));

            // Snippets Count
            let count_str = format!("{} Snippets", app.ranges.len());
            ui.label(RichText::new(count_str).size(10.0).color(palette.text_muted));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Pin toggle button
                let pin_text = if app.timeline_pinned_expanded { "📌 Pinned" } else { "📌 Auto-Expand" };
                let pin_col = if app.timeline_pinned_expanded { palette.accent } else { palette.text_muted };
                if ui.add(egui::Button::new(RichText::new(pin_text).size(10.0).color(pin_col)).frame(false)).on_hover_text("Pin timeline permanently open").clicked() {
                    app.timeline_pinned_expanded = !app.timeline_pinned_expanded;
                }

                ui.label(RichText::new("Hover to expand").size(10.0).color(palette.text_muted));
            });
        });

        ui.add_space(2.0);

        // Full-width Scrubbable Mini-map Track
        let track_h = 16.0_f32;
        let (track_rect, track_response) = ui.allocate_exact_size(
            vec2(total_width, track_h),
            egui::Sense::click_and_drag(),
        );
        let p = ui.painter_at(track_rect);

        // Track Background
        p.rect_filled(track_rect, 2.0, palette.timeline_track_bg);
        p.rect_stroke(track_rect, 2.0, Stroke::new(1.0_f32, palette.border_color), StrokeKind::Inside);

        // Paint Cut / Keep segments across full width
        let (cut_col, keep_col) = (palette.cut_color, palette.keep_color);
        if total_frames > 0 {
            for r in &app.ranges {
                let start_pct = (r.in_frame as f32 / total_frames as f32).clamp(0.0, 1.0);
                let end_pct = (r.out_frame as f32 / total_frames as f32).clamp(0.0, 1.0);
                let x0 = track_rect.left() + start_pct * track_rect.width();
                let x1 = track_rect.left() + end_pct * track_rect.width();
                let fill = if app.cut_mode { cut_col } else { keep_col };
                let seg_rect = Rect::from_min_max(pos2(x0, track_rect.top()), pos2(x1, track_rect.bottom()));
                p.rect_filled(seg_rect, 1.0, fill);
            }
        }

        // Defect Warning Markers on Mini-map Track
        if app.settings.timeline.show_timeline_defect_markers && dur_sec > 0.0 {
            let defect_ranges = app.playback.defect_map().read().map_or(vec![], |m| m.ranges().to_vec());
            for range in &defect_ranges {
                let x0 = track_rect.left() + (range.start_pts_sec / dur_sec).clamp(0.0, 1.0) as f32 * track_rect.width();
                let x1 = (track_rect.left() + (range.end_pts_sec / dur_sec).clamp(0.0, 1.0) as f32 * track_rect.width()).max(x0 + 2.0);
                let defect_rect = Rect::from_min_max(pos2(x0, track_rect.top()), pos2(x1, track_rect.top() + 3.0));
                p.rect_filled(defect_rect, 0.0, Color32::from_rgba_unmultiplied(230, 160, 30, 180));
            }
        }

        // Paint In / Out markers
        if let Some(inf) = app.in_marker {
            let in_pct = (inf as f32 / total_frames as f32).clamp(0.0, 1.0);
            let in_x = track_rect.left() + in_pct * track_rect.width();
            p.line_segment([pos2(in_x, track_rect.top()), pos2(in_x, track_rect.bottom())], Stroke::new(2.0_f32, palette.keep_color));
        }
        if let Some(outf) = app.out_marker {
            let out_pct = (outf as f32 / total_frames as f32).clamp(0.0, 1.0);
            let out_x = track_rect.left() + out_pct * track_rect.width();
            p.line_segment([pos2(out_x, track_rect.top()), pos2(out_x, track_rect.bottom())], Stroke::new(2.0_f32, palette.cut_color));
        }

        // Playhead needle
        if total_frames > 0 {
            let cur_pct = (current_frame as f32 / total_frames as f32).clamp(0.0, 1.0);
            let cur_x = track_rect.left() + cur_pct * track_rect.width();
            p.line_segment([pos2(cur_x, track_rect.top() - 2.0), pos2(cur_x, track_rect.bottom() + 2.0)], Stroke::new(2.0_f32, palette.playhead_color));
        }

        // Scrubbing on Mini-map
        if track_response.clicked() || track_response.dragged() {
            app.playback.set_playing(false);
            if let Some(pos) = track_response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos())) {
                let pct = ((pos.x - track_rect.left()) / track_rect.width()).clamp(0.0, 1.0);
                let target_frame = (pct * (total_frames.saturating_sub(1)) as f32).round() as usize;
                app.playback.seek_internal(target_frame, false, true);
            }
        }
    });
}
