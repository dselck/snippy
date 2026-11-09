use egui::{Color32, Painter, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

/// Draw Play icon (centered equilateral triangle pointing right)
pub fn draw_play(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.height() * 0.38).min(r.width() * 0.38);
    let p1 = pos2(c.x - s * 0.65, c.y - s);
    let p2 = pos2(c.x - s * 0.65, c.y + s);
    let p3 = pos2(c.x + s * 0.95, c.y);
    p.add(Shape::convex_polygon(vec![p1, p2, p3], color, Stroke::NONE));
}

/// Draw Pause icon (two vertical bars)
pub fn draw_pause(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let bar_w = (r.width() * 0.16).max(2.0);
    let bar_h = r.height() * 0.68;
    let gap = (r.width() * 0.15).max(3.0);
    
    let left_bar = Rect::from_center_size(pos2(c.x - gap, c.y), vec2(bar_w, bar_h));
    let right_bar = Rect::from_center_size(pos2(c.x + gap, c.y), vec2(bar_w, bar_h));
    p.rect_filled(left_bar, 1.0, color);
    p.rect_filled(right_bar, 1.0, color);
}

/// Draw Step Backward icon (vertical line + left triangle)
pub fn draw_step_backward(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.height() * 0.32).min(r.width() * 0.32);
    let line_x = c.x - s * 0.8;
    p.line_segment(
        [pos2(line_x, c.y - s), pos2(line_x, c.y + s)],
        Stroke::new(1.8_f32, color),
    );
    let p1 = pos2(c.x + s * 0.8, c.y - s * 0.85);
    let p2 = pos2(c.x + s * 0.8, c.y + s * 0.85);
    let p3 = pos2(c.x - s * 0.3, c.y);
    p.add(Shape::convex_polygon(vec![p1, p2, p3], color, Stroke::NONE));
}

/// Draw Step Forward icon (right triangle + vertical line)
pub fn draw_step_forward(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.height() * 0.32).min(r.width() * 0.32);
    let line_x = c.x + s * 0.8;
    p.line_segment(
        [pos2(line_x, c.y - s), pos2(line_x, c.y + s)],
        Stroke::new(1.8_f32, color),
    );
    let p1 = pos2(c.x - s * 0.8, c.y - s * 0.85);
    let p2 = pos2(c.x - s * 0.8, c.y + s * 0.85);
    let p3 = pos2(c.x + s * 0.3, c.y);
    p.add(Shape::convex_polygon(vec![p1, p2, p3], color, Stroke::NONE));
}

/// Draw Jump to Start icon (|<<)
pub fn draw_jump_start(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.height() * 0.32).min(r.width() * 0.32);
    let line_x = c.x - s * 1.1;
    p.line_segment(
        [pos2(line_x, c.y - s), pos2(line_x, c.y + s)],
        Stroke::new(2.0_f32, color),
    );
    // Left double chevron
    let t1_1 = pos2(c.x - s * 0.1, c.y - s * 0.8);
    let t1_2 = pos2(c.x - s * 0.1, c.y + s * 0.8);
    let t1_3 = pos2(c.x - s * 0.9, c.y);
    p.add(Shape::convex_polygon(vec![t1_1, t1_2, t1_3], color, Stroke::NONE));

    let t2_1 = pos2(c.x + s * 0.8, c.y - s * 0.8);
    let t2_2 = pos2(c.x + s * 0.8, c.y + s * 0.8);
    let t2_3 = pos2(c.x, c.y);
    p.add(Shape::convex_polygon(vec![t2_1, t2_2, t2_3], color, Stroke::NONE));
}

/// Draw Jump to End icon (>>|)
pub fn draw_jump_end(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.height() * 0.32).min(r.width() * 0.32);
    let line_x = c.x + s * 1.1;
    p.line_segment(
        [pos2(line_x, c.y - s), pos2(line_x, c.y + s)],
        Stroke::new(2.0_f32, color),
    );
    // Right double chevron
    let t1_1 = pos2(c.x + s * 0.1, c.y - s * 0.8);
    let t1_2 = pos2(c.x + s * 0.1, c.y + s * 0.8);
    let t1_3 = pos2(c.x + s * 0.9, c.y);
    p.add(Shape::convex_polygon(vec![t1_1, t1_2, t1_3], color, Stroke::NONE));

    let t2_1 = pos2(c.x - s * 0.8, c.y - s * 0.8);
    let t2_2 = pos2(c.x - s * 0.8, c.y + s * 0.8);
    let t2_3 = pos2(c.x, c.y);
    p.add(Shape::convex_polygon(vec![t2_1, t2_2, t2_3], color, Stroke::NONE));
}

