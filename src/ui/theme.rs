use egui::Color32;
use crate::settings::Theme;

#[derive(Debug, Clone)]
pub struct ThemePalette {
    pub is_dark: bool,
    
    // Core Layout Colors
    pub panel_bg: Color32,
    pub card_bg: Color32,
    pub border_color: Color32,
    
    // Typography Colors
    pub text_normal: Color32,
    pub text_muted: Color32,
    
    // Accents & Warnings
    pub accent: Color32,
    pub accent_hover: Color32,
    pub secondary: Color32,
    pub secondary_hover: Color32,
    pub warn_color: Color32,
    
    // Inactive & Interactive Widget Backgrounds
    pub widget_inactive_bg: Color32,
    pub widget_hovered_bg: Color32,
    pub widget_active_bg: Color32,

    // Timeline & Editing Colors
    pub cut_color: Color32,
    pub keep_color: Color32,
    pub timeline_track_bg: Color32,
    pub timeline_snippet_cut_bg: Color32,
    pub timeline_snippet_keep_bg: Color32,
}

pub fn get_palette(appearance: &crate::settings::AppearanceSettings) -> ThemePalette {
    match appearance.theme {
        Theme::CustomDynamic => {
            generate_dynamic_palette_expansive(appearance)
        }
        Theme::VideoEditorDark => ThemePalette {
            is_dark: true,
            panel_bg: Color32::from_rgb(18, 18, 22),
            card_bg: Color32::from_rgb(30, 30, 36),
            border_color: Color32::from_rgb(55, 55, 65),
            text_normal: Color32::from_rgb(242, 242, 247),
            text_muted: Color32::from_rgb(158, 158, 170),
            accent: Color32::from_rgb(59, 130, 246),
            accent_hover: Color32::from_rgb(96, 165, 250),
            secondary: Color32::from_rgb(100, 116, 139),
            secondary_hover: Color32::from_rgb(148, 163, 184),
            warn_color: Color32::from_rgb(245, 158, 11),
            widget_inactive_bg: Color32::from_rgb(38, 38, 46),
            widget_hovered_bg: Color32::from_rgb(48, 48, 58),
            widget_active_bg: Color32::from_rgb(59, 130, 246),
            cut_color: Color32::from_rgb(239, 68, 68),
            keep_color: Color32::from_rgb(16, 185, 129),
            timeline_track_bg: Color32::from_rgb(15, 15, 18),
            timeline_snippet_cut_bg: Color32::from_rgba_premultiplied(239, 68, 68, 45),
            timeline_snippet_keep_bg: Color32::from_rgba_premultiplied(16, 185, 129, 45),
        },
        Theme::SleekMidnight => ThemePalette {
            is_dark: true,
            panel_bg: Color32::from_rgb(9, 9, 15),
            card_bg: Color32::from_rgb(18, 18, 27),
            border_color: Color32::from_rgb(33, 33, 53),
            text_normal: Color32::from_rgb(226, 232, 240),
            text_muted: Color32::from_rgb(148, 163, 184),
            accent: Color32::from_rgb(99, 102, 241),
            accent_hover: Color32::from_rgb(129, 140, 248),
            secondary: Color32::from_rgb(139, 92, 246),
            secondary_hover: Color32::from_rgb(167, 139, 250),
            warn_color: Color32::from_rgb(245, 158, 11),
            widget_inactive_bg: Color32::from_rgb(26, 26, 38),
            widget_hovered_bg: Color32::from_rgb(38, 38, 56),
            widget_active_bg: Color32::from_rgb(99, 102, 241),
            cut_color: Color32::from_rgb(244, 63, 94),
            keep_color: Color32::from_rgb(16, 185, 129),
            timeline_track_bg: Color32::from_rgb(5, 5, 8),
            timeline_snippet_cut_bg: Color32::from_rgba_premultiplied(244, 63, 94, 45),
            timeline_snippet_keep_bg: Color32::from_rgba_premultiplied(16, 185, 129, 45),
        },
        Theme::HighContrastOled => ThemePalette {
            is_dark: true,
            panel_bg: Color32::from_rgb(0, 0, 0),
            card_bg: Color32::from_rgb(18, 18, 18),
            border_color: Color32::from_rgb(42, 42, 42),
            text_normal: Color32::from_rgb(255, 255, 255),
            text_muted: Color32::from_rgb(163, 163, 163),
            accent: Color32::from_rgb(0, 240, 255),
            accent_hover: Color32::from_rgb(51, 243, 255),
            secondary: Color32::from_rgb(255, 0, 255),
            secondary_hover: Color32::from_rgb(255, 102, 255),
            warn_color: Color32::from_rgb(251, 191, 36),
            widget_inactive_bg: Color32::from_rgb(24, 24, 24),
            widget_hovered_bg: Color32::from_rgb(36, 36, 36),
            widget_active_bg: Color32::from_rgb(0, 240, 255),
            cut_color: Color32::from_rgb(255, 59, 48),
            keep_color: Color32::from_rgb(52, 199, 89),
            timeline_track_bg: Color32::from_rgb(8, 8, 8),
            timeline_snippet_cut_bg: Color32::from_rgba_premultiplied(255, 59, 48, 45),
            timeline_snippet_keep_bg: Color32::from_rgba_premultiplied(52, 199, 89, 45),
        },
        Theme::ClassicIndigoDark => ThemePalette {
            is_dark: true,
            panel_bg: Color32::from_rgb(8, 8, 14),
            card_bg: Color32::from_rgb(15, 14, 25),
            border_color: Color32::from_rgb(33, 31, 54),
            text_normal: Color32::from_rgb(220, 222, 235),
            text_muted: Color32::from_rgb(130, 130, 160),
            accent: Color32::from_rgb(99, 102, 241),
            accent_hover: Color32::from_rgb(124, 58, 237),
            secondary: Color32::from_rgb(219, 39, 119),
            secondary_hover: Color32::from_rgb(244, 63, 94),
            warn_color: Color32::from_rgb(200, 150, 50),
            widget_inactive_bg: Color32::from_rgb(24, 23, 40),
            widget_hovered_bg: Color32::from_rgb(38, 36, 64),
            widget_active_bg: Color32::from_rgb(99, 102, 241),
            cut_color: Color32::from_rgb(239, 68, 68),
            keep_color: Color32::from_rgb(16, 185, 129),
            timeline_track_bg: Color32::from_rgb(16, 15, 28),
            timeline_snippet_cut_bg: Color32::from_rgba_premultiplied(239, 68, 68, 45),
            timeline_snippet_keep_bg: Color32::from_rgba_premultiplied(16, 185, 129, 45),
        },
        Theme::VideoEditorLight => ThemePalette {
            is_dark: false,
            panel_bg: Color32::from_rgb(243, 244, 246),
            card_bg: Color32::from_rgb(229, 231, 235),
            border_color: Color32::from_rgb(209, 213, 219),
            text_normal: Color32::from_rgb(31, 41, 55),
            text_muted: Color32::from_rgb(75, 85, 99),
            accent: Color32::from_rgb(37, 99, 235),
            accent_hover: Color32::from_rgb(29, 78, 216),
            secondary: Color32::from_rgb(71, 85, 105),
            secondary_hover: Color32::from_rgb(51, 65, 85),
            warn_color: Color32::from_rgb(217, 119, 6),
            widget_inactive_bg: Color32::from_rgb(209, 213, 219),
            widget_hovered_bg: Color32::from_rgb(190, 192, 200),
            widget_active_bg: Color32::from_rgb(37, 99, 235),
            cut_color: Color32::from_rgb(220, 38, 38),
            keep_color: Color32::from_rgb(5, 150, 105),
            timeline_track_bg: Color32::from_rgb(255, 255, 255),
            timeline_snippet_cut_bg: Color32::from_rgba_premultiplied(220, 38, 38, 45),
            timeline_snippet_keep_bg: Color32::from_rgba_premultiplied(5, 150, 105, 45),
        },
        Theme::ClassicWarmLight => ThemePalette {
            is_dark: false,
            panel_bg: Color32::from_rgb(250, 250, 249),
            card_bg: Color32::from_rgb(245, 245, 244),
            border_color: Color32::from_rgb(231, 229, 228),
            text_normal: Color32::from_rgb(28, 25, 23),
            text_muted: Color32::from_rgb(87, 83, 78),
            accent: Color32::from_rgb(124, 58, 237),
            accent_hover: Color32::from_rgb(109, 40, 217),
            secondary: Color32::from_rgb(217, 119, 6),
            secondary_hover: Color32::from_rgb(180, 83, 9),
            warn_color: Color32::from_rgb(217, 119, 6),
            widget_inactive_bg: Color32::from_rgb(231, 229, 228),
            widget_hovered_bg: Color32::from_rgb(210, 208, 205),
            widget_active_bg: Color32::from_rgb(124, 58, 237),
            cut_color: Color32::from_rgb(225, 29, 72),
            keep_color: Color32::from_rgb(22, 163, 74),
            timeline_track_bg: Color32::from_rgb(255, 255, 255),
            timeline_snippet_cut_bg: Color32::from_rgba_premultiplied(225, 29, 72, 45),
            timeline_snippet_keep_bg: Color32::from_rgba_premultiplied(22, 163, 74, 45),
        },
    }
}


