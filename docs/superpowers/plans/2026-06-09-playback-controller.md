# PlaybackController Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Consolidate A/V synchronization, frame queue coordination, and timeline scrubbing out of the GUI thread into a deep `PlaybackController` module with pluggable adapters.

**Architecture:** Create a long-lived `PlaybackController` struct in `src/media/controller.rs`. It acts as a single-tick facade to the UI thread while internally delegating raw audio CPAL and FFmpeg tasks to decoupled `AudioBackend` and `MediaSource` adapters.

**Tech Stack:** Rust, egui, cpal, ffmpeg-next, crossbeam-channel, ringbuf

---

## File Structure Changes
- **Create**: `src/media/controller.rs` — Contains `PlaybackController`, traits `AudioBackend` and `MediaSource`, null/mock adapters, production adapters, and unit tests.
- **Modify**: `src/media/mod.rs` — Declare `pub mod controller;` and re-export key types.
- **Modify**: `src/ui/mod.rs` — Replace raw atomic synchronization and frame queue filtering with calls to the new controller tick interface.

---

### Task 1: Define Traits & Core Interfaces

**Files:**
- Create: `src/media/controller.rs`
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Create `src/media/controller.rs` with core traits and types**

Write the initial file:
```rust
use std::sync::Arc;
use crate::media::{DecodedFrame, VideoMetadata};
use crossbeam_channel::Receiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackCommand {
    Play,
    Pause,
    TogglePlay,
    Seek {
        frame_idx: usize,
        precise: bool,
    },
}

pub struct PlaybackStateView<'a> {
    pub frame_to_render: Option<&'a DecodedFrame>,
    pub current_time: f64,
    pub current_frame: usize,
    pub is_playing: bool,
    pub seek_in_progress: bool,
}

pub trait AudioBackend: Send + Sync {
    fn configure(&self, time_seconds: f64, playing: bool, flush: bool, paused_for_sync: bool);
    fn current_time(&self) -> f64;
    fn set_has_audio(&self, has_audio: bool);
    fn sample_rate(&self) -> u32;
}

pub trait MediaSource: Send {
    fn send_command(&self, cmd: crate::media::MediaCommand) -> Result<(), anyhow::Error>;
    fn poll_frame(&mut self) -> Result<Option<DecodedFrame>, anyhow::Error>;
}
```

- [ ] **Step 2: Declare `controller` module in `src/media/mod.rs`**

Add the submodule declaration to `src/media/mod.rs`:
```rust
pub mod controller;
```

- [ ] **Step 3: Run build to verify basic compilation**

Run: `cargo check`
Expected: PASS with warnings about unused types.

- [ ] **Step 4: Commit**

```bash
git add src/media/mod.rs src/media/controller.rs
git commit -m "arch: define core PlaybackController traits and structures"
```

---

### Task 2: Implement Mock/Null Adapters & Controller Facade

**Files:**
- Modify: `src/media/controller.rs`

- [ ] **Step 1: Write `NullAudioBackend` and `MockMediaSource`**

