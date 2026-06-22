# Themeable Color Palette Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a themeable color palette system for Snippy that supports 6 selectable light/dark themes, improves contrast, and repaints the application in real-time when the settings are changed.

**Architecture:** We will update the `Theme` enum in `src/settings.rs` to include the 6 theme options, mapping them to a centralized `ThemePalette` struct defined in `src/ui/theme.rs`. We will update `configure_theme` to load the current theme's palette, apply it globally, and replace all hardcoded color calls in the UI with the active palette's colors.

**Tech Stack:** Rust, egui, eframe

---

### Task 1: Update Theme Enum in settings.rs

**Files:**
* Modify: `src/settings.rs`

- [ ] **Step 1: Write the updated Theme enum**
  Modify the `Theme` enum definition in `src/settings.rs` around line 43. Replace it with the new theme variants and add `#[serde(alias = "dark")]` to the default `VideoEditorDark` for backward compatibility.
  ```rust
  #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
  #[serde(rename_all = "snake_case")]
  pub enum Theme {
      #[serde(alias = "dark")]
      VideoEditorDark,
      SleekMidnight,
      HighContrastOled,
      ClassicIndigoDark,
      VideoEditorLight,
      ClassicWarmLight,
  }

  impl Default for Theme {
      fn default() -> Self {
          Theme::VideoEditorDark
      }
  }
  ```

- [ ] **Step 2: Verify compilation**
  Run: `cargo check`
  Expected: Successful compilation (or simple warnings about unused fields).

- [ ] **Step 3: Commit**
  ```bash
  git add src/settings.rs
  git commit -m "feat: expand Theme enum with dark and light variants"
  ```

---

### Task 2: Create ThemePalette and Theme mappings in theme.rs

**Files:**
* Create: `src/ui/theme.rs`
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create theme.rs**
  Create the file `src/ui/theme.rs` and write the `ThemePalette` definition and `get_palette` mapping function.
  ```rust
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

  pub fn get_palette(theme: Theme) -> ThemePalette {
      match theme {
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
  ```

- [ ] **Step 2: Register theme module in mod.rs**
  At the top of `src/ui/mod.rs` (around line 10), register the `theme` module and export the items:
  ```rust
  pub mod theme;
  pub use theme::{get_palette, ThemePalette};
  ```

- [ ] **Step 3: Commit**
  ```bash
  git add src/ui/theme.rs src/ui/mod.rs
  git commit -m "feat: implement ThemePalette structure and registration"
  ```

---

### Task 3: Refactor configure_theme to apply palettes dynamically

**Files:**
* Modify: `src/ui/mod.rs:78-108`, `src/ui/mod.rs:410-462`

- [ ] **Step 1: Modify configure_theme definition**
  Change the function signature and implementation of `configure_theme` in `src/ui/mod.rs` to take the active `Theme`:
  ```rust
  fn configure_theme(ctx: &egui::Context, theme: crate::settings::Theme) {
      let palette = get_palette(theme);
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

      // Global layout roundings
      visuals.window_corner_radius = egui::CornerRadius::same(12);
      visuals.window_shadow = egui::epaint::Shadow {
          offset: [0, 8],
          blur: 15,
          spread: 0,
          color: Color32::from_black_alpha(40),
      };

      style.visuals = visuals;
      style.spacing.item_spacing = egui::vec2(10.0, 10.0);
      style.spacing.button_padding = egui::vec2(12.0, 8.0);
      ctx.set_global_style(style);
  }
  ```

- [ ] **Step 2: Update MainApp::new initialization order**
  Modify `MainApp::new` to load settings before configuring the theme:
  ```rust
      pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
          configure_fonts(&cc.egui_ctx);

          // Load settings from disk; push any load error as a Notification.
          let (settings, load_err) = Settings::load();

          // Configure theme using loaded settings
          configure_theme(&cc.egui_ctx, settings.appearance.theme);
  ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: Successful compilation.

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: refactor configure_theme to apply theme palettes dynamically"
  ```

---

### Task 4: Replace hardcoded colors in mod.rs with palette references

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace settings modal hardcoded colors**
  Modify lines in `draw_settings_modal` (around lines 155-159) to load the active palette:
  ```rust
          let palette  = get_palette(self.settings.appearance.theme);
          let accent   = palette.accent;
          let muted    = palette.text_muted;
          let warn_col = palette.warn_color;
          let card_bg  = palette.card_bg;
          let sep_col  = palette.border_color;
  ```

