# Timeline Spacing and Drag Tracking Refinement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct the layout feedback loop by implementing pixel-perfect spacing calculations and a proximity-based cursor drag guard.

**Architecture:** 
1. Modify `src/ui/mod.rs` to compute `reserved_for_waveform` exactly matching the widgets and spaces drawn after the filmstrip.
2. Modify `src/ui/mod.rs` to only update `self.timeline_height` if the pointer y-coordinate is within 15.0 pixels of the top edge of the panel when `any_down()` is true.

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Refine Layout Math and Height Persistence Logic

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Update reserved_for_waveform calculation**
  Modify the `filmstrip_height` calculation block in `src/ui/mod.rs` (around line 1159) to exactly sum the space and widgets below the filmstrip:
  ```rust
                      let total_width = ui.available_width();
                      let spacing_y = ui.spacing().item_spacing.y;
                      let wave_height = 32.0;
                      let reserved_for_waveform = spacing_y + 6.0 + spacing_y + wave_height;
                      let filmstrip_height = (ui.available_height() - reserved_for_waveform).max(30.0);
  ```

- [ ] **Step 2: Add proximity-based guard to timeline height persistence**
  Modify the drag-tracking code block in `src/ui/mod.rs` (around line 1595) to check distance from the top border of the panel:
  ```rust
          let new_height = panel_response.response.rect.height();
          if new_height != self.timeline_height && ui.ctx().input(|i| i.pointer.any_down()) {
              if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                  let dist_to_top = (pos.y - panel_response.response.rect.top()).abs();
                  if dist_to_top < 15.0 {
                      self.timeline_height = new_height;
                  }
              }
          }
  ```

- [ ] **Step 3: Verify cargo check compiles without errors**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 4: Run tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 5: Commit changes to git**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "fix: refine timeline layout spacing and add proximity-based drag tracking check"
  ```