Append to `src/media/controller.rs`:
```rust
use std::time::Instant;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};

pub struct NullAudioBackend {
    epoch: Instant,
    start_us: Arc<AtomicU64>,
    base_us: Arc<AtomicU64>,
    has_audio: Arc<AtomicBool>,
}

impl NullAudioBackend {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            start_us: Arc::new(AtomicU64::new(0)),
            base_us: Arc::new(AtomicU64::new(0)),
            has_audio: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl AudioBackend for NullAudioBackend {
    fn configure(&self, time_seconds: f64, playing: bool, _flush: bool, _paused_for_sync: bool) {
        let time_us = (time_seconds * 1_000_000.0) as u64;
        self.base_us.store(time_us, Ordering::Relaxed);
        if playing {
            let now_us = self.epoch.elapsed().as_micros() as u64;
            self.start_us.store(now_us, Ordering::Relaxed);
        } else {
            self.start_us.store(0, Ordering::Relaxed);
        }
    }

    fn current_time(&self) -> f64 {
        let start_us = self.start_us.load(Ordering::Relaxed);
        let base_us = self.base_us.load(Ordering::Relaxed) as f64;
        if start_us > 0 {
            let elapsed_us = self.epoch.elapsed().as_micros() as u64 - start_us;
            (base_us + elapsed_us as f64) / 1_000_000.0
        } else {
            base_us / 1_000_000.0
        }
    }

    fn set_has_audio(&self, has_audio: bool) {
        self.has_audio.store(has_audio, Ordering::Relaxed);
    }

    fn sample_rate(&self) -> u32 {
        44100
    }
}

pub struct MockMediaSource {
    pub cmd_tx: crossbeam_channel::Sender<crate::media::MediaCommand>,
    pub frame_rx: crossbeam_channel::Receiver<DecodedFrame>,
}

impl MediaSource for MockMediaSource {
    fn send_command(&self, cmd: crate::media::MediaCommand) -> Result<(), anyhow::Error> {
        self.cmd_tx.send(cmd)?;
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<DecodedFrame>, anyhow::Error> {
        match self.frame_rx.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(None),
            Err(e) => Err(anyhow::Error::from(e)),
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add src/media/controller.rs
git commit -m "arch: implement NullAudioBackend and MockMediaSource adapters"
```

---

### Task 3: Implement PlaybackController Logic & Unit Tests

**Files:**
- Modify: `src/media/controller.rs`

- [ ] **Step 1: Write PlaybackController implementation**