/// Draw Mark In bracket icon ([)
pub fn draw_mark_in(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let w = r.width() * 0.28;
    let h = r.height() * 0.40;
    let stroke = Stroke::new(2.0_f32, color);
    
    let left = c.x - w * 0.5;
    let right = c.x + w * 0.5;
    let top = c.y - h;
    let bottom = c.y + h;

    p.line_segment([pos2(right, top), pos2(left, top)], stroke);
    p.line_segment([pos2(left, top), pos2(left, bottom)], stroke);
    p.line_segment([pos2(left, bottom), pos2(right, bottom)], stroke);
}

/// Draw Mark Out bracket icon (])
pub fn draw_mark_out(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let w = r.width() * 0.28;
    let h = r.height() * 0.40;
    let stroke = Stroke::new(2.0_f32, color);
    
    let left = c.x - w * 0.5;
    let right = c.x + w * 0.5;
    let top = c.y - h;
    let bottom = c.y + h;

    p.line_segment([pos2(left, top), pos2(right, top)], stroke);
    p.line_segment([pos2(right, top), pos2(right, bottom)], stroke);
    p.line_segment([pos2(right, bottom), pos2(left, bottom)], stroke);
}

/// Draw Scissor / Razor Cut icon
pub fn draw_scissor(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.35).min(r.height() * 0.35);
    let stroke = Stroke::new(1.6_f32, color);

    // Blades crossing through pivot
    p.line_segment([pos2(c.x - s * 0.7, c.y - s * 0.7), pos2(c.x + s * 0.9, c.y + s * 0.7)], stroke);
    p.line_segment([pos2(c.x - s * 0.7, c.y + s * 0.7), pos2(c.x + s * 0.9, c.y - s * 0.7)], stroke);

    // Pivot dot
    p.circle_filled(c, 1.8, color);

    // Finger rings
    let ring_r = s * 0.35;
    p.circle_stroke(pos2(c.x - s * 0.9, c.y - s * 0.8), ring_r, Stroke::new(1.4_f32, color));
    p.circle_stroke(pos2(c.x - s * 0.9, c.y + s * 0.8), ring_r, Stroke::new(1.4_f32, color));
}

/// Draw Trash Bin icon
pub fn draw_trash(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.35).min(r.height() * 0.35);
    let stroke = Stroke::new(1.4_f32, color);

    // Lid handle
    p.line_segment([pos2(c.x - s * 0.25, c.y - s * 0.9), pos2(c.x + s * 0.25, c.y - s * 0.9)], stroke);
    // Lid rim
    p.line_segment([pos2(c.x - s * 0.85, c.y - s * 0.65), pos2(c.x + s * 0.85, c.y - s * 0.65)], stroke);

    // Bin bucket (tapered)
    let p_tl = pos2(c.x - s * 0.65, c.y - s * 0.55);
    let p_tr = pos2(c.x + s * 0.65, c.y - s * 0.55);
    let p_br = pos2(c.x + s * 0.50, c.y + s * 0.9);
    let p_bl = pos2(c.x - s * 0.50, c.y + s * 0.9);

    p.line_segment([p_tl, p_bl], stroke);
    p.line_segment([p_bl, p_br], stroke);
    p.line_segment([p_br, p_tr], stroke);

    // Inner slats
    p.line_segment([pos2(c.x - s * 0.2, c.y - s * 0.4), pos2(c.x - s * 0.15, c.y + s * 0.75)], stroke);
    p.line_segment([pos2(c.x + s * 0.2, c.y - s * 0.4), pos2(c.x + s * 0.15, c.y + s * 0.75)], stroke);
}

/// Draw Gear / Settings icon
pub fn draw_settings(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let outer_r = (r.width() * 0.38).min(r.height() * 0.38);
    let inner_r = outer_r * 0.65;
    let tooth_w = 2.0_f32;

    // Center hole
    p.circle_stroke(c, outer_r * 0.35, Stroke::new(1.6_f32, color));

    // Outer gear teeth
    for i in 0..6 {
        let angle = i as f32 * std::f32::consts::PI / 3.0;
        let dir = vec2(angle.cos(), angle.sin());
        p.line_segment(
            [pos2(c.x + dir.x * inner_r, c.y + dir.y * inner_r), pos2(c.x + dir.x * outer_r, c.y + dir.y * outer_r)],
            Stroke::new(tooth_w, color),
        );
    }
    p.circle_stroke(c, inner_r, Stroke::new(1.5_f32, color));
}

