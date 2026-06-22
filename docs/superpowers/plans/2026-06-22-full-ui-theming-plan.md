# Full UI Theming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Theme all remaining hardcoded color definitions in the main layout, sidebar, timeline, sprockets, playhead, waveform visualizer, markers, and telemetry HUD to reference the active `ThemePalette`.

**Architecture:** We will load the current palette at the start of the `ui` rendering method in `src/ui/mod.rs` and distribute it to all child panels, widgets, and custom drawing callbacks.

**Tech Stack:** Rust, egui, eframe

---

### Task 1: Theme Layout Panels & General Typography

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Retrieve active palette in `ui` method**
  At the very start of the `ui` method in `src/ui/mod.rs` (around line 1017), retrieve the active palette:
  ```rust
      fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
          let palette = get_palette(self.settings.appearance.theme);
          let ctx = ui.ctx().clone();
  ```

- [ ] **Step 2: Update Layout Panels styling**
  * In the bottom panel (`timeline_deck` around line 1137), replace fill with `palette.panel_bg`:
    ```rust
                    .fill(palette.panel_bg)
    ```
  * In the right panel (`sidebar_deck` around line 1635), replace fill with `palette.card_bg` and stroke with `palette.border_color`:
    ```rust
                    .fill(palette.card_bg)
                    .stroke(egui::Stroke::new(1.0, palette.border_color))
    ```
  * In the central panel (`CentralPanel` around line 1749), replace fill with `palette.panel_bg`:
    ```rust
                    .fill(palette.panel_bg)
    ```

- [ ] **Step 3: Update Typography and Bracket Accents**
  * Update sidebar header brackets (lines 1647-1649) to use `palette.accent` and `palette.text_normal`:
    ```rust
                            ui.label(egui::RichText::new("[").strong().color(palette.accent).size(18.0));
                            ui.label(egui::RichText::new("snippets").strong().color(palette.text_normal).size(18.0));
                            ui.label(egui::RichText::new("]").strong().color(palette.accent).size(18.0));
    ```
  * Update Central Panel title and brackets (lines 1762, 1769, 1784) to use `palette.accent` and `palette.text_normal`:
    ```rust
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
                            // Right Bracket ]
                            ui.label(
                                egui::RichText::new("]")
                                    .font(egui::FontId::new(20.0, egui::FontFamily::Name("medium".into())))
                                    .color(palette.accent)
                            );
    ```
  * Update filename label (line 1796) and Settings gear icon (line 1807):
    ```rust
                                    .color(palette.text_muted)
    ```
    and
    ```rust
                                    egui::RichText::new("⚙").size(18.0).color(palette.text_muted)
    ```

- [ ] **Step 4: Verify compilation**
  Run: `cargo check`
  Expected: Success.

- [ ] **Step 5: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: theme layout panels, buttons, and typography"
  ```

---

### Task 2: Theme Filmstrip & Sprockets

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace Filmstrip track background, borders, and sprockets**
  * Update filmstrip rect fill and stroke (around lines 1192-1193):
    ```rust
                        painter.rect_filled(filmstrip_rect, 6.0, palette.timeline_track_bg);
                        painter.rect_stroke(filmstrip_rect, 6.0, egui::Stroke::new(1.0, palette.border_color), egui::StrokeKind::Inside);
    ```
  * Update sprocket hole fill (around lines 1225, 1232):
    ```rust
                            painter.rect_filled(top_sprocket_rect, 1.0, palette.panel_bg);
    ```
    and
    ```rust
                            painter.rect_filled(bottom_sprocket_rect, 1.0, palette.panel_bg);
    ```

- [ ] **Step 2: Update Thumbnail frame backgrounds & borders**
  * Replace loading placeholder text colors (around lines 1282, 1284):
    ```rust
                                palette.text_muted // Dimmed
    ```
    and
    ```rust
                                palette.text_normal
    ```
  * Update thumbnail backgrounds (around lines 1315-1317):
    ```rust
                                    palette.timeline_track_bg
    ```
  * Update thumbnail border stroke (around line 1320):
    ```rust
                                painter.rect_stroke(thub_rect, 4.0, egui::Stroke::new(1.0, palette.border_color), egui::StrokeKind::Inside);
    ```
  * Update loading progress arc color (around line 1329):
    ```rust
                                    let arc_color = palette.accent;
    ```
  * Update in-flight selection overlay and border (around lines 1354, 1371):
    ```rust
                                        palette.cut_color.linear_multiply(0.2),
    ```
    and
    ```rust
                                        egui::Stroke::new(2.0, palette.accent),
    ```
  * Update floating timecode labels (around line 1386):
    ```rust
                                    palette.text_muted,
    ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: Success.

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: theme filmstrip track, sprockets, and thumbnails"
  ```

---

### Task 3: Theme Waveform Deck & Visualizer

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace Waveform container fill and stroke**
  * Update Waveform container fill and stroke (around lines 1465-1466):
    ```rust
                            palette.card_bg,
                            egui::Stroke::new(1.0, palette.border_color),
    ```

- [ ] **Step 2: Update Waveform background and bars**
  * Update Waveform background fill (around line 1545):
    ```rust
                                    palette.timeline_track_bg
    ```
  * Update Waveform bars colors (around lines 1547-1549):
    ```rust
                                    palette.accent.linear_multiply(0.35)
    ```
    and
    ```rust
                                    palette.accent
    ```

- [ ] **Step 3: Verify compilation**
  Run: `cargo check`
  Expected: Success.

- [ ] **Step 4: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: theme waveform deck and audio visualizer"
  ```

