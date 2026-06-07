# Video Loading & Backend Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the egui UI to the media/audio engines, implementing the "Load Video" button using `rfd::FileDialog` and implementing accurate frame seeking and playback backpressure.

**Architecture:** Expose CPAL stream control in `AudioEngine`, update seek calculations in `MediaEngine` to use the true stream FPS, and implement video loading/recreation of engine instances and audio/video frame synchronization in `MainApp` using bounded channels and backpressure.

**Tech Stack:** Rust, egui, rfd, ffmpeg-next, cpal, crossbeam-channel

---

### Task 1: Add RFD Dependency to Cargo.toml

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add `rfd` crate to dependencies**

Open `Cargo.toml` and add `rfd = "0.14"` to the `[dependencies]` section:

```toml
[dependencies]
eframe = { version = "0.27.2", features = ["wgpu"] }
ffmpeg-next = "8.1.0"
cpal = "0.15.3"
ringbuf = "0.4.4"
crossbeam-channel = "0.5.12"
tokio = { version = "1.37.0", features = ["rt", "rt-multi-thread"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0.82"
bytemuck = { version = "1.16.0", features = ["derive"] }
rfd = "0.14"
```

- [ ] **Step 2: Verify dependency resolution**

Run: `cargo check`
Expected: Resolves, downloads, and compiles dependencies successfully.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add rfd file dialog dependency"
```

---

### Task 2: Expose CPAL Stream Control in AudioEngine

**Files:**
- Modify: `src/audio/mod.rs`

- [ ] **Step 1: Rename `_stream` and implement `play` and `pause` controls**

Modify `AudioEngine` in `src/audio/mod.rs` to expose `stream` and add control methods:

```rust
pub struct AudioEngine {
    pub stream: cpal::Stream,
    pub tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
    pub playback_state: Arc<AudioPlaybackState>,
}

impl AudioEngine {
    ...
    pub fn play(&self) -> Result<(), cpal::PlayStreamError> {
        self.stream.play()
    }

    pub fn pause(&self) -> Result<(), cpal::PauseStreamError> {
        self.stream.pause()
    }
}
```

Wait, update the constructor `AudioEngine::new` to initialize `stream` instead of `_stream`:

```rust
        Ok(Self {
            stream,
            tx: prod,
            playback_state: Arc::new(AudioPlaybackState {
                samples_played,
                sample_rate,
                has_audio,
                fallback_start_time,
                fallback_base_time,
            }),
        })
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check`
Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add src/audio/mod.rs
git commit -m "feat: expose cpal stream control in AudioEngine"
```

---

### Task 3: Support True FPS and Audio Flag in Media Engine

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Extend VideoMetadata and pass fps to run_decoding_loop**

Extend `VideoMetadata` struct to return a `has_audio` field:

```rust
pub struct VideoMetadata {
    pub total_frames: usize,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
}
```

- [ ] **Step 2: Update MediaEngine::new to populate has_audio and pass fps**

Modify `MediaEngine::new` to check for audio presence and pass the stream's true `fps` to `run_decoding_loop`:

```rust
        let has_audio = input
            .streams()
            .best(ffmpeg_next::media::Type::Audio)
            .is_some();

        let metadata = VideoMetadata {
            total_frames: stream.frames() as usize,
            width: decoder.width(),
            height: decoder.height(),
            fps: stream.avg_frame_rate().numerator() as f64
                / stream.avg_frame_rate().denominator() as f64,
            has_audio,
        };

        // 2. Spawn the background Media Engine decoding loop
        let (cmd_tx, background_rx) = crossbeam_channel::unbounded();
        let fps_clone = metadata.fps;
        let thread_handle = thread::spawn(move || {
            let _ = run_decoding_loop(
                &path_str,
                target_sample_rate,
                target_channels,
                frame_tx,
                audio_tx,
                background_rx,
                cmd_rx,
                fps_clone,
            );
        });
```

- [ ] **Step 3: Modify run_decoding_loop signature and seek calculation**

