# Timeline Height Feedback Loop Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the timeline panel height feedback loop by replacing the pointer input `any_down()` check with a panel `dragged()` check.

**Architecture:** 
1. Modify `src/ui/mod.rs` to only overwrite `self.timeline_height` when `panel_response.response.dragged()` is `true`.

**Tech Stack:** Rust, egui 0.34.3

---

### Task 1: Update Height Persistence Check

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace any_down() check with panel_response.response.dragged()**
  Modify the drag-tracking condition in `src/ui/mod.rs` (around line 1595):

  Replace:
  ```rust
          let new_height = panel_response.response.rect.height();
          if new_height != self.timeline_height && ui.ctx().input(|i| i.pointer.any_down()) {
              self.timeline_height = new_height;
          }
  ```

  With:
  ```rust
          let new_height = panel_response.response.rect.height();
          if panel_response.response.dragged() {
              self.timeline_height = new_height;
          }
  ```

- [ ] **Step 2: Verify cargo check compiles without errors**
  Run: `cargo check`
  Expected: PASS

- [ ] **Step 3: Run tests**
  Run: `cargo test`
  Expected: PASS

- [ ] **Step 4: Commit changes to git**
  Run:
  ```bash
  git add src/ui/mod.rs
  git commit -m "fix: update timeline panel height only when dragging its resize handle"
  ```