---

### Task 4: Theme Playhead, Scissor Icons, Markers, and Telemetry HUD

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Theme playhead and scissors**
  * Update filmstrip playhead handle (around line 985):
    ```rust
                palette.accent, // Playhead handle
    ```
  * Update vertical playhead line (around line 1448):
    ```rust
                            palette.accent,
    ```
  * Update scissor icon colors (around lines 1653 and 1776):
    ```rust
                            draw_scissor_icon(&ui.painter_at(scissor_rect), scissor_rect, palette.accent);
    ```

- [ ] **Step 2: Theme timeline markers**
  * Update timeline marker flag colors (around lines 1599, 1602):
    ```rust
                            draw_timeline_marker(&ui.painter(), inf, palette.warn_color, 1.0);
    ```
    and
    ```rust
                            draw_timeline_marker(&ui.painter(), outf, palette.accent, -1.0);
    ```
  * Update timeline subtext color (around line 1610):
    ```rust
                                    .color(palette.text_muted)
    ```

- [ ] **Step 3: Theme Telemetry HUD**
  * Update Telemetry HUD background and border (around lines 2023-2024):
    ```rust
                                                    palette.panel_bg.linear_multiply(0.9)
                                                    egui::Stroke::new(1.0, palette.border_color)
    ```
  * Update Telemetry HUD grid line stroke (around line 2068):
    ```rust
                                    egui::Stroke::new(1.0, palette.accent.linear_multiply(0.1)),
    ```
  * Update Telemetry stats card background and border (around lines 2084-2085):
    ```rust
                                            .fill(palette.card_bg)
                                            .stroke(egui::Stroke::new(1.0, palette.border_color))
    ```
  * Update Telemetry subtext and title colors (around lines 2059, 2091):
    ```rust
                                            palette.text_muted,
    ```
    and
    ```rust
                                                        ui.label(egui::RichText::new("⚡ AV SYNC TELEMETRY").strong().color(palette.accent_hover));
    ```

- [ ] **Step 4: Verify compilation**
  Run: `cargo check`
  Expected: Success.

- [ ] **Step 5: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: theme playhead, markers, scissor icons, and telemetry HUD"
  ```

---

### Task 5: Theme Snippet List Items

**Files:**
* Modify: `src/ui/mod.rs`

- [ ] **Step 1: Theme snippet card label and mode switch indicator**
  * Update the action label color (around line 1668):
    ```rust
                                let label_color = if self.cut_mode { palette.cut_color } else { palette.keep_color };
    ```
  * Update the bracket decorations and snippet ID text (around lines 1689-1691):
    ```rust
                                            ui.label(egui::RichText::new("[").strong().color(palette.accent));
                                            ui.label(egui::RichText::new(format!("{:02}", range.id)).strong().color(palette.text_normal));
                                            ui.label(egui::RichText::new("]").strong().color(palette.accent));
    ```
  * Update range timecode text color (around line 1696):
    ```rust
                                        ui.label(egui::RichText::new(time_str).monospace().color(palette.text_normal));
    ```
  * Update card background fill (around line 1706):
    ```rust
                                        painter.rect_filled(rect, 2.0, palette.widget_inactive_bg);
    ```
  * Update mode badge color (around lines 1721-1723):
    ```rust
                                                palette.cut_color // cut is red
                                                palette.keep_color // keep is green
    ```

- [ ] **Step 2: Run final build and tests**
  Run: `cargo test`
  Expected: All tests pass.

- [ ] **Step 3: Commit**
  ```bash
  git add src/ui/mod.rs
  git commit -m "feat: theme snippet list items and run test suite"
  ```
