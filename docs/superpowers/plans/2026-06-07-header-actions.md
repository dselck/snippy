# Header Load & Export Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the sidebar bottom "🚀 Export All Snippets" button and add "Load Video" and "Export Video" buttons to the main viewport header (right-justified) using custom vector shapes and tooltips.

**Architecture:**
- Extend the `PlaybackIcon` enum with `LoadVideo` and `ExportVideo` variants.
- Implement vector drawing inside `draw_playback_icon` for these new variants (tray and arrow designs).
- Update the layout inside `src/ui/mod.rs` to right-align the new header buttons, while keeping the logo and title left-aligned.
- Delete the bottom-up sidebar panel code and its container code.

**Tech Stack:** Rust, egui

---

### Task 1: Extend PlaybackIcon Enum and Implement Vector Icons

**Files:**
- Modify: `src/ui/mod.rs:12-23`
- Modify: `src/ui/mod.rs:302-315`

- [ ] **Step 1: Extend `PlaybackIcon`**

Modify `PlaybackIcon` to include `LoadVideo` and `ExportVideo`.

```rust
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
```

- [ ] **Step 2: Add vector drawing paths inside `draw_playback_icon`**

Modify `draw_playback_icon` in `src/ui/mod.rs` after the `PlaybackIcon::AddSnippet` match branch to draw the tray & arrow icons:

```rust
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
```

- [ ] **Step 3: Run cargo check to verify compilation**

Run: `cargo check`
Expected: Passes compile (with dead code warnings on new enum variants)

- [ ] **Step 4: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: extend PlaybackIcon and draw vector tray/arrow glyphs"
```

---

### Task 2: Remove Sidebar bottom "Export All Snippets" Button

**Files:**
- Modify: `src/ui/mod.rs:841-850`

- [ ] **Step 1: Delete bottom_up code**

Locate the following block inside `egui::SidePanel::right` show closure and remove it:

```rust
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                        ui.add_space(10.0);
                        let btn = ui.add_enabled(
                            !self.ranges.is_empty(),
                            egui::Button::new("🚀 Export All Snippets").min_size(egui::vec2(ui.available_width(), 42.0))
                        ).on_hover_text("Export all queued snippets to separate video files");
                        if btn.clicked() {
                            // Trigger Batch Export
                        }
                    });
```

- [ ] **Step 2: Run cargo check to verify**

Run: `cargo check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/ui/mod.rs
git commit -m "style: remove bottom export all snippets button from sidebar"
```

---

### Task 3: Implement Right-Justified Load and Export buttons in the Central Panel Header

**Files:**
- Modify: `src/ui/mod.rs:861-902`

- [ ] **Step 1: Rewrite header horizontal layout to align buttons to the right**

Change the header display in the `CentralPanel` closure. Keep the left-aligned elements inside a left-aligned container, then use `ui.with_layout(egui::Layout::right_to_left(egui::Align::Center))` to draw the two circular header action buttons.

```rust
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
                            ui.label(
                                egui::RichText::new(format!("—  {}", path))
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
                                    "Export all snippets to separate video files",
                                ) {
                                    println!("Export clicked: exporting {} snippets", self.ranges.len());
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
                                println!("Load clicked: loading new video");
                            }
                        });
                    });
```

- [ ] **Step 2: Run cargo check and run app to test**

Run: `cargo check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: add load/export header buttons to viewport deck"
```