pub fn rgb_to_oklch(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let to_linear = |c: u8| -> f32 {
        let val = c as f32 / 255.0;
        if val <= 0.04045 {
            val / 12.92
        } else {
            ((val + 0.055) / 1.055).powf(2.4)
        }
    };
    
    let r_lin = to_linear(r);
    let g_lin = to_linear(g);
    let b_lin = to_linear(b);
    
    let l_lms = 0.4122214708 * r_lin + 0.5363113620 * g_lin + 0.0514453018 * b_lin;
    let m_lms = 0.2119034982 * r_lin + 0.6806995478 * g_lin + 0.1073969540 * b_lin;
    let s_lms = 0.0883024619 * r_lin + 0.2817188376 * g_lin + 0.6299787005 * b_lin;
    
    let l_prime = l_lms.max(0.0).powf(1.0 / 3.0);
    let m_prime = m_lms.max(0.0).powf(1.0 / 3.0);
    let s_prime = s_lms.max(0.0).powf(1.0 / 3.0);
    
    let l_ok = 0.2104542553 * l_prime + 0.7936177850 * m_prime - 0.0040720468 * s_prime;
    let a_ok = 1.9779984951 * l_prime - 2.4285922050 * m_prime + 0.4505937099 * s_prime;
    let b_ok = 0.0259040371 * l_prime + 0.7827717658 * m_prime - 0.8086757660 * s_prime;
    
    let l = l_ok.clamp(0.0, 1.0);
    let c = (a_ok * a_ok + b_ok * b_ok).sqrt();
    let mut h = b_ok.atan2(a_ok).to_degrees();
    if h < 0.0 {
        h += 360.0;
    }
    
    (l, c, h)
}

