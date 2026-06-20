# Timeline Layout Correction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct the timeline layout constraints in the bottom panel to accommodate the 72px waveform, preventing it from overflowing below the screen.

**Architecture:** 
1. Set the default `timeline_height` in `MainApp::new` to `220.0` (in `src/ui/mod.rs`).
2. Update the `timeline_deck` panel size range to `150.0..=350.0` (in `src/ui/mod.rs`).
3. Settle on a uniform `wave_height = 72.0` layout, and calculate the `filmstrip_height` dynamically by subtracting `wave_height + 12.0` (the reserved space for the waveform, text, and margins) from the available height.

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Update Panel Constraints and filmstrip_height Calculation

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Update timeline_height default in MainApp::new**
  Modify the `MainApp::new` initialization (around line 120) to default `timeline_height` to `220.0`.
  ```rust
  timeline_height: 220.0,
  ```

- [ ] **Step 2: Update size range in timeline_deck Panel**
  Modify the bottom panel instantiation (around line 1113-1116) to change the allowed size range:
  ```rust
          egui::Panel::bottom("timeline_deck")
              .resizable(true)
              .default_size(self.timeline_height)
              .size_range(150.0..=350.0)
  ```

- [ ] **Step 3: Calculate filmstrip_height dynamically**
  Define `wave_height` and use it to compute the remaining available height for the filmstrip, instead of subtracting `30.0` (around line 1159):
  ```rust
                      let total_width = ui.available_width();
                      let spacing_y = ui.spacing().item_spacing.y;
                      let wave_height = 72.0;
                      let reserved_for_waveform = wave_height + 12.0;
                      let filmstrip_height = (ui.available_height() - reserved_for_waveform - spacing_y).max(30.0);
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
  git commit -m "fix: adjust timeline panel size constraints and dynamically calculate filmstrip height"
  ```