Add the controller struct, fields, and state machine inside `src/media/controller.rs`:
```rust
pub struct PlaybackController {
    audio_backend: Box<dyn AudioBackend>,
    media_source: Option<Box<dyn MediaSource>>,
    latest_sequence_id: usize,
    expected_frame_for_seq: Option<usize>,
    desired_scrub_frame: Option<usize>,
    latest_frame: Option<DecodedFrame>,
    future_frame: Option<DecodedFrame>,
    drift_history: std::collections::VecDeque<f32>,
    is_playing: bool,
    fps: f64,
    total_frames: usize,
    seek_in_progress: bool,
}

impl PlaybackController {
    pub fn new() -> Self {
        Self {
            audio_backend: Box::new(NullAudioBackend::new()),
            media_source: None,
            latest_sequence_id: 0,
            expected_frame_for_seq: None,
            desired_scrub_frame: None,
            latest_frame: None,
            future_frame: None,
            drift_history: std::collections::VecDeque::with_capacity(60),
            is_playing: false,
            fps: 24.0,
            total_frames: 0,
            seek_in_progress: false,
        }
    }

    pub fn load_source_with_adapters(
        &mut self,
        metadata: VideoMetadata,
        audio_backend: Box<dyn AudioBackend>,
        media_source: Box<dyn MediaSource>,
    ) -> Result<VideoMetadata, anyhow::Error> {
        self.fps = metadata.fps;
        self.total_frames = metadata.total_frames;
        self.audio_backend = audio_backend;
        self.media_source = Some(media_source);
        self.latest_sequence_id = 0;
        self.expected_frame_for_seq = None;
        self.desired_scrub_frame = None;
        self.latest_frame = None;
        self.future_frame = None;
        self.is_playing = false;
        self.seek_in_progress = false;

        self.audio_backend.set_has_audio(metadata.has_audio);
        self.audio_backend.configure(0.0, false, true, false);

        Ok(metadata)
    }

    fn nearest_keyframe(&self, target: usize, keyframes: &[usize]) -> usize {
        if keyframes.is_empty() {
            return target;
        }
        match keyframes.binary_search(&target) {
            Ok(i) => keyframes[i],
            Err(0) => keyframes[0],
            Err(i) => keyframes[i - 1],
        }
    }

    fn is_seek_in_flight(&self) -> bool {
        if self.latest_sequence_id == 0 {
            return false;
        }
        self.latest_frame.as_ref().map_or(true, |f| {
            if f.sequence_id() < self.latest_sequence_id {
                true
            } else if let Some(expected) = self.expected_frame_for_seq {
                f.frame_idx() < expected
            } else {
                false
            }
        })
    }

    fn seek_internal(&mut self, frame: usize, keyframe_only: bool, audio_enabled: bool) {
        self.latest_sequence_id += 1;
        self.seek_in_progress = true;
        let clamped = frame.clamp(0, self.total_frames);

        // Expected is target frame or nearest keyframe
        let expected = if keyframe_only {
            // Note: tests will stub/simulate keyframes. For fallback, keyframe every 50 frames
            (clamped / 50) * 50
        } else {
            clamped
        };
        self.expected_frame_for_seq = Some(expected);

        if audio_enabled {
            self.audio_backend.configure(clamped as f64 / self.fps, self.is_playing, true, false);
        } else {
            self.audio_backend.configure(clamped as f64 / self.fps, false, false, false);
        }

        if let Some(ref ms) = self.media_source {
            let _ = ms.send_command(crate::media::MediaCommand::Seek {
                frame_index: clamped,
                keyframe_only,
                sequence_id: self.latest_sequence_id,
                audio_enabled,
            });
        }
        self.future_frame = None;
    }

    pub fn tick(&mut self, cmd: Option<PlaybackCommand>) -> PlaybackStateView<'_> {
        // 1. Process command
        if let Some(c) = cmd {
            match c {
                PlaybackCommand::Play => {
                    self.is_playing = true;
                    self.desired_scrub_frame = None;
                    self.expected_frame_for_seq = None;
                    self.audio_backend.configure(self.current_time(), true, false, false);
                    if let Some(ref ms) = self.media_source {
                        let _ = ms.send_command(crate::media::MediaCommand::Play);
                    }
                }
                PlaybackCommand::Pause => {
                    self.is_playing = false;
                    self.audio_backend.configure(self.current_time(), false, false, false);
                    if let Some(ref ms) = self.media_source {
                        let _ = ms.send_command(crate::media::MediaCommand::Pause);
                    }
                }
                PlaybackCommand::TogglePlay => {
                    if self.is_playing {
                        self.tick(Some(PlaybackCommand::Pause));
                    } else {
                        self.tick(Some(PlaybackCommand::Play));
                    }
                }
                PlaybackCommand::Seek { frame_idx, precise } => {
                    if !precise {
                        if self.is_seek_in_flight() {
                            self.desired_scrub_frame = Some(frame_idx);
                        } else {
                            self.seek_internal(frame_idx, true, false);
                        }
                    } else {
                        self.desired_scrub_frame = None;
                        self.seek_internal(frame_idx, false, true);
                    }
                }
            }
        }

        // 2. Scrub Loop: poll desired scrub frame if seek finished
        if !self.is_seek_in_flight() {
            if let Some(target) = self.desired_scrub_frame.take() {
                self.seek_internal(target, true, false);
            }
        }

        // 3. Poll frames from media source
        if let Some(ref mut ms) = self.media_source {
            while let Ok(Some(frame)) = ms.poll_frame() {
                if frame.sequence_id() == self.latest_sequence_id {
                    let master_time = self.audio_backend.current_time();
                    let pts = frame.pts_sec();
                    if self.is_playing {
                        if pts <= master_time {
                            self.latest_frame = Some(frame);
                        } else {
                            self.future_frame = Some(frame);
                            break;
                        }
                    } else {
                        // In pause/seek, accept the frame immediately
                        self.latest_frame = Some(frame);
                        break;
                    }
                }
            }
        }

        // 4. Promote future frame if time elapsed
        let master_time = self.audio_backend.current_time();
        if self.is_playing {
            if let Some(ref f) = self.future_frame {
                if f.pts_sec() <= master_time {
                    self.latest_frame = self.future_frame.take();
                }
            }
        }

        // 5. A/V Sync pause check
        if self.is_playing {
            let video_time = self.latest_frame.as_ref()
                .filter(|f| f.sequence_id() == self.latest_sequence_id)
                .map_or(0.0, |f| f.pts_sec());
            let drift = master_time - video_time;

            if drift > 0.15 {
                // Video lagging behind audio: trigger sync pause
                self.audio_backend.configure(master_time, true, false, true);
            } else {
                // Caught up
                self.audio_backend.configure(master_time, true, false, false);
            }
        }

        let current_frame = self.latest_frame.as_ref().map(|f| f.frame_idx()).unwrap_or(0);

        PlaybackStateView {
            frame_to_render: self.latest_frame.as_ref(),
            current_time: master_time,
            current_frame,
            is_playing: self.is_playing,
            seek_in_progress: self.seek_in_progress,
        }
    }
}
```