pub fn oklch_to_rgb(l: f32, c: f32, h_deg: f32) -> Color32 {
    let mut min_c = 0.0;
    let mut max_c = c;
    let mut best_rgb = None;
    
    for _ in 0..12 {
        let mid_c = (min_c + max_c) * 0.5;
        if let Some(rgb) = oklch_to_linear_rgb_raw(l, mid_c, h_deg) {
            best_rgb = Some(rgb);
            min_c = mid_c;
        } else {
            max_c = mid_c;
        }
    }
    
    let (r, g, b) = best_rgb.unwrap_or_else(|| {
        let h_rad = h_deg.to_radians();
        let a = c * h_rad.cos();
        let b = c * h_rad.sin();
        let l_lms = l + 0.3963377774 * a + 0.2158037573 * b;
        let m_lms = l - 0.1055613458 * a - 0.0638541728 * b;
        let s_lms = l - 0.0894841775 * a - 1.2914855480 * b;
        let l_lin = l_lms.powi(3);
        let m_lin = m_lms.powi(3);
        let s_lin = s_lms.powi(3);
        let r_lin = (4.0767416621 * l_lin - 3.3077115913 * m_lin + 0.2309699292 * s_lin).clamp(0.0, 1.0);
        let g_lin = (-1.2684380046 * l_lin + 2.6097574011 * m_lin - 0.3413193965 * s_lin).clamp(0.0, 1.0);
        let b_lin = (-0.0041960863 * l_lin - 0.7034186147 * m_lin + 1.7076147010 * s_lin).clamp(0.0, 1.0);
        
        let to_srgb = |val: f32| -> u8 {
            let srgb = if val <= 0.0031308 {
                12.92 * val
            } else {
                1.055 * val.powf(1.0 / 2.4) - 0.055
            };
            (srgb * 255.0).round() as u8
        };
        (to_srgb(r_lin), to_srgb(g_lin), to_srgb(b_lin))
    });
    
    Color32::from_rgb(r, g, b)
}

