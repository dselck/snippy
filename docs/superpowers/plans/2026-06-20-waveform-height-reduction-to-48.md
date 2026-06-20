# Waveform Height Adjustment to 48px Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Adjust the audio waveform height to 48px (doubled from 24px) and update the timeline panel layout constraints to match.

**Architecture:** 
1. Set the default `timeline_height` in `MainApp::new` to `180.0` (in `src/ui/mod.rs`).
2. Update the `timeline_deck` panel size range to `130.0..=300.0` (in `src/ui/mod.rs`).
3. Set `wave_height = 48.0` in `src/ui/mod.rs` (updating both the filmstrip offset calculation and the rendering loop).

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Update Panel Constraints and Waveform Height

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Set timeline_height default to 180.0**
  Modify `timeline_height` in `MainApp::new` (around line 120) to default to `180.0`.
  ```rust
  timeline_height: 180.0,
  ```

- [ ] **Step 2: Update size range in timeline_deck Panel to 130.0..=300.0**
  Modify the bottom panel instantiation (around line 1113-1116) to change the allowed size range:
  ```rust
          egui::Panel::bottom("timeline_deck")
              .resizable(true)
              .default_size(self.timeline_height)
              .size_range(130.0..=300.0)
  ```

- [ ] **Step 3: Update wave_height to 48.0 in filmstrip size calculation**
  Modify the `wave_height` variable in the `filmstrip_height` calculation block (around line 1159):
  ```rust
                      let wave_height = 48.0;
  ```

- [ ] **Step 4: Update wave_height to 48.0 in drawing block**
  Modify the local `wave_height` variable in the waveform rendering block (around line 1431):
  ```rust
                      // 3. Compact Synced Waveform Component
                      let wave_height = 48.0;
  ```

- [ ] **Step 5: Verify cargo check compiles without errors**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 6: Run tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 7: Commit changes to git**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "style: reduce waveform height to 48px and adjust timeline constraints"
  ```
