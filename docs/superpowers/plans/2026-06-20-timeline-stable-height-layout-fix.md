# Stable Timeline Height Layout Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Permanently fix the timeline panel feedback loop by deriving the filmstrip height from the stable `timeline_height` state instead of `available_height()`.

**Architecture:** 
1. Set the default `timeline_height` to `192.0` in `MainApp::new` to give `48.0` pixels of default thumbnail height.
2. Update the resizable bottom panel size range to `160.0..=350.0`.
3. In `src/ui/mod.rs`, calculate `filmstrip_height` directly using `self.timeline_height`:
   ```rust
   let filmstrip_height = (self.timeline_height - 128.0).max(30.0);
   ```

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Update Heights to Stable State-based Formula

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Set default timeline_height to 192.0 in MainApp::new**
  Modify the `timeline_height` default (around line 120):
  ```rust
  timeline_height: 192.0,
  ```

- [ ] **Step 2: Update size range in timeline_deck Panel to 160.0..=350.0**
  Modify the bottom panel instantiation (around line 1113-1116) to change the allowed size range:
  ```rust
          let panel_response = egui::Panel::bottom("timeline_deck")
              .resizable(true)
              .default_size(self.timeline_height)
              .size_range(160.0..=350.0)
  ```

- [ ] **Step 3: Calculate filmstrip_height using stable state height**
  Modify the `filmstrip_height` calculation block (around line 1159) to use the stable `self.timeline_height`:
  ```rust
                      let total_width = ui.available_width();
                      let wave_height = 32.0;
                      let filmstrip_height = (self.timeline_height - 128.0).max(30.0);
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
  git commit -m "fix: resolve timeline layout feedback loop by calculating filmstrip height from state height"
  ```