fn oklch_to_linear_rgb_raw(l: f32, c: f32, h_deg: f32) -> Option<(u8, u8, u8)> {
    let h_rad = h_deg.to_radians();
    let a = c * h_rad.cos();
    let b = c * h_rad.sin();
    
    let l_lms = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_lms = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_lms = l - 0.0894841775 * a - 1.2914855480 * b;
    
    let l_lin = l_lms.powi(3);
    let m_lin = m_lms.powi(3);
    let s_lin = s_lms.powi(3);
    
    let r_lin = 4.0767416621 * l_lin - 3.3077115913 * m_lin + 0.2309699292 * s_lin;
    let g_lin = -1.2684380046 * l_lin + 2.6097574011 * m_lin - 0.3413193965 * s_lin;
    let b_lin = -0.0041960863 * l_lin - 0.7034186147 * m_lin + 1.7076147010 * s_lin;
    
    if r_lin < -0.001 || r_lin > 1.001 || g_lin < -0.001 || g_lin > 1.001 || b_lin < -0.001 || b_lin > 1.001 {
        return None;
    }
    
    let to_srgb = |val: f32| -> u8 {
        let val = val.clamp(0.0, 1.0);
        let srgb = if val <= 0.0031308 {
            12.92 * val
        } else {
            1.055 * val.powf(1.0 / 2.4) - 0.055
        };
        (srgb * 255.0).round() as u8
    };
    
    Some((to_srgb(r_lin), to_srgb(g_lin), to_srgb(b_lin)))
}

