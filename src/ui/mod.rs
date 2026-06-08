pub mod yuv_renderer;

use crate::media;
use crossbeam_channel::Receiver;
use ringbuf::traits::Split;
use eframe::egui;
use eframe::egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Seeking,
    Paused,
    Playing,
}

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
    pub current_frame: usize,
    pub total_frames: usize,
    pub is_playing: bool,
    pub video_path: Option<String>,
    pub video_fps: f64,

    // Range Selection Markers
    pub in_marker: Option<usize>,
    pub out_marker: Option<usize>,
    pub ranges: Vec<ExportRange>,
    pub next_range_id: usize,

    // Snippets cut/keep toggle
    pub cut_mode: bool,

    // Channels & Playback
    pub frame_rx: Option<Receiver<media::DecodedFrame>>,
    pub latest_frame: Option<media::DecodedFrame>,

    // Mock Audio Loudness Profile (for visual layout)
    pub loudness_profile: Vec<f32>,

    // Filmstrip Zoom Level (0.0 = fully zoomed out, 1.0 = fully zoomed in)
    pub zoom_level: f32,

    // Height of the resizable bottom timeline deck panel
    pub timeline_height: f32,

    pub _audio_engine: Option<crate::audio::AudioEngine>,
    pub playback_state: std::sync::Arc<crate::audio::AudioPlaybackState>,
    pub media_engine: Option<crate::media::MediaEngine>,
    pub keyframes: Vec<usize>,
    pub future_frame: Option<media::DecodedFrame>,
    pub latest_sequence_id: usize,
    pub show_debug_hud: bool,
    pub drift_history: std::collections::VecDeque<f32>,
    pub stale_discards_count: usize,
    pub drift_pause_count: usize,
    pub seek_in_progress: bool,
    pub ui_state: PlaybackState,
    pub last_seek_time: Option<std::time::Instant>,
    pub target_seek_frame: Option<usize>,
}

impl MainApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_fonts(&cc.egui_ctx);
        configure_theme(&cc.egui_ctx);

        if let Some(wgpu_state) = &cc.wgpu_render_state {
            let mut renderer = wgpu_state.renderer.write();
            renderer.callback_resources.insert(yuv_renderer::YuvRenderer::new(
                &wgpu_state.device,
                wgpu_state.target_format,
            ));
        }

        // Generate a mock loudness profile to show a beautiful timeline waveform
        let mut loudness = Vec::with_capacity(300);
        for i in 0..300 {
            let sine = (i as f32 * 0.15).sin().abs();
            let noise = (i as f32 * 0.45).cos().abs() * 0.3;
            let val = (sine * 0.7 + noise * 0.3) * 0.8;
            loudness.push(val.clamp(0.05, 1.0));
        }

        let audio_engine = crate::audio::AudioEngine::new().ok();
        let playback_state = audio_engine.as_ref().map(|ae| std::sync::Arc::clone(&ae.playback_state))
            .unwrap_or_else(|| std::sync::Arc::new(crate::audio::AudioPlaybackState {
                current_position_us: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                sample_rate: 44100,
                has_audio: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                fallback_start_time: std::sync::Arc::new(std::sync::Mutex::new(None)),
                fallback_base_time: std::sync::Arc::new(std::sync::Mutex::new(0.0)),
                should_flush: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                is_playing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                audio_paused_for_sync: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }));

        Self {
            current_frame: 180,
            total_frames: 1000,
            is_playing: false,
            video_path: Some("workspace_demo_reel.mp4".to_string()),
            video_fps: 24.0,
            in_marker: Some(120),
            out_marker: Some(450),
            ranges: vec![
                ExportRange { id: 1, in_frame: 120, out_frame: 450 },
                ExportRange { id: 2, in_frame: 600, out_frame: 820 },
            ],
            next_range_id: 3,
            cut_mode: false,
            frame_rx: None,
            latest_frame: None,
            loudness_profile: loudness,
            zoom_level: 0.0,
            timeline_height: 145.0,
            _audio_engine: audio_engine,
            playback_state,
            media_engine: None,
            keyframes: (0..1000).step_by(50).collect(),
            future_frame: None,
            latest_sequence_id: 0,
            show_debug_hud: false,
            drift_history: std::collections::VecDeque::with_capacity(60),
            stale_discards_count: 0,
            drift_pause_count: 0,
            seek_in_progress: false,
            ui_state: PlaybackState::Paused,
            last_seek_time: None,
            target_seek_frame: None,
        }
    }

    fn frame_to_time(&self, frame: usize) -> String {
        let total_seconds = frame as f64 / self.video_fps;
        let minutes = (total_seconds / 60.0).floor() as usize;
        let seconds = total_seconds.floor() as usize % 60;
        let centiseconds = ((total_seconds - total_seconds.floor()) * 100.0).floor() as usize;
        format!("{:02}:{:02}.{:02}", minutes, seconds, centiseconds)
    }

    fn set_playing(&mut self, play: bool) {
        self.is_playing = play;
        self.playback_state.is_playing.store(play, std::sync::atomic::Ordering::SeqCst);
        if play {
            self.playback_state.start_fallback_clock();
            if let Some(ref me) = self.media_engine {
                let _ = me.tx.send(crate::media::MediaCommand::Play);
            }
        } else {
            self.playback_state.stop_fallback_clock();
            if let Some(ref me) = self.media_engine {
                let _ = me.tx.send(crate::media::MediaCommand::Pause);
            }
        }
    }

    fn nearest_keyframe(&self, target: usize) -> usize {
        if self.keyframes.is_empty() {
            return target;
        }
        match self.keyframes.binary_search(&target) {
            Ok(i) => self.keyframes[i],
            Err(0) => self.keyframes[0],
            Err(i) => self.keyframes[i - 1],
        }
    }

    fn seek_to_frame(&mut self, frame: usize, keyframe_only: bool) {
        self.latest_sequence_id += 1;
        self.seek_in_progress = true;
        self.current_frame = frame.clamp(0, self.total_frames);
        self.playback_state.reset(self.current_frame as f64 / self.video_fps);
        if let Some(ref me) = self.media_engine {
            let _ = me.tx.send(crate::media::MediaCommand::Seek {
                frame_index: self.current_frame,
                keyframe_only,
                sequence_id: self.latest_sequence_id,
            });
        }
        if let Some(ref rx) = self.frame_rx {
            while rx.try_recv().is_ok() {}
        }
        self.future_frame = None;
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    
    // Install Outfit-Regular and Outfit-Medium
    fonts.font_data.insert(
        "outfit_regular".to_owned(),
        egui::FontData::from_static(include_bytes!("../../resources/fonts/Outfit-Regular.ttf")),
    );
    fonts.font_data.insert(
        "outfit_medium".to_owned(),
        egui::FontData::from_static(include_bytes!("../../resources/fonts/Outfit-Medium.ttf")),
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

fn configure_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    
    // Custom Slate/Indigo Dark Palette
    let panel_bg = Color32::from_rgb(8, 8, 14);       // Deep space black
    let card_bg = Color32::from_rgb(15, 14, 25);       // Sleek card background
    let border_color = Color32::from_rgb(33, 31, 54);   // Subdued violet border
    let text_normal = Color32::from_rgb(220, 222, 235); // Clean light text
    let accent_purple = Color32::from_rgb(99, 102, 241); // Electric indigo accent
    let accent_hover = Color32::from_rgb(124, 58, 237);  // Bright purple hover

    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = panel_bg;
    visuals.panel_fill = panel_bg;
    
    // Configure buttons, inputs, slider styles
    visuals.widgets.noninteractive.bg_fill = card_bg;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, border_color);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text_normal);

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(24, 23, 40);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, border_color);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text_normal);
    visuals.widgets.inactive.rounding = 6.0.into();

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 36, 64);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent_purple);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.hovered.rounding = 6.0.into();

    visuals.widgets.active.bg_fill = accent_purple;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent_hover);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.active.rounding = 6.0.into();

    // Selection styling
    visuals.selection.bg_fill = accent_purple;
    visuals.selection.stroke = egui::Stroke::new(1.0, accent_hover);

    // Global layout roundings
    visuals.window_rounding = 12.0.into();
    visuals.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 8.0),
        blur: 15.0,
        spread: 0.0,
        color: Color32::from_black_alpha(40),
    };

    style.visuals = visuals;
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    ctx.set_style(style);
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