Update `run_decoding_loop` to accept `fps: f64` and compute the correct `seek_pts`:

```rust
fn run_decoding_loop(
    path: &str,
    target_sample_rate: u32,
    target_channels: usize,
    frame_tx: Sender<DecodedFrame>,
    mut audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
    background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
    fps: f64,
) -> Result<(), anyhow::Error> {
    ...
```

Update seek PTS calculation inside the loop:

```rust
        if let Some(frame_index) = seek_requested {
            let (resp_tx, resp_rx) = crossbeam_channel::bounded::<bool>(1);
            let _ = video_seek_tx.send((frame_index, resp_tx));
            if let Ok(found) = resp_rx.recv() {
                if !found {
                    // Drop packets_iter to release borrow on input
                    drop(packets_iter);

                    // Flush queues and seeking input context
                    let seek_pts = (frame_index as f64 * 1_000_000.0 / fps) as i64;
                    let _ = input.seek(seek_pts, ..seek_pts + 1);

                    // Notify audio thread to flush
                    let _ = audio_seek_tx.send(());

                    // Recreate packets_iter
                    packets_iter = input.packets();
                }
            }
            continue;
        }
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check`
Expected: Compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add src/media/mod.rs
git commit -m "feat: support true fps seeking and has_audio metadata flag"
```

---

### Task 4: UI Integration & Video Loading Dialog in MainApp

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Add media_engine field to MainApp**

Add `pub media_engine: Option<crate::media::MediaEngine>` to the `MainApp` struct definition, and initialize it as `None` in `MainApp::new`.

- [ ] **Step 2: Implement seek_to_frame helper in MainApp**

Add the `seek_to_frame` method to `impl MainApp`:

```rust
    fn seek_to_frame(&mut self, frame: usize) {
        self.current_frame = frame.clamp(0, self.total_frames);
        self.playback_state.reset(self.current_frame as f64 / self.video_fps);
        if let Some(ref me) = self.media_engine {
            let _ = me.tx.send(crate::media::MediaCommand::Seek { frame_index: self.current_frame });
        }
        if let Some(ref rx) = self.frame_rx {
            while rx.try_recv().is_ok() {}
        }
    }
```

- [ ] **Step 3: Update Play/Pause UI logic to pause/resume the cpal stream**

Update the play/pause synchronization inside the `update` loop (where fallback clock is synced):

```rust
        // Sync fallback clock state with play state
        if self.is_playing {
            let running = self.playback_state.fallback_start_time.lock().unwrap().is_some();
            if !running {
                self.playback_state.start_fallback_clock();
                if let Some(ref ae) = self._audio_engine {
                    let _ = ae.play();
                }
            }
            self.current_frame = (self.playback_state.current_time() * self.video_fps) as usize;
            if self.current_frame >= self.total_frames {
                self.current_frame = self.total_frames;
                self.is_playing = false;
                self.playback_state.stop_fallback_clock();
                if let Some(ref ae) = self._audio_engine {
                    let _ = ae.pause();
                }
            }
        } else {
            self.playback_state.stop_fallback_clock();
            if let Some(ref ae) = self._audio_engine {
                let _ = ae.pause();
            }
        }
