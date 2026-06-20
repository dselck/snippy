# Timeline Drag Persistence Refinement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refine the timeline panel height dragging logic using a persistent boolean state flag (`is_resizing_timeline`) to prevent snapping/drifts during fast drags.

**Architecture:** 
1. Add `is_resizing_timeline` state to `MainApp` (in `src/ui/mod.rs`).
2. Track when a click starts on the top border (within 15px), and continue updating `self.timeline_height` as long as `any_down()` remains true.

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Add is_resizing_timeline Flag and Refine Drag Tracking

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add is_resizing_timeline to MainApp struct**
  Add the new boolean field to the struct (around line 66):
  ```rust
      pub is_resizing_timeline: bool,
  ```

- [ ] **Step 2: Initialize is_resizing_timeline to false in MainApp::new**
  Modify the `Self` builder in `MainApp::new` (around line 130) to include the new field:
  ```rust
              is_dragging_filmstrip: false,
              is_resizing_timeline: false,
  ```

- [ ] **Step 3: Update bottom panel drag tracking logic**
  Replace the drag-persistence update block (around line 1595):

  Replace:
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

  With:
  ```rust
          let new_height = panel_response.response.rect.height();

          if ui.ctx().input(|i| i.pointer.any_pressed()) {
              if let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) {
                  let dist_to_top = (pos.y - panel_response.response.rect.top()).abs();
                  if dist_to_top < 15.0 {
                      self.is_resizing_timeline = true;
                  }
              }
          }

          if self.is_resizing_timeline {
              if ui.ctx().input(|i| i.pointer.any_down()) {
                  self.timeline_height = new_height;
              } else {
                  self.is_resizing_timeline = false;
              }
          }
  ```

- [ ] **Step 4: Verify cargo check compiles without errors**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 5: Run tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 6: Commit changes to git**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "fix: implement persistent drag state for stable timeline panel resizing"
  ```