fn toggle_switch(ui: &mut egui::Ui, cut_mode: &mut bool) -> egui::Response {
    let desired_size = egui::vec2(34.0, 18.0);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
    
    if response.clicked() {
        *cut_mode = !*cut_mode;
        response.mark_changed();
    }
    
    let painter = ui.painter();
    
    // Background color: red if cut_mode, green if keep_mode
    let bg_color = if *cut_mode {
        Color32::from_rgb(239, 68, 68)   // Red for CUT
    } else {
        Color32::from_rgb(16, 185, 129)  // Green for KEEP
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
    icon: PlaybackIcon,
    is_play_btn: bool,
    tooltip: &str,
) -> bool {
    let button_size = if is_play_btn { 34.0 } else { 26.0 };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(button_size, button_size),
        egui::Sense::click(),
    );

    let response = response.on_hover_text(tooltip);

    let painter = ui.painter();
    
    // Determine background color based on hover, click states
    let bg_color = if response.clicked() || response.is_pointer_button_down_on() {
        if is_play_btn {
            Color32::from_rgb(124, 58, 237) // Bright active click
        } else if matches!(icon, PlaybackIcon::AddSnippet) {
            Color32::from_rgb(52, 211, 153) // Lighter green active click (#34D399)
        } else {
            Color32::from_rgb(45, 43, 75)
        }
    } else if response.hovered() {
        if is_play_btn {
            Color32::from_rgb(124, 58, 237) // Accent purple hover
        } else if matches!(icon, PlaybackIcon::AddSnippet) {
            Color32::from_rgb(52, 211, 153) // Green hover
        } else {
            Color32::from_rgb(34, 32, 58)
        }
    } else {
        if is_play_btn {
            Color32::from_rgb(99, 102, 241) // Default play button (indigo)
        } else if matches!(icon, PlaybackIcon::AddSnippet) {
            Color32::from_rgb(16, 185, 129) // Default green (#10B981)
        } else {
            Color32::from_rgb(16, 15, 28)
        }
    };

    let border_color = if is_play_btn {
        Color32::from_rgb(124, 58, 237)
    } else if matches!(icon, PlaybackIcon::AddSnippet) {
        Color32::from_rgb(16, 185, 129)
    } else {
        Color32::from_rgb(28, 26, 48)
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
        Color32::WHITE
    } else if response.hovered() {
        Color32::from_rgb(199, 210, 254)
    } else {
        Color32::from_rgb(148, 163, 184)
    };

    draw_playback_icon(painter, rect, &icon, icon_color);

    response.clicked()
}

impl MainApp {
    fn progress_bar(
        &mut self,
        ui: &mut egui::Ui,
    ) -> bool {
        let height = 12.0; // Interaction height
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::click_and_drag(),
        );

        let response = response.on_hover_text("Drag or click to seek timeline");

        let painter = ui.painter();
        
        let track_height = 5.0; // Constant track height
        let track_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), track_height));

        // 1. Draw track background color dynamically based on cut/keep mode
        let track_color = if self.cut_mode {
            Color32::from_rgb(8, 8, 14) // When cutting, default rest is keep (deep dark)
        } else {
            Color32::from_rgb(63, 29, 29) // When keeping, default rest is cut (crimson)
        };
        painter.rect_filled(
            track_rect,
            track_height * 0.5,
            track_color,
        );

        // 2. Draw keep/cut regions (saved export ranges) on the track
        let snippet_color = if self.cut_mode {
            Color32::from_rgb(63, 29, 29) // In cut mode, snippets are red/cut
        } else {
            Color32::from_rgb(99, 102, 241) // In keep mode, snippets are indigo/kept
        };
        for range in &self.ranges {
            let left_pct = range.in_frame as f32 / self.total_frames as f32;
            let right_pct = range.out_frame as f32 / self.total_frames as f32;
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
        if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker) {
            if inf < outf {
                let left_pct = inf as f32 / self.total_frames as f32;
                let right_pct = outf as f32 / self.total_frames as f32;
                let left_x = rect.left() + left_pct * rect.width();
                let right_x = rect.left() + right_pct * rect.width();
                
                let selection_rect = egui::Rect::from_min_max(
                    egui::pos2(left_x, track_rect.min.y),
                    egui::pos2(right_x, track_rect.max.y),
                );
                painter.rect_filled(
                    selection_rect,
                    track_height * 0.5,
                    Color32::from_rgb(59, 130, 246), // Active selection color (electric blue)
                );
            }
        }

        // 4. Always draw circular playhead handle (always visible)
        let fraction = (self.current_frame as f32 / self.total_frames as f32).clamp(0.0, 1.0);
        let progress_width = rect.width() * fraction;
        let handle_x = rect.left() + progress_width;
        let handle_center = egui::pos2(handle_x, rect.center().y);
        painter.circle_filled(
            handle_center,
            6.0,
            Color32::from_rgb(236, 72, 153), // Hot pink playhead handle
        );

        // Handle user seeking click/drag
        if response.dragged() || response.clicked() {
            if let Some(mouse_pos) = response.interact_pointer_pos() {
                let rel_x = (mouse_pos.x - rect.left()).clamp(0.0, rect.width());
                let pct = rel_x / rect.width();
                self.current_frame = (pct * self.total_frames as f32) as usize;
                return true;
            }
        }

        false
    }
}