fn generate_dynamic_palette_expansive(appearance: &crate::settings::AppearanceSettings) -> ThemePalette {
    use crate::settings::{CustomMode, ColorHarmony, SemanticStyle};
    
    let is_dark = match appearance.custom_theme_mode {
        CustomMode::Dark => true,
        CustomMode::Light => false,
        CustomMode::Custom => appearance.custom_bg_lightness < 0.5,
    };
    
    let hue = appearance.custom_theme_hue;
    let chroma = appearance.custom_theme_chroma;
    
    // Determine background / neutral settings
    let (bg_l, bg_c, neutral_h) = match appearance.custom_theme_mode {
        CustomMode::Dark => (0.08, appearance.custom_bg_chroma, appearance.custom_neutral_hue),
        CustomMode::Light => (0.96, appearance.custom_bg_chroma, appearance.custom_neutral_hue),
        CustomMode::Custom => (
            appearance.custom_bg_lightness,
            appearance.custom_bg_chroma,
            appearance.custom_neutral_hue,
        ),
    };
    
    let (accent_l, accent_c, accent_h) = match appearance.custom_theme_mode {
        CustomMode::Dark => (0.65, chroma, hue),
        CustomMode::Light => (0.55, chroma, hue),
        CustomMode::Custom => (
            appearance.custom_accent_lightness,
            chroma,
            hue,
        ),
    };
    
    // Core Backgrounds & Surfaces
    let panel_bg = oklch_to_rgb(bg_l, bg_c, neutral_h);
    let card_bg = oklch_to_rgb(
        if is_dark { (bg_l + 0.05).min(1.0) } else { (bg_l - 0.06).max(0.0) },
        bg_c * 1.2,
        neutral_h,
    );
    let border_color = oklch_to_rgb(
        if is_dark { (bg_l + 0.12).min(1.0) } else { (bg_l - 0.18).max(0.0) },
        bg_c * 1.5,
        neutral_h,
    );
    
    let widget_inactive_bg = oklch_to_rgb(
        if is_dark { (bg_l + 0.08).min(1.0) } else { (bg_l - 0.10).max(0.0) },
        bg_c * 1.2,
        neutral_h,
    );
    let widget_hovered_bg = oklch_to_rgb(
        if is_dark { (bg_l + 0.14).min(1.0) } else { (bg_l - 0.16).max(0.0) },
        bg_c * 1.5,
        neutral_h,
    );
    
    // Typography with contrast safety based on actual bg_l
    let text_normal = if is_dark {
        let text_l = (bg_l + 0.87).min(0.98);
        oklch_to_rgb(text_l, bg_c * 0.4, neutral_h)
    } else {
        let text_l = (bg_l - 0.80).max(0.05);
        oklch_to_rgb(text_l, bg_c * 0.4, neutral_h)
    };
    let text_muted = if is_dark {
        let text_l = (bg_l + 0.57).min(0.85);
        oklch_to_rgb(text_l, bg_c * 0.6, neutral_h)
    } else {
        let text_l = (bg_l - 0.50).max(0.25);
        oklch_to_rgb(text_l, bg_c * 0.6, neutral_h)
    };
    
    // Accent
    let accent = oklch_to_rgb(accent_l, accent_c, accent_h);
    let accent_hover = oklch_to_rgb(
        if is_dark { (accent_l + 0.07).min(1.0) } else { (accent_l - 0.07).max(0.0) },
        (accent_c + 0.03).min(0.37),
        accent_h,
    );
    let widget_active_bg = accent;
    
    // Secondary Color Harmony
    let secondary_h = match appearance.custom_harmony {
        ColorHarmony::Complementary => (accent_h + 180.0) % 360.0,
        ColorHarmony::SplitComplementary => (accent_h + 150.0) % 360.0,
        ColorHarmony::Analogous => (accent_h + 30.0) % 360.0,
        ColorHarmony::Triadic => (accent_h + 120.0) % 360.0,
        ColorHarmony::Tetradic => (accent_h + 90.0) % 360.0,
        ColorHarmony::Monochromatic => accent_h,
    };
    let secondary_c = appearance.custom_secondary_chroma;
    let secondary_l = if is_dark {
        (accent_l - 0.1).max(0.3)
    } else {
        (accent_l + 0.1).min(0.8)
    };
    let secondary = oklch_to_rgb(secondary_l, secondary_c, secondary_h);
    let secondary_hover = oklch_to_rgb(
        if is_dark { (secondary_l + 0.07).min(1.0) } else { (secondary_l - 0.07).max(0.0) },
        (secondary_c + 0.03).min(0.37),
        secondary_h,
    );

    // Deriving Cut/Keep/Warn Hues via Color Harmony or Classic semantic colors
    let (keep_h, cut_h, warn_h) = match appearance.custom_semantic_style {
        SemanticStyle::Classic => (140.0, 25.0, 75.0),
        SemanticStyle::Harmonized => match appearance.custom_harmony {
            ColorHarmony::Complementary => (
                accent_h,
                (accent_h + 180.0) % 360.0,
                (accent_h + 90.0) % 360.0,
            ),
            ColorHarmony::SplitComplementary => (
                (accent_h + 30.0) % 360.0,
                (accent_h + 150.0) % 360.0,
                (accent_h + 210.0) % 360.0,
            ),
            ColorHarmony::Analogous => (
                (accent_h + 330.0) % 360.0,
                (accent_h + 30.0) % 360.0,
                accent_h,
            ),
            ColorHarmony::Triadic => (
                (accent_h + 120.0) % 360.0,
                (accent_h + 240.0) % 360.0,
                (accent_h + 30.0) % 360.0,
            ),
            ColorHarmony::Tetradic => (
                (accent_h + 180.0) % 360.0,
                (accent_h + 90.0) % 360.0,
                (accent_h + 270.0) % 360.0,
            ),
            ColorHarmony::Monochromatic => (
                accent_h,
                accent_h,
                accent_h,
            ),
        },
    };
    
    // Cut/Keep/Warn colors derived from harmony or classic tones
    let (keep_color, cut_color, warn_color) = if appearance.custom_semantic_style == SemanticStyle::Classic {
        if is_dark {
            (
                oklch_to_rgb(0.65, (chroma * 0.8).clamp(0.08, 0.16), 140.0),
                oklch_to_rgb(0.60, (chroma * 0.9).clamp(0.10, 0.18), 25.0),
                oklch_to_rgb(0.72, (chroma * 0.7).clamp(0.08, 0.15), 75.0),
            )
        } else {
            (
                oklch_to_rgb(0.45, (chroma * 0.8).clamp(0.08, 0.16), 140.0),
                oklch_to_rgb(0.45, (chroma * 0.9).clamp(0.10, 0.18), 25.0),
                oklch_to_rgb(0.55, (chroma * 0.7).clamp(0.08, 0.15), 75.0),
            )
        }
    } else if appearance.custom_harmony == ColorHarmony::Monochromatic {
        (
            oklch_to_rgb(if is_dark { 0.70 } else { 0.45 }, (accent_c * 1.2).min(0.37), keep_h),
            oklch_to_rgb(if is_dark { 0.45 } else { 0.70 }, (accent_c * 0.6).min(0.37), cut_h),
            oklch_to_rgb(if is_dark { 0.85 } else { 0.30 }, (accent_c * 0.9).min(0.37), warn_h),
        )
    } else {
        (
            oklch_to_rgb(if is_dark { 0.65 } else { 0.55 }, (accent_c * 1.1).min(0.37), keep_h),
            oklch_to_rgb(if is_dark { 0.60 } else { 0.50 }, (accent_c * 1.3).min(0.37), cut_h),
            oklch_to_rgb(if is_dark { 0.70 } else { 0.60 }, accent_c, warn_h),
        )
    };
    
    let timeline_track_bg = oklch_to_rgb(
        if is_dark { (bg_l - 0.04).max(0.0) } else { 1.0 },
        if is_dark { bg_c * 0.5 } else { 0.0 },
        neutral_h,
    );
    
    let timeline_snippet_cut_bg = Color32::from_rgba_premultiplied(
        cut_color.r(), cut_color.g(), cut_color.b(), 45
    );
    let timeline_snippet_keep_bg = Color32::from_rgba_premultiplied(
        keep_color.r(), keep_color.g(), keep_color.b(), 45
    );
    
    ThemePalette {
        is_dark,
        panel_bg,
        card_bg,
        border_color,
        text_normal,
        text_muted,
        accent,
        accent_hover,
        secondary,
        secondary_hover,
        warn_color,
        widget_inactive_bg,
        widget_hovered_bg,
        widget_active_bg,
        cut_color,
        keep_color,
        timeline_track_bg,
        timeline_snippet_cut_bg,
        timeline_snippet_keep_bg,
    }
}