/// Draw Open Folder icon
pub fn draw_folder(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let w = r.width() * 0.42;
    let h = r.height() * 0.30;
    let stroke = Stroke::new(1.4_f32, color);

    let left = c.x - w;
    let right = c.x + w;
    let top = c.y - h;
    let bottom = c.y + h;
    let tab_w = w * 0.6;

    // Folder tab
    p.line_segment([pos2(left, top), pos2(left + tab_w, top)], stroke);
    p.line_segment([pos2(left + tab_w, top), pos2(left + tab_w + 3.0, top + 4.0)], stroke);
    p.line_segment([pos2(left + tab_w + 3.0, top + 4.0), pos2(right, top + 4.0)], stroke);

    // Main box
    p.line_segment([pos2(right, top + 4.0), pos2(right, bottom)], stroke);
    p.line_segment([pos2(right, bottom), pos2(left, bottom)], stroke);
    p.line_segment([pos2(left, bottom), pos2(left, top)], stroke);
}

/// Draw Export icon (tray with arrow pointing up-right or right)
pub fn draw_export(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.36).min(r.height() * 0.36);
    let stroke = Stroke::new(1.6_f32, color);

    // Lower tray
    let left = c.x - s;
    let right = c.x + s;
    let bottom = c.y + s * 0.8;
    let tray_top = c.y + s * 0.2;

    p.line_segment([pos2(left, tray_top), pos2(left, bottom)], stroke);
    p.line_segment([pos2(left, bottom), pos2(right, bottom)], stroke);
    p.line_segment([pos2(right, bottom), pos2(right, tray_top)], stroke);

    // Arrow pointing up
    let arrow_top = c.y - s * 0.9;
    let arrow_base = c.y + s * 0.2;
    p.line_segment([pos2(c.x, arrow_base), pos2(c.x, arrow_top)], stroke);
    p.line_segment([pos2(c.x - s * 0.45, arrow_top + s * 0.45), pos2(c.x, arrow_top)], stroke);
    p.line_segment([pos2(c.x + s * 0.45, arrow_top + s * 0.45), pos2(c.x, arrow_top)], stroke);
}

/// Draw Save Floppy icon
pub fn draw_save(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.36).min(r.height() * 0.36);
    let stroke = Stroke::new(1.4_f32, color);

    let left = c.x - s;
    let right = c.x + s;
    let top = c.y - s;
    let bottom = c.y + s;

    // Body with top-right chamfer
    let chamfer = s * 0.4;
    p.line_segment([pos2(left, top), pos2(right - chamfer, top)], stroke);
    p.line_segment([pos2(right - chamfer, top), pos2(right, top + chamfer)], stroke);
    p.line_segment([pos2(right, top + chamfer), pos2(right, bottom)], stroke);
    p.line_segment([pos2(right, bottom), pos2(left, bottom)], stroke);
    p.line_segment([pos2(left, bottom), pos2(left, top)], stroke);

    // Shutter rectangle
    let shutter = Rect::from_min_max(pos2(left + s * 0.35, top), pos2(right - s * 0.5, top + s * 0.7));
    p.rect_stroke(shutter, 0.0, stroke, StrokeKind::Inside);
}

/// Draw Tag / Label icon
pub fn draw_tag(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.36).min(r.height() * 0.36);
    let stroke = Stroke::new(1.4_f32, color);

    let p1 = pos2(c.x - s * 0.8, c.y - s * 0.4);
    let p2 = pos2(c.x, c.y - s * 0.8);
    let p3 = pos2(c.x + s * 0.8, c.y);
    let p4 = pos2(c.x, c.y + s * 0.8);
    let p5 = pos2(c.x - s * 0.8, c.y + s * 0.4);

    p.line_segment([p1, p2], stroke);
    p.line_segment([p2, p3], stroke);
    p.line_segment([p3, p4], stroke);
    p.line_segment([p4, p5], stroke);
    p.line_segment([p5, p1], stroke);

    // Tag eyelet
    p.circle_filled(pos2(c.x - s * 0.3, c.y), 1.5, color);
}