impl eframe::App for MainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let ctrl_d = egui::KeyboardShortcut::new(egui::Modifiers::CTRL, egui::Key::D);
        if ctx.input_mut(|i| i.consume_shortcut(&ctrl_d)) {
            self.show_debug_hud = !self.show_debug_hud;
        }

        let previous_frame = self.current_frame;

        // Sync fallback clock state and correct AV drift if lagging
        if self.is_playing {
            let video_time = self.latest_frame.as_ref()
                .filter(|f| f.sequence_id() == self.latest_sequence_id)
                .map_or(self.current_frame as f64 / self.video_fps, |f| f.frame_idx() as f64 / self.video_fps);
            let current_time = self.playback_state.current_time();
            let drift = current_time - video_time;

            // Harvest drift history
            self.drift_history.push_back(drift as f32);
            if self.drift_history.len() > 60 {
                self.drift_history.pop_front();
            }

            // Real-time Drift Correction: Video lag > 150ms
            if drift > 0.15 && !self.playback_state.audio_paused_for_sync.load(std::sync::atomic::Ordering::SeqCst) {
                self.playback_state.audio_paused_for_sync.store(true, std::sync::atomic::Ordering::SeqCst);
                self.drift_pause_count += 1;
                println!("AV SYNC: Video lag detected ({:.1} ms). Pausing audio for sync.", drift * 1000.0);
            }

            // Ensure playback clock is running/stopped based on audio_paused_for_sync
            if !self.playback_state.audio_paused_for_sync.load(std::sync::atomic::Ordering::SeqCst) {
                let running = self.playback_state.is_playing.swap(true, std::sync::atomic::Ordering::SeqCst);
                if !running {
                    self.playback_state.start_fallback_clock();
                }
            } else {
                self.playback_state.stop_fallback_clock();
            }

            self.current_frame = (self.playback_state.current_time() * self.video_fps) as usize;
            if self.current_frame >= self.total_frames {
                self.current_frame = self.total_frames;
                self.set_playing(false);
            }
        } else {
            self.playback_state.stop_fallback_clock();
            self.playback_state.is_playing.store(false, std::sync::atomic::Ordering::SeqCst);
        }

        // 1. Process background frames
        if let Some(ref rx) = self.frame_rx {
            let target_frame = (self.playback_state.current_time() * self.video_fps) as usize;
            
            // Promote future frame if it's now ready and not stale
            if let Some(ref f) = self.future_frame {
                if f.sequence_id() < self.latest_sequence_id {
                    self.stale_discards_count += 1;
                    self.future_frame = None;
                } else if f.frame_idx() <= target_frame {
                    self.latest_frame = self.future_frame.take();
                }
            }

            // Receive new frames from channel
            if self.future_frame.is_none() {
                while let Ok(frame) = rx.try_recv() {
                    if frame.sequence_id() < self.latest_sequence_id {
                        self.stale_discards_count += 1;
                        continue; // Discard stale frame!
                    }
                    let idx = frame.frame_idx();
                    if idx <= target_frame {
                        self.latest_frame = Some(frame);
                    } else {
                        self.future_frame = Some(frame);
                        break;
                    }
                }
            }

            if self.playback_state.audio_paused_for_sync.load(std::sync::atomic::Ordering::SeqCst) {
                let mut caught_up = false;
                if let Some(ref f) = self.latest_frame {
                    if f.sequence_id() == self.latest_sequence_id {
                        if self.seek_in_progress {
                            // Seek catch-up: we just need any frame of the new sequence
                            caught_up = true;
                        } else {
                            // Playback drift catch-up: we need the frame index to catch up to target_frame
                            if f.frame_idx() >= target_frame || f.frame_idx() >= self.total_frames.saturating_sub(1) {
                                caught_up = true;
                            }
                        }
                    }
                }
                if caught_up {
                    self.seek_in_progress = false;
                    self.playback_state.audio_paused_for_sync.store(false, std::sync::atomic::Ordering::SeqCst);
                    if self.is_playing {
                        self.playback_state.start_fallback_clock();
                    }
                }
            }

            ctx.request_repaint();
        }

        // 2. Bottom Panel: Timeline and Waveform Deck (Full width)
        egui::TopBottomPanel::bottom("timeline_deck")
            .resizable(true)
            .default_height(self.timeline_height)
            .height_range(100.0..=250.0)
            .frame(egui::Frame::none()
                .fill(Color32::from_rgb(8, 8, 14))
                .inner_margin(egui::Margin::symmetric(16.0, 12.0)))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Timeline Time Indicators & Zoom Controls
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(self.frame_to_time(self.current_frame))
                                .size(16.0)
                                .color(Color32::WHITE)
                        );
                        ui.label(format!("/ {}", self.frame_to_time(self.total_frames)));
                        
                        // Zoom controls aligned right
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Zoom In button (+)
                            if ui.small_button("+").on_hover_text("Zoom in filmstrip (show fewer frames)").clicked() {
                                self.zoom_level = (self.zoom_level + 0.1).clamp(0.0, 1.0);
                            }
                            
                            // Zoom slider
                            ui.add(egui::Slider::new(&mut self.zoom_level, 0.0..=1.0)
                                .show_value(false)
                                .text("Zoom"));

                            // Zoom Out button (-)
                            if ui.small_button("−").on_hover_text("Zoom out filmstrip (show more overview)").clicked() {
                                self.zoom_level = (self.zoom_level - 0.1).clamp(0.0, 1.0);
                            }

                            ui.add_space(8.0);
                            ui.label(format!("Frame {} / {}", self.current_frame, self.total_frames));
                        });
                    });

                    ui.add_space(4.0);

                    // 2. Sprocket Filmstrip Viewport
                    let total_width = ui.available_width();
                    let spacing_y = ui.spacing().item_spacing.y;
                    let filmstrip_height = (ui.available_height() - 30.0 - spacing_y).max(30.0);

                    let (filmstrip_rect, _response) = ui.allocate_exact_size(
                        egui::vec2(total_width, filmstrip_height),
                        egui::Sense::hover(),
                    );
                    
                    let painter = ui.painter_at(filmstrip_rect);
                    // Draw filmstrip track card background
                    painter.rect_filled(filmstrip_rect, 6.0, Color32::from_rgb(12, 11, 20));
                    painter.rect_stroke(filmstrip_rect, 6.0, egui::Stroke::new(1.0, Color32::from_rgb(26, 25, 43)));

                    let thumbnail_spacing = 6.0;
                    let sprocket_height = 8.0;
                    let inner_height = (filmstrip_height - (sprocket_height * 2.0)).max(10.0);
                    let thumbnail_width = inner_height * 16.0 / 9.0;

                    // Calculate number of odd thumbnails that fit
                    let mut n = ((total_width + thumbnail_spacing) / (thumbnail_width + thumbnail_spacing)) as usize;
                    if n % 2 == 0 { n = n.saturating_sub(1); }
                    let num_thumbnails = n.max(3);
                    let num_gaps = num_thumbnails - 1;

                    let occupied_width = num_thumbnails as f32 * thumbnail_width + num_gaps as f32 * thumbnail_spacing;
                    let left_margin = (total_width - occupied_width) * 0.5;

                    let inner_rect = egui::Rect::from_min_max(
                        egui::pos2(filmstrip_rect.left() + left_margin, filmstrip_rect.top() + sprocket_height),
                        egui::pos2(filmstrip_rect.left() + left_margin + occupied_width, filmstrip_rect.bottom() - sprocket_height),
                    );

                    // Draw sprockets on top and bottom margins (matching the actual thumbnails row width)
                    let sprocket_width = 8.0;
                    let sprocket_h = 5.0;
                    let num_sprockets = ((occupied_width / 16.0) as usize).max(5);
                    let sprocket_gap = (occupied_width - (sprocket_width * num_sprockets as f32)) / (num_sprockets - 1) as f32;
                    for s in 0..num_sprockets {
                        let sx = inner_rect.left() + s as f32 * (sprocket_width + sprocket_gap);
                        
                        // Top sprocket hole
                        let top_sprocket_rect = egui::Rect::from_min_size(
                            egui::pos2(sx, filmstrip_rect.top() + (sprocket_height - sprocket_h) * 0.5),
                            egui::vec2(sprocket_width, sprocket_h),
                        );
                        painter.rect_filled(top_sprocket_rect, 1.0, Color32::from_rgb(24, 23, 40));

                        // Bottom sprocket hole
                        let bottom_sprocket_rect = egui::Rect::from_min_size(
                            egui::pos2(sx, filmstrip_rect.bottom() - (sprocket_height + sprocket_h) * 0.5),
                            egui::vec2(sprocket_width, sprocket_h),
                        );
                        painter.rect_filled(bottom_sprocket_rect, 1.0, Color32::from_rgb(24, 23, 40));
                    }

                    // Calculate frame delta step exponentially
                    let max_delta = (self.total_frames as f32 / num_gaps as f32).max(1.0);
                    let delta_f = (max_delta.ln() * (1.0 - self.zoom_level)).exp();
                    let delta = (delta_f as usize).max(1);

                    for i in 0..num_thumbnails {
                        let offset_idx = i as isize - (num_thumbnails / 2) as isize;
                        let target_frame = self.current_frame as isize + offset_idx * delta as isize;

                        let tx = inner_rect.left() + i as f32 * (thumbnail_width + thumbnail_spacing);
                        let thub_rect = egui::Rect::from_min_size(
                            egui::pos2(tx, inner_rect.top()),
                            egui::vec2(thumbnail_width, inner_height),
                        );

                        if target_frame >= 0 && target_frame <= self.total_frames as isize {
                            let is_center = offset_idx == 0;
                            
                            let bg_color = if is_center {
                                Color32::from_rgb(33, 31, 54)
                            } else {
                                Color32::from_rgb(18, 17, 28)
                            };
                            let border_color = if is_center {
                                Color32::from_rgb(236, 72, 153) // Electric pink border for center playhead
                            } else {
                                Color32::from_rgb(45, 43, 75)
                            };
                            painter.rect_filled(thub_rect, 4.0, bg_color);
                            painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(if is_center { 1.5 } else { 1.0 }, border_color));

                            // Color graphic procedural thumbnail
                            let hash_val = target_frame as usize;
                            let bar_color = Color32::from_rgb(
                                ((hash_val * 37) % 120 + 30) as u8,
                                ((hash_val * 59) % 120 + 30) as u8,
                                ((hash_val * 97) % 120 + 60) as u8,
                            );
                            
                            // Don't draw preview rectangle if it is too small
                            if inner_height > 18.0 {
                                let content_rect = egui::Rect::from_min_max(
                                    egui::pos2(thub_rect.left() + 3.0, thub_rect.top() + 3.0),
                                    egui::pos2(thub_rect.right() - 3.0, thub_rect.bottom() - 13.0),
                                );
                                painter.rect_filled(content_rect, 2.0, bar_color);
                            }

                            // Underlay text code (timecode) - hide if too small
                            if inner_height > 12.0 {
                                let time_text = self.frame_to_time(target_frame as usize);
                                painter.text(
                                    egui::pos2(thub_rect.center().x, thub_rect.bottom() - 6.0),
                                    egui::Align2::CENTER_CENTER,
                                    time_text,
                                    egui::FontId::monospace(8.0),
                                    Color32::from_rgb(148, 163, 184),
                                );
                            }

                            // Handle click interaction to center this frame
                            let response_id = ui.make_persistent_id(format!("filmstrip_th_{}", i));
                            let th_response = ui.interact(thub_rect, response_id, egui::Sense::click());
                            if th_response.clicked() {
                                self.seek_to_frame(target_frame as usize, false);
                                self.set_playing(false);
                            }
                        } else {
                            // Out of bounds thumbnail placeholder
                            painter.rect_filled(thub_rect, 4.0, Color32::from_rgb(8, 8, 14));
                            painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(20, 19, 32)));
                            
                            if inner_height > 12.0 {
                                painter.text(
                                    thub_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "OUT",
                                    egui::FontId::proportional(9.0),
                                    Color32::from_rgb(45, 43, 75),
                                );
                            }
                        }
                    }

                    // Vertical Center Playhead line over the filmstrip
                    let center_x = inner_rect.center().x;
                    painter.line_segment(
                        [egui::pos2(center_x, filmstrip_rect.top() - 2.0), egui::pos2(center_x, filmstrip_rect.bottom() + 2.0)],
                        egui::Stroke::new(2.0, Color32::from_rgb(236, 72, 153)),
                    );
                    painter.circle_filled(
                        egui::pos2(center_x, filmstrip_rect.top()),
                        4.0,
                        Color32::from_rgb(236, 72, 153),
                    );

                    ui.add_space(6.0);

                    // 3. Compact Synced Waveform Component
                    let wave_height = 24.0;
                    let (wave_rect, wave_response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), wave_height),
                        egui::Sense::click_and_drag(),
                    );
                    let wave_response = wave_response.on_hover_text("Waveform Viewport: Drag or click to scrub");

                    let painter = ui.painter_at(wave_rect);
                    painter.rect(
                        wave_rect,
                        4.0,
                        Color32::from_rgb(14, 13, 23),
                        egui::Stroke::new(1.0, Color32::from_rgb(26, 25, 43)),
                    );

                    // Synced frame calculations matching filmstrip
                    let half_width = num_thumbnails / 2;
                    let start_frame = self.current_frame as isize - half_width as isize * delta as isize;
                    let visible_range_width = (2 * half_width * delta) as f32;

                    let frame_to_x = |frame: usize| -> f32 {
                        let pct = (frame as isize - start_frame) as f32 / visible_range_width;
                        wave_rect.left() + pct * wave_rect.width()
                    };

                    // Render mock waveform loudness bars zoomed in sync
                    let bar_count = self.loudness_profile.len();
                    let bar_width = wave_rect.width() / bar_count as f32;
                    let middle_y = wave_rect.center().y;

                    for i in 0..bar_count {
                        let pct = i as f32 / bar_count as f32;
                        let frame_idx_isize = start_frame + (pct * visible_range_width) as isize;
                        let frame_idx = frame_idx_isize.clamp(0, self.total_frames as isize) as usize;
                        let out_of_bounds = frame_idx_isize < 0 || frame_idx_isize > self.total_frames as isize;

                        let amplitude = if out_of_bounds {
                            0.02
                        } else {
                            let profile_idx = ((frame_idx as f32 / self.total_frames as f32) * (self.loudness_profile.len() - 1) as f32) as usize;
                            self.loudness_profile[profile_idx.clamp(0, self.loudness_profile.len() - 1)]
                        };

                        let bar_h = amplitude * (wave_rect.height() - 6.0);
                        let x = wave_rect.left() + i as f32 * bar_width;

                        let inside_active_selection = match (self.in_marker, self.out_marker) {
                            (Some(inf), Some(outf)) => frame_idx >= inf && frame_idx <= outf,
                            _ => false,
                        };

                        let inside_queued_range = self.ranges.iter().any(|r| frame_idx >= r.in_frame && frame_idx <= r.out_frame);

                        let bar_color = if out_of_bounds {
                            Color32::from_rgb(20, 19, 32)
                        } else if inside_active_selection {
                            Color32::from_rgb(59, 130, 246)
                        } else if inside_queued_range {
                            if self.cut_mode {
                                Color32::from_rgb(239, 68, 68)
                            } else {
                                Color32::from_rgb(16, 185, 129)
                            }
                        } else {
                            if self.cut_mode {
                                Color32::from_rgb(12, 55, 35)
                            } else {
                                Color32::from_rgb(85, 30, 30)
                            }
                        };

                        painter.line_segment(
                            [
                                egui::pos2(x + bar_width * 0.5, middle_y - bar_h * 0.5),
                                egui::pos2(x + bar_width * 0.5, middle_y + bar_h * 0.5),
                            ],
                            egui::Stroke::new(bar_width * 0.75, bar_color),
                        );
                    }

                    // Highlight queued ranges inside the visible window
                    for range in &self.ranges {
                        let raw_left = frame_to_x(range.in_frame);
                        let raw_right = frame_to_x(range.out_frame);
                        
                        if raw_right >= wave_rect.left() && raw_left <= wave_rect.right() {
                            let left_x = raw_left.clamp(wave_rect.left(), wave_rect.right());
                            let right_x = raw_right.clamp(wave_rect.left(), wave_rect.right());
                            
                            let overlay_color = if self.cut_mode {
                                Color32::from_rgba_premultiplied(239, 68, 68, 15)
                            } else {
                                Color32::from_rgba_premultiplied(16, 185, 129, 15)
                            };
                            
                            let line_color = if self.cut_mode {
                                Color32::from_rgba_premultiplied(239, 68, 68, 100)
                            } else {
                                Color32::from_rgba_premultiplied(16, 185, 129, 100)
                            };

                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(left_x, wave_rect.top() + 1.0),
                                    egui::pos2(right_x, wave_rect.bottom() - 1.0),
                                ),
                                0.0,
                                overlay_color,
                            );
                            
                            if raw_left >= wave_rect.left() && raw_left <= wave_rect.right() {
                                painter.line_segment(
                                    [egui::pos2(raw_left, wave_rect.top()), egui::pos2(raw_left, wave_rect.bottom())],
                                    egui::Stroke::new(1.0, line_color),
                                );
                            }
                            if raw_right >= wave_rect.left() && raw_right <= wave_rect.right() {
                                painter.line_segment(
                                    [egui::pos2(raw_right, wave_rect.top()), egui::pos2(raw_right, wave_rect.bottom())],
                                    egui::Stroke::new(1.0, line_color),
                                );
                            }
                        }
                    }

                    // Highlight selected range inside the visible window
                    if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker) {
                        let raw_left = frame_to_x(inf);
                        let raw_right = frame_to_x(outf);
                        
                        if raw_right >= wave_rect.left() && raw_left <= wave_rect.right() {
                            let left_x = raw_left.clamp(wave_rect.left(), wave_rect.right());
                            let right_x = raw_right.clamp(wave_rect.left(), wave_rect.right());
                            
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(left_x, wave_rect.top() + 1.0),
                                    egui::pos2(right_x, wave_rect.bottom() - 1.0),
                                ),
                                0.0,
                                Color32::from_rgba_premultiplied(59, 130, 246, 15),
                            );
                            
                            if raw_left >= wave_rect.left() && raw_left <= wave_rect.right() {
                                painter.line_segment(
                                    [egui::pos2(raw_left, wave_rect.top()), egui::pos2(raw_left, wave_rect.bottom())],
                                    egui::Stroke::new(2.0, Color32::from_rgb(59, 130, 246)),
                                );
                            }
                            if raw_right >= wave_rect.left() && raw_right <= wave_rect.right() {
                                painter.line_segment(
                                    [egui::pos2(raw_right, wave_rect.top()), egui::pos2(raw_right, wave_rect.bottom())],
                                    egui::Stroke::new(2.0, Color32::from_rgb(59, 130, 246)),
                                );
                            }
                        }
                    }

                    // Playhead vertical line is always centered in the synced waveform
                    let playhead_x = wave_rect.center().x;
                    painter.line_segment(
                        [egui::pos2(playhead_x, wave_rect.top() - 2.0), egui::pos2(playhead_x, wave_rect.bottom() + 2.0)],
                        egui::Stroke::new(2.0, Color32::from_rgb(236, 72, 153)),
                    );
                    painter.circle_filled(
                        egui::pos2(playhead_x, wave_rect.top()),
                        3.0,
                        Color32::from_rgb(236, 72, 153),
                    );

                    if wave_response.dragged() || wave_response.clicked() {
                        if let Some(mouse_pos) = wave_response.interact_pointer_pos() {
                            let rel_x = (mouse_pos.x - wave_rect.left()).clamp(0.0, wave_rect.width());
                            let pct = rel_x / wave_rect.width();
                            let target_frame = (pct * self.total_frames as f32) as usize;
                            let target_kf = self.nearest_keyframe(target_frame);
                            self.seek_to_frame(target_kf, true);
                            self.set_playing(false);
                        }
                    }
                });
            });

        // Egui's TopBottomPanel automatically persists the resized height in its internal memory.
        // We do not update self.timeline_height here to avoid a layout feedback loop.

        egui::SidePanel::right("sidebar_deck")
            .resizable(false)
            .default_width(300.0)
            .frame(egui::Frame::none()
                .fill(Color32::from_rgb(11, 10, 18))
                .stroke(egui::Stroke::new(1.0, Color32::from_rgb(22, 21, 38)))
                .inner_margin(egui::Margin::same(16.0)))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    // Title: [snippets] ✂ and Inline Toggle Switch
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
                        ui.label(egui::RichText::new("[").strong().color(Color32::from_rgb(99, 102, 241)).size(18.0));
                        ui.label(egui::RichText::new("snippets").strong().color(Color32::WHITE).size(18.0));
                        ui.label(egui::RichText::new("]").strong().color(Color32::from_rgb(99, 102, 241)).size(18.0));
                        ui.add_space(6.0);
                        
                        let (scissor_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        draw_scissor_icon(&ui.painter_at(scissor_rect), scissor_rect, Color32::from_rgb(236, 72, 153));

                        // Inline Mode Toggle Switch on the right
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let switch_resp = toggle_switch(ui, &mut self.cut_mode);
                            let tooltip_text = if self.cut_mode {
                                "Mode: CUT (Snippets define what to crop out; rest is kept)"
                            } else {
                                "Mode: KEEP (Snippets define what to keep; rest is cut)"
                            };
                            let _switch_resp = switch_resp.on_hover_text(tooltip_text);

                            ui.add_space(4.0);

                            let label_text = if self.cut_mode { "CUT" } else { "KEEP" };
                            let label_color = if self.cut_mode { Color32::from_rgb(239, 68, 68) } else { Color32::from_rgb(16, 185, 129) };
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
                                        ui.label(egui::RichText::new("[").strong().color(Color32::from_rgb(99, 102, 241)));
                                        ui.label(egui::RichText::new(format!("{:02}", range.id)).strong().color(Color32::WHITE));
                                        ui.label(egui::RichText::new("]").strong().color(Color32::from_rgb(99, 102, 241)));
                                    });

                                    // Column 2: In - Out times (monospaced)
                                    let time_str = format!("{}-{}", self.frame_to_time(range.in_frame), self.frame_to_time(range.out_frame));
                                    ui.label(egui::RichText::new(time_str).monospace().color(Color32::from_rgb(220, 222, 235)));

                                    // Column 3: Progress mini track
                                    let (rect, _response) = ui.allocate_exact_size(
                                        egui::vec2(40.0, 4.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    
                                    // Draw background track
                                    painter.rect_filled(rect, 2.0, Color32::from_rgb(30, 27, 51));
                                    
                                    // Draw fill segment
                                    if self.total_frames > 0 {
                                        let start_pct = range.in_frame as f32 / self.total_frames as f32;
                                        let len_pct = (range.out_frame - range.in_frame) as f32 / self.total_frames as f32;
                                        
                                        let fill_left = rect.left() + start_pct * rect.width();
                                        let fill_width = len_pct * rect.width();
                                        let fill_rect = egui::Rect::from_min_max(
                                            egui::pos2(fill_left, rect.top()),
                                            egui::pos2(fill_left + fill_width.max(1.0), rect.bottom()),
                                        );
                                        
                                        let fill_color = if self.cut_mode {
                                            Color32::from_rgb(239, 68, 68) // cut is red
                                        } else {
                                            Color32::from_rgb(16, 185, 129) // keep is green
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
                        }
                    });


                });
            });

        // 4. Central Panel: The Video Viewport Deck (Remaining center space)
        egui::CentralPanel::default()
            .frame(egui::Frame::none()
                .fill(Color32::from_rgb(8, 8, 14))
                .inner_margin(egui::Margin::same(16.0)))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(4.0, 0.0);

                        // Left Bracket [
                        ui.label(
                            egui::RichText::new("[")
                                .font(egui::FontId::new(20.0, egui::FontFamily::Name("medium".into())))
                                .color(Color32::from_rgb(99, 102, 241))
                        );

                        // Word mark "snippy"
                        ui.label(
                            egui::RichText::new("snippy")
                                .font(egui::FontId::new(18.0, egui::FontFamily::Name("medium".into())))
                                .color(Color32::WHITE)
                        );

                        ui.add_space(2.0);

                        // Scissor Icon
                        let (scissor_rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                        draw_scissor_icon(&ui.painter_at(scissor_rect), scissor_rect, Color32::from_rgb(236, 72, 153));

                        ui.add_space(2.0);

                        // Right Bracket ]
                        ui.label(
                            egui::RichText::new("]")
                                .font(egui::FontId::new(20.0, egui::FontFamily::Name("medium".into())))
                                .color(Color32::from_rgb(99, 102, 241))
                        );

                        ui.add_space(8.0);

                        if let Some(path) = &self.video_path {
                            let filename = std::path::Path::new(path)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(path);
                            ui.label(
                                egui::RichText::new(format!("—  {}", filename))
                                    .color(Color32::from_rgb(142, 144, 166))
                                    .font(egui::FontId::proportional(14.0))
                            );
                        }

                        // Right-justified Load and Export buttons
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);

                            // Export button
                            let has_ranges = !self.ranges.is_empty();
                            ui.add_enabled_ui(has_ranges, |ui| {
                                if playback_control_button(
                                    ui,
                                    PlaybackIcon::ExportVideo,
                                    false,
                                    "export video",
                                ) {
                                    println!("Export clicked: exporting single video with {} ranges", self.ranges.len());
                                }
                            });

                            ui.add_space(4.0);

                            // Load button
                            if playback_control_button(
                                ui,
                                PlaybackIcon::LoadVideo,
                                false,
                                "Load video file",
                            ) {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Video File", &["mp4", "mkv", "avi", "mov"])
                                    .pick_file()
                                {
                                    // 1. Stop current playback and drop old engines
                                    self.set_playing(false);
                                    self.media_engine = None;
                                    self._audio_engine = None;
                                    self.latest_frame = None;

                                    // 2. Initialize fresh Audio Engine
                                    let mut audio_engine = crate::audio::AudioEngine::new().ok();
                                    
                                    // 3. Setup channels
                                    let (frame_tx, frame_rx) = crossbeam_channel::bounded(4);
                                    let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

                                    let target_sample_rate = audio_engine.as_ref().map(|ae| ae.playback_state.sample_rate).unwrap_or(44100);
                                    let target_channels = 1; // Mono samples expected by CPAL callback
                                    
                                    // Set default audio producer
                                    let audio_tx = audio_engine.as_mut().and_then(|ae| ae.tx.take()).unwrap_or_else(|| {
                                        // Fallback/dummy buffer if no audio engine
                                        let rb = ringbuf::SharedRb::<ringbuf::storage::Heap<f32>>::new(1024);
                                        let (prod, _) = rb.split();
                                        prod
                                    });

                                    // Set default tags producer
                                    let tags_tx = audio_engine.as_mut().and_then(|ae| ae.tags_tx.take()).unwrap_or_else(|| {
                                        // Fallback/dummy buffer if no audio engine
                                        let rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(u64, u64)>>::new(256);
                                        let (prod, _) = rb.split();
                                        prod
                                    });

                                    // Resolve active playback state first
                                    let playback_state = audio_engine.as_ref()
                                        .map(|ae| std::sync::Arc::clone(&ae.playback_state))
                                        .unwrap_or_else(|| std::sync::Arc::new(crate::audio::AudioPlaybackState {
                                            current_position_us: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                                            sample_rate: 44100,
                                            has_audio: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                            fallback_start_time: std::sync::Arc::new(std::sync::Mutex::new(None)),
                                            fallback_base_time: std::sync::Arc::new(std::sync::Mutex::new(0.0)),
                                            should_flush: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                            is_playing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                            audio_paused_for_sync: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                        }));

                                    // 4. Initialize MediaEngine
                                    match crate::media::MediaEngine::new(
                                        &path,
                                        target_sample_rate,
                                        target_channels,
                                        frame_tx,
                                        audio_tx,
                                        tags_tx,
                                        cmd_rx,
                                        std::sync::Arc::clone(&playback_state),
                                    ) {
                                        Ok((me, metadata)) => {
                                            self.total_frames = metadata.total_frames;
                                            self.video_fps = metadata.fps;
                                            self.video_path = Some(path.to_string_lossy().into_owned());
                                            self.current_frame = 0;
                                            self.in_marker = None;
                                            self.out_marker = None;
                                            self.ranges.clear();
                                            self.stale_discards_count = 0;
                                            self.drift_pause_count = 0;
                                            self.drift_history.clear();
                                            
                                            playback_state.reset(0.0);
                                            playback_state.has_audio.store(metadata.has_audio && audio_engine.is_some(), std::sync::atomic::Ordering::Relaxed);
                                            
                                            self.playback_state = playback_state;
                                            self._audio_engine = audio_engine;
                                            self.media_engine = Some(me);
                                            self.frame_rx = Some(frame_rx);
                                            self.keyframes = metadata.keyframes;
                                            self.seek_to_frame(0, false);
                                            
                                            println!("Loaded video: {}", path.display());
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to load video: {:?}", e);
                                        }
                                    }
                                }
                            }
                        });
                    });
                    
                    ui.add_space(10.0);

                    // Video canvas viewport drawing card
                    let controls_height = 80.0; // Expanded to accommodate progress slider
                    let canvas_height = ui.available_height() - controls_height - 10.0;

                    egui::Frame::none()
                        .fill(Color32::from_rgb(12, 11, 20))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(26, 25, 43)))
                        .rounding(8.0)
                        .show(ui, |ui| {
                            let available_space = egui::vec2(ui.available_width(), canvas_height);
                            let (rect, _response) = ui.allocate_exact_size(
                                available_space,
                                egui::Sense::hover(),
                            );

                            let painter = ui.painter_at(rect);

                            let mut rendered = false;
                            if let Some(media::DecodedFrame::Cpu { planes, .. }) = &self.latest_frame {
                                let video_w = planes.width as f32;
                                let video_h = planes.height as f32;
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
                                        YuvCallback { planes: planes.clone() },
                                    );
                                    painter.add(callback);
                                    rendered = true;
                                }
                            }

                            if !rendered {
                                // Draw a visual loading/empty state in the viewport
                                painter.text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "WORKSPACE DEMO REEL\n\n(wgpu video surface output will render here via Metal/DirectX/Vulkan)\n(YUV conversion handled entirely on the GPU)",
                                    egui::FontId::proportional(15.0),
                                    Color32::from_rgb(85, 83, 115),
                                );
                            }

                            // Draw a subtle border glow inside the viewport
                            painter.rect_stroke(
                                rect,
                                8.0,
                                egui::Stroke::new(1.0, Color32::from_rgba_premultiplied(99, 102, 241, 10)),
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
                                
                                ui.allocate_ui_at_rect(hud_rect, |ui| {
                                    egui::Frame::none()
                                        .fill(Color32::from_rgba_unmultiplied(15, 17, 28, 235))
                                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(63, 66, 97)))
                                        .rounding(8.0)
                                        .inner_margin(10.0)
                                        .show(ui, |ui| {
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new("⚡ AV SYNC TELEMETRY").strong().color(Color32::from_rgb(129, 140, 248)));
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        let active_color = if self.is_playing { Color32::from_rgb(74, 222, 128) } else { Color32::from_rgb(156, 163, 175) };
                                                        ui.label(egui::RichText::new(if self.is_playing { "● ACTIVE" } else { "■ PAUSED" }).color(active_color).font(egui::FontId::monospace(10.0)));
                                                    });
                                                });
                                                
                                                ui.separator();
                                                
                                                let master_time = self.playback_state.current_time();
                                                let video_time = self.latest_frame.as_ref()
                                                    .filter(|f| f.sequence_id() == self.latest_sequence_id)
                                                    .map_or(self.current_frame as f64 / self.video_fps, |f| f.frame_idx() as f64 / self.video_fps);
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
                                                
                                                let drift_color = if drift.abs() > 0.15 { Color32::LIGHT_RED } else { Color32::LIGHT_GREEN };
                                                ui.horizontal(|ui| {
                                                    ui.label("AV Sync Drift:");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{:.1} ms ({})", drift * 1000.0, if drift > 0.0 { "Video Lag" } else { "Video Lead" })).monospace().color(drift_color));
                                                    });
                                                });
                                                
                                                ui.horizontal(|ui| {
                                                    ui.label("Sequence ID :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{}", self.latest_sequence_id)).monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("Stale Drops :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{}", self.stale_discards_count)).monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("Sync Pauses :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{}", self.drift_pause_count)).monospace());
                                                    });
                                                });
                                                ui.horizontal(|ui| {
                                                    ui.label("CPAL Rate   :");
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.label(egui::RichText::new(format!("{} Hz", self.playback_state.sample_rate)).monospace());
                                                    });
                                                });
                                                
                                                // Task 4: Custom Sparkline Plot
                                                ui.add_space(6.0);
                                                let plot_height = 50.0;
                                                let (plot_rect, _) = ui.allocate_exact_size(
                                                    egui::vec2(ui.available_width(), plot_height),
                                                    egui::Sense::hover(),
                                                );
                                                
                                                let plot_painter = ui.painter_at(plot_rect);
                                                plot_painter.rect_filled(plot_rect, 4.0, Color32::from_rgb(11, 13, 25));
                                                plot_painter.rect_stroke(plot_rect, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(39, 42, 68)));
                                                
                                                let zero_y = plot_rect.center().y;
                                                plot_painter.line_segment(
                                                    [egui::pos2(plot_rect.left(), zero_y), egui::pos2(plot_rect.right(), zero_y)],
                                                    egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(148, 163, 184, 50)),
                                                );
                                                
                                                let max_scale_sec = 0.3;
                                                let scale_y = |drift_val_sec: f32| -> f32 {
                                                    let normalized = (drift_val_sec / max_scale_sec).clamp(-1.0, 1.0);
                                                    zero_y - normalized * (plot_height * 0.4)
                                                };
                                                
                                                let t_plus_y = scale_y(0.15);
                                                let t_minus_y = scale_y(-0.15);
                                                
                                                plot_painter.line_segment([egui::pos2(plot_rect.left(), t_plus_y), egui::pos2(plot_rect.right(), t_plus_y)], egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(244, 63, 94, 80)));
                                                plot_painter.line_segment([egui::pos2(plot_rect.left(), t_minus_y), egui::pos2(plot_rect.right(), t_minus_y)], egui::Stroke::new(1.0, Color32::from_rgba_unmultiplied(244, 63, 94, 80)));
                                                
                                                plot_painter.text(egui::pos2(plot_rect.left() + 4.0, t_plus_y - 2.0), egui::Align2::LEFT_BOTTOM, "+150ms", egui::FontId::monospace(8.0), Color32::from_rgba_unmultiplied(244, 63, 94, 150));
                                                plot_painter.text(egui::pos2(plot_rect.left() + 4.0, t_minus_y + 2.0), egui::Align2::LEFT_TOP, "-150ms", egui::FontId::monospace(8.0), Color32::from_rgba_unmultiplied(244, 63, 94, 150));
                                                
                                                if self.drift_history.len() > 1 {
                                                    let mut points = Vec::new();
                                                    let dx = plot_rect.width() / (self.drift_history.len() - 1) as f32;
                                                    for (i, &d_val) in self.drift_history.iter().enumerate() {
                                                        let x = plot_rect.left() + i as f32 * dx;
                                                        let y = scale_y(d_val);
                                                        points.push(egui::pos2(x, y));
                                                    }
                                                    for window in points.windows(2) {
                                                        plot_painter.line_segment([window[0], window[1]], egui::Stroke::new(1.5, Color32::from_rgb(251, 113, 133)));
                                                    }
                                                }
                                                
                                                // Task 5: Diagnostic Buttons
                                                ui.add_space(8.0);
                                                ui.horizontal(|ui| {
                                                    if ui.button("Flush Buffer").on_hover_text("Flush/drain audio ring buffer").clicked() {
                                                        self.playback_state.reset(self.current_frame as f64 / self.video_fps);
                                                        println!("HUD: Manual flush of audio buffer triggered");
                                                    }
                                                    if ui.button("Force Resync").on_hover_text("Force resync clocks").clicked() {
                                                        let current = self.current_frame;
                                                        self.seek_to_frame(current, false);
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
                    if self.progress_bar(ui) {
                        self.set_playing(false);
                        let target_kf = self.nearest_keyframe(self.current_frame);
                        self.seek_to_frame(target_kf, true);
                    }

                    ui.add_space(10.0);

                    // Grouped Controls bar directly below video viewport
                    ui.horizontal(|ui| {
                        let full_width = ui.available_width();

                        // 1. Playback controls centered relative to the full_width
                        let controls_width = 238.0; // 6 * 26 + 34 (play) + 6 * 8 (spacing) = 238
                        let offset = (full_width - controls_width) * 0.5;
                        ui.add_space(offset.max(0.0));

                        ui.allocate_ui_with_layout(
                            egui::vec2(controls_width, 34.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);

                                if playback_control_button(ui, PlaybackIcon::Start, false, "Go to start of video") {
                                    self.seek_to_frame(0, false);
                                }
                                if playback_control_button(ui, PlaybackIcon::PrevFrame, false, "Step backward one frame") {
                                    self.seek_to_frame(self.current_frame.saturating_sub(1), false);
                                }
                                if playback_control_button(ui, PlaybackIcon::SetIn, false, "Set In marker at current frame [") {
                                    self.in_marker = Some(self.current_frame);
                                }

                                // Play/pause button
                                let play_icon = if self.is_playing { PlaybackIcon::Pause } else { PlaybackIcon::Play };
                                let play_tooltip = if self.is_playing { "Pause playback" } else { "Start playback" };
                                if playback_control_button(ui, play_icon, true, play_tooltip) {
                                    self.set_playing(!self.is_playing);
                                }

                                if playback_control_button(ui, PlaybackIcon::SetOut, false, "Set Out marker at current frame ]") {
                                    self.out_marker = Some(self.current_frame);
                                }
                                if playback_control_button(ui, PlaybackIcon::NextFrame, false, "Step forward one frame") {
                                    self.seek_to_frame(self.current_frame + 1, false);
                                }
                                if playback_control_button(ui, PlaybackIcon::End, false, "Go to end of video") {
                                    self.seek_to_frame(self.total_frames, false);
                                }
                            },
                        );

                        // 2. Right-justified Add Snippet button (placed at the end of the row)
                        let mut clicked = false;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let enabled = self.in_marker.is_some() && self.out_marker.is_some();
                            ui.add_enabled_ui(enabled, |ui| {
                                if playback_control_button(ui, PlaybackIcon::AddSnippet, false, "Add current selection as a new snippet") {
                                    clicked = true;
                                }
                            });
                        });

                        if clicked {
                            if let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker) {
                                if inf < outf {
                                    self.ranges.push(ExportRange {
                                        id: self.next_range_id,
                                        in_frame: inf,
                                        out_frame: outf,
                                    });
                                    self.next_range_id += 1;
                                }
                            }
                        }
                    });
                });
            });

        if self.current_frame != previous_frame && !self.is_playing {
            self.playback_state.reset(self.current_frame as f64 / self.video_fps);
        }
    }
}

struct YuvCallback {
    planes: media::YuvPlanes,
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
            renderer.update_textures(device, queue, &self.planes);
        }
        vec![]
    }

    fn paint<'a>(
        &'a self,
        info: egui::PaintCallbackInfo,
        render_pass: &mut eframe::wgpu::RenderPass<'a>,
        callback_resources: &'a eframe::egui_wgpu::CallbackResources,
    ) {
        let viewport = info.viewport_in_pixels();
        let clip_rect = info.clip_rect_in_pixels();

        render_pass.set_viewport(
            viewport.left_px as f32,
            viewport.top_px as f32,
            viewport.width_px as f32,
            viewport.height_px as f32,
            0.0,
            1.0,
        );

        render_pass.set_scissor_rect(
            clip_rect.left_px.max(0) as u32,
            clip_rect.top_px.max(0) as u32,
            clip_rect.width_px.max(0) as u32,
            clip_rect.height_px.max(0) as u32,
        );

        if let Some(renderer) = callback_resources.get::<yuv_renderer::YuvRenderer>() {
            renderer.paint(render_pass);
        }
    }
}


