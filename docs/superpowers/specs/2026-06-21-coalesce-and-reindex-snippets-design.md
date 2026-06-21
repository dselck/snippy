# Spec: Coalesce and Re-index Snippets

## Background

Snippy allows users to select ranges of a video (snippets) to either keep or cut. Currently, snippet IDs are monotonically increasing, meaning that deleting a snippet leaves a gap in the ID sequence shown in the UI. Additionally, adding overlapping or adjacent selections creates redundant and overlapping snippets, which complicates the timeline rendering, exporting logic, and user interface.

## Goals

1. **Sequential 1..N IDs:** Ensure snippet IDs displayed in the UI always form a contiguous sequence from `1` to `N`, even after adding or deleting snippets.
2. **Coalescing:** Combine overlapping or directly contiguous (adjacent with no gaps) snippets into a single, merged snippet when they are created.

## Design

### 1. Range Coalescing and Re-indexing Algorithm

We will implement a helper function `coalesce_and_reindex_ranges` in `src/ui/mod.rs` to process the ranges list:

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

### 2. State Integration in MainApp

In `src/ui/mod.rs`, we will integrate this helper function into the two event points where ranges are modified:

#### A. Add Snippet Action (around line 2274)
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

#### B. Delete Snippet Action (around line 1716)
```rust
if let Some(idx) = to_delete {
    self.ranges.remove(idx);
    coalesce_and_reindex_ranges(&mut self.ranges);
    self.next_range_id = self.ranges.len() + 1;
}
```

## Verification Plan

We will add unit tests under `mod tests` in `src/ui/mod.rs` to verify the following scenarios:
1. **No-op Coalescing:** Adding non-overlapping, non-contiguous snippets preserves them as distinct.
2. **Overlapping Coalescing:** Overlapping snippets merge into one.
3. **Contiguous Coalescing:** Adjacent/contiguous snippets (e.g. `[10..20]` and `[21..30]`) merge into one.
4. **Re-indexing on Delete:** Deleting a snippet re-indexes the remaining snippets to maintain a continuous `1..N` sequence.