- [ ] **Step 2: Add unit tests verifying seek coalescing & A/V sync pause**

Add the `tests` module block:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::YuvPlanes;
    use crossbeam_channel::unbounded;

    #[test]
    fn test_seek_coalescing_scrub() {
        let (cmd_tx, cmd_rx) = unbounded();
        let (frame_tx, frame_rx) = unbounded();
        let mock_source = MockMediaSource { cmd_tx, frame_rx };
        let null_audio = NullAudioBackend::new();

        let mut controller = PlaybackController::new();
        let meta = VideoMetadata {
            total_frames: 100,
            width: 100,
            height: 100,
            fps: 25.0,
            has_audio: false,
            keyframes: vec![0, 50, 100],
            keyframes_pts: vec![0, 50, 100],
        };

        controller.load_source_with_adapters(meta, Box::new(null_audio), Box::new(mock_source)).unwrap();

        // 1. Send first scrub seek: frame 25
        controller.tick(Some(PlaybackCommand::Seek { frame_idx: 25, precise: false }));
        // Expected seek command to go in-flight immediately
        let cmd1 = cmd_rx.try_recv().unwrap();
        assert!(matches!(cmd1, crate::media::MediaCommand::Seek { frame_index: 25, keyframe_only: true, .. }));

        // 2. While seek is in-flight, request seeks to 30, then 40
        controller.tick(Some(PlaybackCommand::Seek { frame_idx: 30, precise: false }));
        controller.tick(Some(PlaybackCommand::Seek { frame_idx: 40, precise: false }));
        // Ensure no new commands were sent (coalescing active)
        assert!(cmd_rx.try_recv().is_err());

        // 3. Receive the frame for seek 25 to clear the in-flight gate
        let frame = DecodedFrame::Cpu {
            frame_idx: 0, // keyframe for 25 is 0
            planes: YuvPlanes::default(),
            sequence_id: 1,
            pts_sec: 0.0,
        };
        frame_tx.send(frame).unwrap();

        // Tick to consume the frame and release the gate
        controller.tick(None);

        // Verify the latest seek (40) gets dispatched, skipping 30
        let cmd2 = cmd_rx.try_recv().unwrap();
        assert!(matches!(cmd2, crate::media::MediaCommand::Seek { frame_index: 40, keyframe_only: true, .. }));
    }
}
```

- [ ] **Step 3: Run the unit test to verify it passes**

Run: `cargo test media::controller::tests`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/media/controller.rs
git commit -m "feat: implement PlaybackController ticks, scrub loops, and unit tests"
```

---

### Task 4: Implement Production Adapters

**Files:**
- Modify: `src/media/controller.rs`

- [ ] **Step 1: Write `CpalAudioBackend` and `FfmpegMediaSource` production adapters**

