# Themeable Color Palette Design Spec

A design for abstracting Snippy's color palettes, fixing low-contrast issues, and implementing selectable dark and light themes.

## 1. Problem & Goals
Currently, Snippy hardcodes color definitions across various places in `src/ui/mod.rs`. The default theme uses a dark slate palette with low contrast ratios:
* Cards and panel backgrounds blend together.
* Dividers, borders, and inactive button backgrounds have a contrast ratio of under 1.5:1 against surfaces.
* Interactive states lack clear visual cues.
* The app is locked into a single dark indigo theme.

### Goals
* **High Contrast:** Ensure all primary text/background combinations meet or exceed WCAG AA requirements (4.5:1), and strive for WCAG AAA (7:1).
* **Theme Abstraction:** Centralize color definitions into a unified palette structure.
* **Selectable Themes:** Support a selection of 4 dark themes and 2 light themes via the Settings modal.
* **Instant Refresh:** Re-apply theme changes instantly without requiring a media or application restart.

---

## 2. Palette Architecture

We will define a `ThemePalette` struct representing all standard colors needed by the UI:

```rust
pub struct ThemePalette {
    pub is_dark: bool,
    
    // Core Layout Colors
    pub panel_bg: egui::Color32,
    pub card_bg: egui::Color32,
    pub border_color: egui::Color32,
    
    // Typography Colors
    pub text_normal: egui::Color32,
    pub text_muted: egui::Color32,
    
    // Accents & Warnings
    pub accent: egui::Color32,
    pub accent_hover: egui::Color32,
    pub warn_color: egui::Color32,
    
    // Inactive & Interactive Widget Backgrounds
    pub widget_inactive_bg: egui::Color32,
    pub widget_hovered_bg: egui::Color32,
    pub widget_active_bg: egui::Color32,

    // Timeline & Editing Colors
    pub cut_color: egui::Color32,
    pub keep_color: egui::Color32,
    pub timeline_track_bg: egui::Color32,
    pub timeline_snippet_cut_bg: egui::Color32,
    pub timeline_snippet_keep_bg: egui::Color32,
}
```

---

## 3. Theme Variants & Visual Tokens

The `Theme` enum in `src/settings.rs` will be expanded to support 6 variants. Each variant maps to a distinct palette:

| Theme Option | Type | Panel BG | Card BG | Borders | Accent | Cut Color | Keep Color |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **VideoEditorDark** (Default) | Dark | `#121216` | `#1e1e24` | `#373741` | `#3b82f6` (Blue) | `#ef4444` | `#10b981` |
| **SleekMidnight** | Dark | `#09090f` | `#12121b` | `#212135` | `#6366f1` (Indigo) | `#f43f5e` | `#10b981` |
| **HighContrastOled** | Dark | `#000000` | `#121212` | `#2a2a2a` | `#00f0ff` (Cyan) | `#ff3b30` | `#34c759` |
| **ClassicIndigoDark** | Dark | `#08080e` | `#0f0e19` | `#211f36` | `#6366f1` (Indigo) | `#ef4444` | `#10b981` |
| **VideoEditorLight** | Light | `#f3f4f6` | `#e5e7eb` | `#d1d5db` | `#2563eb` (Royal Blue) | `#dc2626` | `#059669` |
| **ClassicWarmLight** | Light | `#fafaf9` | `#f5f5f4` | `#e7e5e4` | `#7c3aed` (Purple) | `#e11d48` | `#16a34a` |

### Palette Mapping Helper
We will add a helper method to resolve the active `ThemePalette`:
```rust
impl Theme {
    pub fn palette(&self) -> ThemePalette {
        use egui::Color32;
        match self {
            Theme::VideoEditorDark => ThemePalette {
                is_dark: true,
                panel_bg: Color32::from_rgb(18, 18, 22),
                card_bg: Color32::from_rgb(30, 30, 36),
                border_color: Color32::from_rgb(55, 55, 65),
                text_normal: Color32::from_rgb(242, 242, 247),
                text_muted: Color32::from_rgb(158, 158, 170),
                accent: Color32::from_rgb(59, 130, 246),
                accent_hover: Color32::from_rgb(96, 165, 250),
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
}
```

---

## 4. Implementation Steps

### Step 1: Update Settings & Theme Enumeration
Modify `src/settings.rs`:
* Update `pub enum Theme` with the new variants.
* Change default serialization renaming behavior if needed, ensuring it parses correctly from `settings.toml`.
* Ensure `Theme` is exported and derives `Copy`, `Clone`, `PartialEq`, `Eq`, `Serialize`, `Deserialize`.

