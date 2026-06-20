# Timeline Height & Persistence Refinements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refine the timeline bottom panel layout to default to 32px waveform height, 48px filmstrip frame height, and restore timeline height persistence when resizing.

**Architecture:** 
1. Set the default `timeline_height` in `MainApp::new` to `164.0` in `src/ui/mod.rs`.
2. Update the `timeline_deck` panel size range to `130.0..=300.0`.
3. Set `wave_height = 32.0` in `src/ui/mod.rs` (updating both the filmstrip offset calculation and the rendering loop).
4. Bind `self.timeline_height` to the panel's actual height while dragging:
   ```rust
   let new_height = panel_response.response.rect.height();
   if new_height != self.timeline_height && ui.ctx().input(|i| i.pointer.any_down()) {
       self.timeline_height = new_height;
   }
   ```

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Update Panel Layout and Height Persistence

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Set default timeline_height to 164.0**
  Modify `timeline_height` in `MainApp::new` (around line 120) to default to `164.0`.
  ```rust
  timeline_height: 164.0,
  ```

- [ ] **Step 2: Assign panel_response and update size range**
  Modify the bottom panel instantiation (around line 1113-1116) to capture `panel_response` and change the size range to `130.0..=300.0`:
  ```rust
          let panel_response = egui::Panel::bottom("timeline_deck")
              .resizable(true)
              .default_size(self.timeline_height)
              .size_range(130.0..=300.0)
  ```

- [ ] **Step 3: Update wave_height to 32.0 in filmstrip size calculation**
  Modify the `wave_height` variable in the `filmstrip_height` calculation block (around line 1159):
  ```rust
                      let wave_height = 32.0;
  ```

- [ ] **Step 4: Update wave_height to 32.0 in drawing block**
  Modify the local `wave_height` variable in the waveform rendering block (around line 1431):
  ```rust
                      // 3. Compact Synced Waveform Component
                      let wave_height = 32.0;
  ```

- [ ] **Step 5: Add logic to update self.timeline_height during drag**
  Right after the bottom panel's block ends (around line 1592), add the logic to update `self.timeline_height` when dragging:
  ```rust
          let new_height = panel_response.response.rect.height();
          if new_height != self.timeline_height && ui.ctx().input(|i| i.pointer.any_down()) {
              self.timeline_height = new_height;
          }
  ```

- [ ] **Step 6: Verify cargo check compiles without errors**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 7: Run tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 8: Commit changes to git**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "fix: adjust waveform to 32px, filmstrip to 48px, and restore height persistence"
  ```