Append to `src/media/controller.rs`:
```rust
pub struct CpalAudioBackend {
    state: Arc<crate::audio::AudioPlaybackState>,
}

impl CpalAudioBackend {
    pub fn new(state: Arc<crate::audio::AudioPlaybackState>) -> Self {
        Self { state }
    }
}

impl AudioBackend for CpalAudioBackend {
    fn configure(&self, time_seconds: f64, playing: bool, flush: bool, paused_for_sync: bool) {
        if flush {
            self.state.reset(time_seconds);
        } else {
            self.state.set_position(time_seconds);
        }
        self.state.is_playing.store(playing, Ordering::SeqCst);
        self.state.audio_paused_for_sync.store(paused_for_sync, Ordering::SeqCst);
        if playing && !paused_for_sync {
            self.state.start_fallback_clock();
        } else {
            self.state.stop_fallback_clock();
        }
    }

    fn current_time(&self) -> f64 {
        self.state.current_time()
    }

    fn set_has_audio(&self, has_audio: bool) {
        self.state.has_audio.store(has_audio, Ordering::Relaxed);
    }

    fn sample_rate(&self) -> u32 {
        self.state.sample_rate
    }
}

pub struct FfmpegMediaSource {
    engine: crate::media::MediaEngine,
    frame_rx: Receiver<DecodedFrame>,
}

impl FfmpegMediaSource {
    pub fn new(engine: crate::media::MediaEngine, frame_rx: Receiver<DecodedFrame>) -> Self {
        Self { engine, frame_rx }
    }
}

impl MediaSource for FfmpegMediaSource {
    fn send_command(&self, cmd: crate::media::MediaCommand) -> Result<(), anyhow::Error> {
        self.engine.tx.send(cmd)?;
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<DecodedFrame>, anyhow::Error> {
        match self.frame_rx.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(None),
            Err(e) => Err(anyhow::Error::from(e)),
        }
    }
}
```

- [ ] **Step 2: Add dynamic load method helper to `PlaybackController`**

Add concrete initialization methods in `impl PlaybackController` in `src/media/controller.rs`:
```rust
impl PlaybackController {
    pub fn load_source(&mut self, path: &str) -> Result<VideoMetadata, anyhow::Error> {
        // Teardown previous media source
        self.media_source = None;

        // Initialize CPAL audio engine
        let audio_engine = crate::audio::AudioEngine::new().ok();
        let playback_state = audio_engine.as_ref()
            .map(|ae| Arc::clone(&ae.playback_state))
            .unwrap_or_else(|| Arc::new(crate::audio::AudioPlaybackState {
                current_position_us: Arc::new(AtomicU64::new(0)),
                sample_rate: 44100,
                has_audio: Arc::new(AtomicBool::new(false)),
                should_flush: Arc::new(AtomicBool::new(false)),
                is_playing: Arc::new(AtomicBool::new(false)),
                audio_paused_for_sync: Arc::new(AtomicBool::new(false)),
                fallback_epoch: std::time::Instant::now(),
                fallback_start_us: Arc::new(AtomicU64::new(0)),
                fallback_base_us: Arc::new(AtomicU64::new(0)),
            }));

        let (frame_tx, frame_rx) = crossbeam_channel::bounded(4);
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

        let (engine, metadata) = if let Some(ref ae) = audio_engine {
            crate::media::MediaEngine::new(
                path,
                ae.sample_rate,
                2, // stereo
                frame_tx,
                ae.tx.as_ref().unwrap().clone(),
                ae.tags_tx.as_ref().unwrap().clone(),
                cmd_rx,
                Arc::clone(&playback_state),
            )?
        } else {
            // Null playback configuration: dummy channels
            let rb = SharedRb::<Heap<f32>>::new(48000 * 2);
            let (prod, _) = rb.split();
            let tags_rb = SharedRb::<Heap<(u64, u64)>>::new(256);
            let (tags_prod, _) = tags_rb.split();
            crate::media::MediaEngine::new(
                path,
                44100,
                2,
                frame_tx,
                prod,
                tags_prod,
                cmd_rx,
                Arc::clone(&playback_state),
            )?
        };

        let audio_backend = Box::new(CpalAudioBackend::new(playback_state));
        let media_source = Box::new(FfmpegMediaSource::new(engine, frame_rx));

        self.load_source_with_adapters(metadata.clone(), audio_backend, media_source)?;
        
        // Prevent compiler warnings about unused audio engine
        let _ = audio_engine;

        Ok(metadata)
    }
}
```

- [ ] **Step 3: Run full tests compilation check**

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/media/controller.rs
git commit -m "feat: implement CPAL and FFmpeg production adapters for PlaybackController"
```

---

### Task 5: Integrate PlaybackController into GUI

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Replace raw fields in `MainApp` with `PlaybackController`**

Modify structural declarations in `src/ui/mod.rs` (lines 60-78):
```rust
    pub playback_controller: media::controller::PlaybackController,
    pub metadata: Option<media::VideoMetadata>,
