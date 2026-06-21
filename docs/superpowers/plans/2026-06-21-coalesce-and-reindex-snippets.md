# Coalesce and Re-index Snippets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ensure snippet counts are always 1..N (re-indexed on add/delete) and overlapping/adjacent selections are coalesced automatically without gaps.

**Architecture:** We will implement a standalone helper function `coalesce_and_reindex_ranges` in the UI module. This helper will sort ranges by their starting frame, merge overlapping/contiguous ranges, and re-assign IDs. We will integrate it into the add/delete event handlers in the UI and test the logic with new unit tests.

**Tech Stack:** Rust, egui

---

### Task 1: Add Coalesce and Re-indexing Helper and Unit Tests

**Files:**
- Modify: `src/ui/mod.rs`
- Test: `src/ui/mod.rs` (under `mod tests`)

- [ ] **Step 1: Write the helper function**
Add the `coalesce_and_reindex_ranges` helper function at the bottom of `src/ui/mod.rs` (just above the `tests` module).

Code:
```rust
pub fn coalesce_and_reindex_ranges(ranges: &mut Vec<ExportRange>) {
    if ranges.is_empty() {
        return;
    }
    
    // Sort ranges by start frame to ensure left-to-right evaluation
    ranges.sort_by_key(|r| r.in_frame);
    
    let mut merged: Vec<ExportRange> = Vec::with_capacity(ranges.len());
    for range in ranges.drain(..) {
        if let Some(last) = merged.last_mut() {
            // If the current range overlaps or is contiguous with the last one
            if range.in_frame <= last.out_frame + 1 {
                last.out_frame = last.out_frame.max(range.out_frame);
            } else {
                merged.push(range);
            }
        } else {
            merged.push(range);
        }
    }
    
    // Re-assign IDs to be 1..N
    for (idx, range) in merged.iter_mut().enumerate() {
        range.id = idx + 1;
    }
    
    *ranges = merged;
}
```

- [ ] **Step 2: Add unit tests**
Add a new unit test function `test_coalesce_and_reindex_ranges` inside the `mod tests` block at the end of `src/ui/mod.rs`.

Code:
```rust
    #[test]
    fn test_coalesce_and_reindex_ranges() {
        // 1. Empty case
        let mut ranges = vec![];
        coalesce_and_reindex_ranges(&mut ranges);
        assert!(ranges.is_empty());

        // 2. Overlapping, out-of-order, and contiguous case
        let mut ranges = vec![
            ExportRange { id: 99, in_frame: 21, out_frame: 30 }, // contiguous to 10..20
            ExportRange { id: 100, in_frame: 10, out_frame: 20 },
            ExportRange { id: 101, in_frame: 15, out_frame: 25 }, // overlaps both
            ExportRange { id: 102, in_frame: 40, out_frame: 50 }, // distinct
        ];
        coalesce_and_reindex_ranges(&mut ranges);
        
        assert_eq!(ranges.len(), 2);
        
        // The first 3 should be merged into 10..30
        assert_eq!(ranges[0].id, 1);
        assert_eq!(ranges[0].in_frame, 10);
        assert_eq!(ranges[0].out_frame, 30);
        
        // The last one should be 40..50, re-indexed to 2
        assert_eq!(ranges[1].id, 2);
        assert_eq!(ranges[1].in_frame, 40);
        assert_eq!(ranges[1].out_frame, 50);
    }
```

- [ ] **Step 3: Run the unit tests**
Run the tests to verify the helper logic is correct.
Run: `cargo test ui::tests`
Expected: Test passes successfully.

- [ ] **Step 4: Commit**
```bash
git add src/ui/mod.rs
git commit -m "feat: add coalesce_and_reindex_ranges helper and unit tests"
```

---

### Task 2: Integrate Helper into MainApp Event Flow

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Integrate into Add Snippet action**
Modify the add snippet check around line 2271 of `src/ui/mod.rs` to call `coalesce_and_reindex_ranges` and update `self.next_range_id`.

Code to replace:
```rust
                        if clicked
                            && let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker)
                                && inf < outf {
                                    self.ranges.push(ExportRange {
                                        id: self.next_range_id,
                                        in_frame: inf,
                                        out_frame: outf,
                                    });
                                    self.next_range_id += 1;
                                }
```

Replacement code:
```rust
                        if clicked
                            && let (Some(inf), Some(outf)) = (self.in_marker, self.out_marker)
                                && inf < outf {
                                    self.ranges.push(ExportRange {
                                        id: self.next_range_id,
                                        in_frame: inf,
                                        out_frame: outf,
                                    });
                                    coalesce_and_reindex_ranges(&mut self.ranges);
                                    self.next_range_id = self.ranges.len() + 1;
                                }
```

- [ ] **Step 2: Integrate into Delete Snippet action**
Modify the delete snippet check around line 1715 of `src/ui/mod.rs` to call `coalesce_and_reindex_ranges` after removal, and update `self.next_range_id`.

Code to replace:
```rust
                        if let Some(idx) = to_delete {
                            self.ranges.remove(idx);
                        }
```

Replacement code:
```rust
                        if let Some(idx) = to_delete {
                            self.ranges.remove(idx);
                            coalesce_and_reindex_ranges(&mut self.ranges);
                            self.next_range_id = self.ranges.len() + 1;
                        }
```

- [ ] **Step 3: Add integration/regression test**
Run all tests to verify everything passes.
Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**
```bash
git add src/ui/mod.rs
git commit -m "feat: integrate coalescing/re-indexing into add/delete snippet flows"
```