```

- [ ] **Step 4: Update timeline seeking/scrubbing to use seek_to_frame**

Replace all manual assignments of `self.current_frame` during manual scrubbing/seeking/button clicks with a call to `self.seek_to_frame(...)`:

1. **Filmstrip click handler** (around line 224 in `sprocket filmstrip`):
   ```rust
   if th_response.clicked() {
       self.seek_to_frame(target_frame as usize);
       self.is_playing = false; // Stop playback on seek
   }
   ```
2. **Compact waveform scrub handler** (around line 392):
   ```rust
   if wave_response.dragged() || wave_response.clicked() {
       if let Some(mouse_pos) = wave_response.interact_pointer_pos() {
           let rel_x = (mouse_pos.x - wave_rect.left()).clamp(0.0, wave_rect.width());
           let pct = rel_x / wave_rect.width();
           self.seek_to_frame((pct * self.total_frames as f32) as usize);
           self.is_playing = false; // Stop playback on scrub
       }
   }
   ```
3. **Timeline slider change** (around line 1313):
   ```rust
   if self.progress_bar(ui) {
       self.is_playing = false; // Stop playback on manual scrub
       self.seek_to_frame(self.current_frame);
   }
   ```
4. **Step controls** (around line 1335):
   - Start button: `self.seek_to_frame(0);`
   - PrevFrame button: `self.seek_to_frame(self.current_frame.saturating_sub(1));`
   - NextFrame button: `self.seek_to_frame(self.current_frame + 1);`
   - End button: `self.seek_to_frame(self.total_frames);`

- [ ] **Step 5: Implement Load Video dialog action**

Update the `Load video file` button click action (around line 1250):

```rust
                            // Load button
                            if playback_control_button(
                                ui,
                                PlaybackIcon::LoadVideo,
                                false,
                                "Load video file",
                            ) {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("Video File", &["mp4", "mkv", "avi", "mov"])
                                    .pick_file()
                                {
                                    // 1. Stop current playback and drop old engines
                                    self.is_playing = false;
                                    self.media_engine = None;
                                    self._audio_engine = None;
                                    self.latest_frame = None;

                                    // 2. Initialize fresh Audio Engine
                                    let audio_engine = crate::audio::AudioEngine::new().ok();
                                    
                                    // 3. Setup channels
                                    let (frame_tx, frame_rx) = crossbeam_channel::bounded(4);
                                    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded();

                                    let target_sample_rate = audio_engine.as_ref().map(|ae| ae.playback_state.sample_rate).unwrap_or(44100);
                                    let target_channels = 1; // Mono samples expected by CPAL callback
                                    
                                    // Set default audio producer
                                    let audio_tx = audio_engine.as_ref().map(|ae| ae.tx.clone()).unwrap_or_else(|| {
                                        // Fallback/dummy buffer if no audio engine
                                        let rb = ringbuf::SharedRb::<ringbuf::storage::Heap<f32>>::new(1024);
                                        let (prod, _) = rb.split();
                                        prod
                                    });

                                    // 4. Initialize MediaEngine
                                    match crate::media::MediaEngine::new(
                                        &path,
                                        target_sample_rate,
                                        target_channels,
                                        frame_tx,
                                        audio_tx,
                                        cmd_rx,
                                    ) {
                                        Ok((me, metadata)) => {
                                            self.total_frames = metadata.total_frames;
                                            self.video_fps = metadata.fps;
                                            self.video_path = Some(path.to_string_lossy().into_owned());
                                            self.current_frame = 0;
                                            self.in_marker = None;
                                            self.out_marker = None;
                                            self.ranges.clear();
                                            
                                            // Update active playback state and audio flag
                                            let playback_state = audio_engine.as_ref()
                                                .map(|ae| std::sync::Arc::clone(&ae.playback_state))
                                                .unwrap_or_else(|| std::sync::Arc::new(crate::audio::AudioPlaybackState {
                                                    samples_played: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                                                    sample_rate: 44100,
                                                    has_audio: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                                                    fallback_start_time: std::sync::Arc::new(std::sync::Mutex::new(None)),
                                                    fallback_base_time: std::sync::Arc::new(std::sync::Mutex::new(0.0)),
                                                }));
                                            
                                            playback_state.reset(0.0);
                                            playback_state.has_audio.store(metadata.has_audio, std::sync::atomic::Ordering::Relaxed);
                                            
                                            self.playback_state = playback_state;
                                            self._audio_engine = audio_engine;
                                            self.media_engine = Some(me);
                                            self.frame_rx = Some(frame_rx);
                                            
                                            println!("Loaded video: {}", path.display());
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to load video: {:?}", e);
                                        }
                                    }
                                }
                            }
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check`
Expected: Compiles with no errors.

- [ ] **Step 7: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: implement load video dialog and wire play/pause/seek controls to backend"
```