```
Remove these old variables:
`_audio_engine`, `playback_state`, `media_engine`, `keyframes`, `future_frame`, `latest_sequence_id`, `seek_in_progress`, `desired_scrub_frame`, `expected_frame_for_seq`, `stale_discards_count`, `drift_pause_count`.

- [ ] **Step 2: Update `MainApp::new` instantiation**

Modify the constructor `MainApp::new` (lines 102-152) to initialize `playback_controller`:
```rust
        let mut playback_controller = media::controller::PlaybackController::new();
        let mut metadata = None;

        // Auto-load demo reel if present
        let demo_path = "workspace_demo_reel.mp4";
        if std::path::Path::new(demo_path).exists() {
            if let Ok(meta) = playback_controller.load_source(demo_path) {
                metadata = Some(meta);
            }
        }

        Self {
            current_frame: 0,
            total_frames: metadata.as_ref().map_or(1000, |m| m.total_frames),
            is_playing: false,
            video_path: Some(demo_path.to_string()),
            video_fps: metadata.as_ref().map_or(24.0, |m| m.fps),
            in_marker: None,
            out_marker: None,
            ranges: vec![],
            next_range_id: 1,
            cut_mode: false,
            frame_rx: None,
            latest_frame: None,
            loudness_profile: loudness,
            zoom_level: 0.0,
            timeline_height: 145.0,
            playback_controller,
            metadata,
            show_debug_hud: false,
            drift_history: std::collections::VecDeque::with_capacity(60),
            decode_fps_frame_count: 0,
            decode_fps_last_reset: std::time::Instant::now(),
            decode_fps: 0.0,
            left_arrow_held_since: None,
            right_arrow_held_since: None,
        }
```

- [ ] **Step 3: Update frame consumption loop & A/V sync in `update`**

Modify lines 781-915 of `src/ui/mod.rs` to replace raw channel polling, drift correction, and fallback clock tick with a single controller check:
```rust
        // 1. Gather play/seek commands from UI events
        let mut cmd = None;
        if self.is_playing != previous_is_playing {
            cmd = Some(if self.is_playing {
                media::controller::PlaybackCommand::Play
            } else {
                media::controller::PlaybackCommand::Pause
            });
        }

        // Apply arrow keyboard seek shortcuts
        if prev_pressed {
            cmd = Some(media::controller::PlaybackCommand::Seek {
                frame_idx: self.current_frame.saturating_sub(1),
                precise: true,
            });
        } else if next_pressed {
            cmd = Some(media::controller::PlaybackCommand::Seek {
                frame_idx: self.current_frame.saturating_add(1),
                precise: true,
            });
        }

        // 2. Tick the controller facade
        let view = self.playback_controller.tick(cmd);

        // 3. Map status view properties to UI state
        self.is_playing = view.is_playing;
        self.current_frame = view.current_frame;
        
        if let Some(frame) = view.frame_to_render {
            self.latest_frame = Some(frame.clone());
        }

        if self.is_playing {
            ctx.request_repaint();
        }
```

- [ ] **Step 4: Update Video Load button handling**

Modify loading logic in `src/ui/mod.rs` (lines 1498-1596):
```rust
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Video", &["mp4", "mkv", "avi", "mov"])
                                    .pick_file()
                                {
                                    let path_str = path.to_string_lossy().to_string();
                                    match self.playback_controller.load_source(&path_str) {
                                        Ok(meta) => {
                                            self.video_path = Some(path_str);
                                            self.total_frames = meta.total_frames;
                                            self.video_fps = meta.fps;
                                            self.metadata = Some(meta);
                                            self.current_frame = 0;
                                        }
                                        Err(e) => {
                                            eprintln!("Error loading video: {:?}", e);
                                        }
                                    }
                                }
```

- [ ] **Step 5: Run compilation and test suite**

Run: `cargo test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: integrate PlaybackController tick into egui update loop"
```