- [ ] **Step 2: Replace modal window background fill**
  In the modal's `egui::Window::new` call (around line 176), update the fill to use the active palette's background color:
  ```rust
              .frame(egui::Frame::window(&ctx.global_style()).fill(palette.panel_bg).stroke(egui::Stroke::new(1.0, sep_col)))
  ```

- [ ] **Step 3: Replace status toggle colors**
  In the cut/keep capsule toggle button `status_toggle` (around lines 710-713):
  ```rust
      let palette = get_palette(self.settings.appearance.theme);
      let bg_color = if *cut_mode {
          palette.cut_color
      } else {
          palette.keep_color
      };
  ```

- [ ] **Step 4: Replace playback controls button colors**
  In `playback_control_button` (around lines 748-782), resolve the active palette and replace hardcoded button colors:
  ```rust
      let palette = get_palette(self.settings.appearance.theme);
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
  ```

- [ ] **Step 5: Replace timeline and snippet track colors**
  Find the timeline rendering code (around lines 828, 835, 847, 883, 944, 950) and update with the active theme colors:
  * For background (line 828):
    ```rust
                    palette.timeline_track_bg,
    ```
  * For rest/snippet backgrounds (line 835-850):
    Use `palette.timeline_snippet_cut_bg` or `palette.timeline_snippet_keep_bg`.
  * For markers (line 944, 950):
    Use `palette.warn_color` or separate marker indicators.

- [ ] **Step 6: Verify build**
  Run: `cargo build`
  Expected: Successful compilation without warnings.

- [ ] **Step 7: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "refactor: replace hardcoded UI colors with ThemePalette lookups"
  ```

---

### Task 5: Add Theme ComboBox to settings modal and trigger repaints

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace Appearance settings item with ComboBox**
  Modify the `Appearance` settings section in `draw_settings_modal` (around lines 237-248) to show a selectable ComboBox:
  ```rust
                      // ── Appearance ────────────────────────────────────────
                      ui.label(egui::RichText::new("Appearance").size(13.0).color(accent).strong());
                      egui::Frame::NONE.fill(card_bg).corner_radius(8.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                          ui.set_width(452.0);
                          ui.horizontal(|ui| {
                              ui.label("Theme");
                              ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                  egui::ComboBox::from_id_salt("theme_selector")
                                      .selected_text(match self.pending_settings.appearance.theme {
                                          Theme::VideoEditorDark => "Video Editor Dark",
                                          Theme::SleekMidnight => "Sleek Midnight",
                                          Theme::HighContrastOled => "High-Contrast OLED",
                                          Theme::ClassicIndigoDark => "Classic Indigo Dark",
                                          Theme::VideoEditorLight => "Video Editor Light",
                                          Theme::ClassicWarmLight => "Classic Warm Light",
                                      })
                                      .show_ui(ui, |ui| {
                                          ui.selectable_value(&mut self.pending_settings.appearance.theme, Theme::VideoEditorDark, "Video Editor Dark");
                                          ui.selectable_value(&mut self.pending_settings.appearance.theme, Theme::SleekMidnight, "Sleek Midnight");
                                          ui.selectable_value(&mut self.pending_settings.appearance.theme, Theme::HighContrastOled, "High-Contrast OLED");
                                          ui.selectable_value(&mut self.pending_settings.appearance.theme, Theme::ClassicIndigoDark, "Classic Indigo Dark");
                                          ui.selectable_value(&mut self.pending_settings.appearance.theme, Theme::VideoEditorLight, "Video Editor Light");
                                          ui.selectable_value(&mut self.pending_settings.appearance.theme, Theme::ClassicWarmLight, "Classic Warm Light");
                                      });
                              });
                          });
                      });
  ```

- [ ] **Step 2: Add theme repaint trigger on Settings commit**
  Modify settings commit logic in `draw_settings_modal` (around lines 375-380) to repaint the application instantly when a theme changes:
  ```rust
          if close_modal {
              let theme_changed = self.settings.appearance.theme != self.pending_settings.appearance.theme;
              // Commit pending settings → live settings
              self.settings = self.pending_settings.clone();
              self.settings.save();
              self.show_settings_modal = false;
              if theme_changed {
                  configure_theme(ctx, self.settings.appearance.theme);
              }
          }
  ```

- [ ] **Step 3: Run final app build and check**
  Run: `cargo build`
  Expected: Success.

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: add theme selection ComboBox and trigger dynamic repaint"
  ```