/// Draw Close X icon
pub fn draw_close(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.28).min(r.height() * 0.28);
    let stroke = Stroke::new(1.6_f32, color);
    p.line_segment([pos2(c.x - s, c.y - s), pos2(c.x + s, c.y + s)], stroke);
    p.line_segment([pos2(c.x + s, c.y - s), pos2(c.x - s, c.y + s)], stroke);
}

/// Draw Checkmark icon
pub fn draw_check(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.32).min(r.height() * 0.32);
    let stroke = Stroke::new(1.8_f32, color);
    p.line_segment([pos2(c.x - s * 0.8, c.y), pos2(c.x - s * 0.2, c.y + s * 0.7)], stroke);
    p.line_segment([pos2(c.x - s * 0.2, c.y + s * 0.7), pos2(c.x + s * 0.8, c.y - s * 0.7)], stroke);
}

/// Draw Add / Plus icon
pub fn draw_plus(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let s = (r.width() * 0.32).min(r.height() * 0.32);
    let stroke = Stroke::new(1.8_f32, color);
    p.line_segment([pos2(c.x - s, c.y), pos2(c.x + s, c.y)], stroke);
    p.line_segment([pos2(c.x, c.y - s), pos2(c.x, c.y + s)], stroke);
}

/// Draw Audio Waveform symbol
pub fn draw_waveform(p: &Painter, r: Rect, color: Color32) {
    let c = r.center();
    let heights = [0.3, 0.7, 0.9, 0.5, 0.8, 0.4];
    let count = heights.len();
    let spacing = r.width() * 0.65 / count as f32;
    let start_x = c.x - (count as f32 * 0.5 * spacing);

    for (i, h_factor) in heights.iter().enumerate() {
        let x = start_x + (i as f32 + 0.5) * spacing;
        let bar_h = r.height() * 0.6 * h_factor;
        p.line_segment(
            [pos2(x, c.y - bar_h * 0.5), pos2(x, c.y + bar_h * 0.5)],
            Stroke::new(1.8_f32, color),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IconType {
    Play,
    Pause,
    StepBackward,
    StepForward,
    JumpStart,
    JumpEnd,
    MarkIn,
    MarkOut,
    Scissor,
    Trash,
    Settings,
    Folder,
    Export,
    Save,
    Tag,
    Close,
    Check,
    Plus,
    Waveform,
}

pub fn draw_icon(p: &Painter, r: Rect, icon: IconType, color: Color32) {
    match icon {
        IconType::Play => draw_play(p, r, color),
        IconType::Pause => draw_pause(p, r, color),
        IconType::StepBackward => draw_step_backward(p, r, color),
        IconType::StepForward => draw_step_forward(p, r, color),
        IconType::JumpStart => draw_jump_start(p, r, color),
        IconType::JumpEnd => draw_jump_end(p, r, color),
        IconType::MarkIn => draw_mark_in(p, r, color),
        IconType::MarkOut => draw_mark_out(p, r, color),
        IconType::Scissor => draw_scissor(p, r, color),
        IconType::Trash => draw_trash(p, r, color),
        IconType::Settings => draw_settings(p, r, color),
        IconType::Folder => draw_folder(p, r, color),
        IconType::Export => draw_export(p, r, color),
        IconType::Save => draw_save(p, r, color),
        IconType::Tag => draw_tag(p, r, color),
        IconType::Close => draw_close(p, r, color),
        IconType::Check => draw_check(p, r, color),
        IconType::Plus => draw_plus(p, r, color),
        IconType::Waveform => draw_waveform(p, r, color),
    }
}

pub fn icon_button(ui: &mut egui::Ui, icon: IconType, size: egui::Vec2, active: bool, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let fill = if active {
            ui.visuals().selection.bg_fill
        } else if response.hovered() {
            visuals.bg_fill
        } else {
            Color32::TRANSPARENT
        };
        let icon_color = if active {
            Color32::WHITE
        } else if response.hovered() {
            visuals.fg_stroke.color
        } else {
            ui.visuals().text_color()
        };
        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, 3.0, fill);
        }
        draw_icon(ui.painter(), rect.shrink(3.0), icon, icon_color);
    }
    response.on_hover_text(tooltip)
}
