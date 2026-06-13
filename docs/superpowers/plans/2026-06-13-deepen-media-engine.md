# Media Engine Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deepen the Media Engine by extracting the PresenterStateMachine and DecoderStateMachine into testable, pure modules, converting the existing thread loops into dumb adapters.

**Architecture:** The monolithic Media Engine thread loops (`run_decoding_loop` and `run_decoder_thread`) are converted to Event/Action state machines. `PresenterStateMachine` takes Commands and DecodedFrames, outputting `ScheduleWakeup`, `PresentFrame`, etc. `DecoderStateMachine` takes Commands, Packets, QueueStatus, outputting `DemuxNext`, `SeekDemuxer`, `DispatchFrame`.

**Tech Stack:** Rust, crossbeam-channel, ffmpeg-next

---

### Task 1: Create PresenterStateMachine Interface

**Files:**
- Create: `src/media/presenter_state.rs`
- Modify: `src/media/mod.rs` (Add `pub mod presenter_state;`)

- [ ] **Step 1: Write the failing test**

```rust
// In src/media/presenter_state.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaCommand;

    #[test]
    fn test_presenter_seek_outputs_pause() {
        let mut sm = PresenterStateMachine::new();
        let actions = sm.step(PresenterEvent::Command(MediaCommand::Seek {
            frame_index: 10,
            keyframe_only: true,
            sequence_id: 1,
            audio_enabled: false,
        }));
        
        assert!(matches!(actions[0], PresenterAction::ClearLookaheadQueue));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_presenter_seek_outputs_pause`
Expected: FAIL with compilation error (module not found / undefined types)

- [ ] **Step 3: Write minimal implementation**

```rust
// In src/media/presenter_state.rs
use crate::media::{MediaCommand, DecodedFrame};
use std::time::Duration;

#[derive(Debug)]
pub enum PresenterEvent {
    Command(MediaCommand),
    FrameDecoded(DecodedFrame),
    Wakeup,
}

#[derive(Debug, PartialEq)]
pub enum PresenterAction {
    ClearLookaheadQueue,
    SendDecoderCommand(MediaCommand),
    DispatchFrame(DecodedFrame),
    ScheduleWakeup(Duration),
}

pub struct PresenterStateMachine {
    // State goes here
}

impl PresenterStateMachine {
    pub fn new() -> Self {
        Self {}
    }

    pub fn step(&mut self, event: PresenterEvent) -> Vec<PresenterAction> {
        match event {
            PresenterEvent::Command(MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled }) => {
                vec![
                    PresenterAction::ClearLookaheadQueue,
                    PresenterAction::SendDecoderCommand(MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled })
                ]
            }
            _ => vec![]
        }
    }
}
```

- [ ] **Step 4: Hook up module and run test**

Modify `src/media/mod.rs` to include `pub mod presenter_state;`.
Run: `cargo test test_presenter_seek_outputs_pause`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/media/presenter_state.rs src/media/mod.rs
git commit -m "refactor: extract PresenterStateMachine interface and basic test"
```

---

### Task 2: Migrate Presenter State Logic

**Files:**
- Modify: `src/media/presenter_state.rs`

- [ ] **Step 1: Write the failing tests for sync engine integration**

```rust
// In src/media/presenter_state.rs tests
#[test]
fn test_presenter_outputs_wakeup_on_early_frame() {
    let mut sm = PresenterStateMachine::new();
    sm.step(PresenterEvent::Command(MediaCommand::Play));
    
    // Send a frame that is way ahead of the clock
    // (mock implementation of the test)
}
```

- [ ] **Step 2: Migrate logic from run_decoding_loop**

Move the `PresenterState` enum and the `SyncEngine` handling from `src/media/mod.rs` (`run_decoding_loop`) into `PresenterStateMachine::step`.

```rust
// Update PresenterStateMachine implementation
// Note: You must port the actual VideoSyncEngine evaluation into this step function
// and translate SyncAction::Wait into PresenterAction::ScheduleWakeup.
```

- [ ] **Step 3: Run the test suite**

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/media/presenter_state.rs
git commit -m "refactor: migrate full presentation logic into PresenterStateMachine"
```

---

### Task 3: Adapt the Presenter Thread

**Files:**
- Modify: `src/media/mod.rs` (`run_decoding_loop` function)

- [ ] **Step 1: Wire the adapter thread**

Replace the giant nested `match state` blocks in `run_decoding_loop` with a simple loop that calls `PresenterStateMachine::step` and executes the actions using `crossbeam_channel::select!`.

```rust
// Concept to implement in run_decoding_loop:
// let mut sm = PresenterStateMachine::new();
// loop {
//    // Wait for event (with optional timeout if ScheduleWakeup was emitted)
//    let event = select_event_with_timeout(timeout);
//    let actions = sm.step(event);
//    for action in actions {
//        match action {
//            PresenterAction::ScheduleWakeup(d) => timeout = Some(d),
//            PresenterAction::DispatchFrame(f) => frame_tx.send(f),
//            PresenterAction::SendDecoderCommand(c) => decoder_cmd_tx.send(c),
//            // ...
//        }
//    }
// }
```

- [ ] **Step 2: Build and Test App**

Run: `cargo check` and run the app to ensure UI still scrubs and plays video.

- [ ] **Step 3: Commit**

```bash
git add src/media/mod.rs
git commit -m "refactor: convert run_decoding_loop to a dumb thread adapter"
```

---

### Task 4: Create DecoderStateMachine Interface

**Files:**
- Create: `src/media/decoder_state.rs`
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Define the Seam**

```rust
// In src/media/decoder_state.rs
use crate::media::{MediaCommand, DecodedFrame};
use ffmpeg_next::Packet;

pub enum DecoderEvent {
    Command(MediaCommand),
    PacketDemuxed(Packet),
    QueueStatus { video_space: usize, audio_space: usize },
}

pub enum DecoderAction {
    DemuxNextPacket,
    SeekDemuxer(i64),
    PushVideo(DecodedFrame),
    PushAudio(Vec<f32>), // Or whatever the audio representation is
    ScheduleWakeup(std::time::Duration),
}

pub struct DecoderStateMachine {
    // Pipeline references go here
}
```

- [ ] **Step 2: Migrate logic from run_decoder_thread**

Extract the FFmpeg adapter and pipeline routing from `run_decoder_thread` and encapsulate the routing rules inside `DecoderStateMachine`.

- [ ] **Step 3: Commit**

```bash
git add src/media/decoder_state.rs src/media/mod.rs
git commit -m "refactor: extract DecoderStateMachine interface"
```

---

### Task 5: Adapt the Decoder Thread

**Files:**
- Modify: `src/media/mod.rs` (`run_decoder_thread`)

- [ ] **Step 1: Wire the adapter thread**

Rewrite `run_decoder_thread` to be a pure action executor. It owns the `ffmpeg_next::input`, queries the `audio_tx` / `play_queue_tx` capacities to emit `QueueStatus` events, and calls `ffmpeg::read` only when the state machine outputs `Action::DemuxNextPacket`.

- [ ] **Step 2: Build and Test**

Run: `cargo check` and verify full system playback.

- [ ] **Step 3: Commit**

```bash
git add src/media/mod.rs
git commit -m "refactor: convert run_decoder_thread to a dumb adapter"
```