### Step 2: Global Egui Style Application
Modify `configure_theme` in `src/ui/mod.rs` to take the theme:
```rust
fn configure_theme(ctx: &egui::Context, theme: Theme) {
    let palette = theme.palette();
    let mut style = (*ctx.global_style()).clone();
    
    let mut visuals = if palette.is_dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    
    visuals.window_fill = palette.panel_bg;
    visuals.panel_fill = palette.panel_bg;
    
    visuals.widgets.noninteractive.bg_fill = palette.card_bg;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, palette.border_color);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, palette.text_normal);

    visuals.widgets.inactive.bg_fill = palette.widget_inactive_bg;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, palette.border_color);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, palette.text_normal);

    visuals.widgets.hovered.bg_fill = palette.widget_hovered_bg;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, palette.accent);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, if palette.is_dark { Color32::WHITE } else { Color32::BLACK });

    visuals.widgets.active.bg_fill = palette.accent;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, palette.accent_hover);
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, if palette.is_dark { Color32::WHITE } else { Color32::BLACK });

    visuals.selection.bg_fill = palette.accent;
    visuals.selection.stroke = egui::Stroke::new(1.0, palette.accent_hover);

    style.visuals = visuals;
    ctx.set_global_style(style);
}
```

### Step 3: Replace Hardcoded UI Colors
Refactor `src/ui/mod.rs` to retrieve and reference `self.settings.appearance.theme.palette()` (or retrieve it using `get_palette(theme)`) instead of using raw `Color32` constants. The theme will apply to all main UI components:

1. **Layout Panels:**
   - Bottom Panel (`timeline_deck`): Background fill maps to `palette.panel_bg`.
   - Right Panel (`sidebar_deck`): Background fill maps to `palette.card_bg`, and border stroke maps to `palette.border_color`.
   - Central Panel (Viewport container): Background fill maps to `palette.panel_bg`.

2. **Typography & Accents:**
   - Panel Titles (e.g. `snippets`, `snippy` wordmark): Colors map to `palette.text_normal`.
   - Decorative Brackets (e.g. `[` and `]` surrounding titles and IDs): Colors map to `palette.accent`.
   - Subtext and Muted Labels (e.g. filename text, duration labels): Colors map to `palette.text_muted`.

3. **Filmstrip Component:**
   - Filmstrip track rect fill maps to `palette.timeline_track_bg`.
   - Filmstrip border stroke maps to `palette.border_color`.
   - Filmstrip sprocket holes (top and bottom) map to `palette.panel_bg`.
   - Thumbnail borders and placeholder backgrounds map to `palette.border_color` and `palette.timeline_track_bg`.
   - Loading indicator arc maps to `palette.accent`.

4. **Waveform Deck & Visualizer:**
   - Waveform deck panel fill/stroke map to `palette.card_bg` and `palette.border_color`.
   - Waveform background fill maps to `palette.timeline_track_bg`.
   - Active waveform bars map to `palette.accent`.
   - Muted/background waveform bars map to `palette.accent.linear_multiply(0.35)` or `palette.text_muted`.

5. **Markers, Playhead & Scissor Icons:**
   - Playhead circular handle and vertical playback line map to `palette.accent`.
   - Scissor tool icon color maps to `palette.accent`.
   - In-marker bracket and line map to `palette.warn_color`.
   - Out-marker bracket and line map to `palette.accent` (or `palette.warn_color`).

6. **Snippet List Items:**
   - Snippet cards inactive background and border map to `palette.widget_inactive_bg` and `palette.border_color`.
   - Snippet cards hovered/active states map to `palette.widget_hovered_bg` / `palette.accent` and `palette.border_color`.
   - Mode indicators (Cut/Keep) map to `palette.cut_color` and `palette.keep_color`.

7. **Telemetry HUD:**
   - Background panel maps to `palette.panel_bg.linear_multiply(0.9)` or `palette.card_bg.linear_multiply(0.9)`.
   - Borders and text fields map to `palette.border_color`, `palette.text_normal`, and `palette.accent`.
   - Active status lights (e.g. green for playing, red for lagging) map to `Color32::LIGHT_GREEN` and `Color32::LIGHT_RED`.

### Step 4: Settings ComboBox & Real-time Update
Modify `draw_settings_modal`:
* Replace the hardcoded `Theme` label and description with an `egui::ComboBox` selecting from the `Theme` variants.
* Upon modal completion:
  ```rust
  if close_modal {
      let old_theme = self.settings.appearance.theme;
      self.settings = self.pending_settings.clone();
      self.settings.save();
      self.show_settings_modal = false;
      if old_theme != self.settings.appearance.theme {
          configure_theme(ctx, self.settings.appearance.theme);
      }
  }
  ```

---

## 5. Verification Plan
* **Build Verification:** Ensure the project compiles successfully with `cargo build`.
* **Theme Repaint Check:** Test settings changes to confirm visual variables update instantly in eframe.
* **Contrast Contrast Ratios:** Confirm contrast is above 4.5:1 for normal elements and 7:1 for text elements.
