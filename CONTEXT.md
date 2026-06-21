# Snippy

A cross-platform, high-performance video snippet manager and editor built natively in Rust.

## Language

**Viewport**:
The UI region rendered via `wgpu` that displays decoded video frames.
_Avoid_: Screen, player canvas

**Range**:
A specified portion of a video defined by a start (In) frame and an end (Out) frame.
_Avoid_: Clip, segment, slice, crop

**GUI Framework**:
The user interface toolkit. For this project, we use `egui`.
_Avoid_: Tauri, WebView, frontend

**Master Clock**:
The central timer for playback synchronization. It is driven by the ground-truth presentation timestamps (PTS) of the audio samples currently played by the hardware (using tags propagated from FFmpeg decoder outputs), falling back to a system monotonic timer if no audio stream is present. All synchronization decisions are based on these ground-truth FFmpeg timestamps.
_Avoid_: System timer, wall clock, nominal frame rate approximations

**Presentation Timestamp (PTS)**:
The precise time at which a decoded frame (video or audio) should be presented to the user, defined relative to the start of the media stream. It is sourced from the best-effort metadata computed by the media engine, falling back to decoding order timestamps or nominal frame index approximations only when container-level timing is unavailable.
_Avoid_: Frame time, stream index time, playhead position.

**Audio Output**:
The native audio playback interface managed via `cpal` that consumes PCM samples.
_Avoid_: Web Audio, audio channel

**UI Thread**:
The main application thread running the `egui` GUI event loop and viewport rendering.
_Avoid_: Frontend thread, main loop

**Media Engine**:
The background thread responsible for demuxing and decoding video and audio streams.
_Avoid_: Decoder thread, player backend

**Audio Callback**:
The real-time thread callback managed by `cpal` to write PCM samples to hardware.
_Avoid_: Audio write loop

**YUV Planes**:
The three independent channel arrays (Y, U, V) representing a decoded video frame.
_Avoid_: RGB buffer, raw pixels

**YUV Shader**:
The custom WGSL fragment shader running on the GPU that converts YUV planes to RGB pixels.
_Avoid_: Color converter, CPU scale

**GOP Cache**:
The memory cache in CPU RAM that holds decoded video frames of the current Group of Pictures to accelerate reverse scrubbing.
_Avoid_: GPU cache, texture cache

**DecodedFrame**:
The data structure passed from the Media Engine to the UI Thread representing a decoded video frame. It contains either CPU-resident YUV Planes or a platform-specific GPU frame descriptor.
_Avoid_: Video frame payload, raw texture handle

**Scrub Loop**:
The request-driven cycle that governs seek dispatch during timeline scrubbing. One keyframe seek is outstanding at a time; the next seek is only issued once a DecodedFrame carrying the current sequence ID arrives. As the UI spams position updates, the session handles rate-limiting and seeks to the nearest keyframe. When updates stop (drag end), the shown frame naturally remains the nearest keyframe to where the mouse ended up, avoiding a final precise seek.
_Avoid_: Repaint-driven scrubbing, debounced scrubbing

**Desired Scrub Frame**:
The most recent scrubber position recorded by the UI Thread while a seek is in flight. When the in-flight seek completes, the Scrub Loop compares this value against the last-sent frame index to decide whether to issue another seek.
_Avoid_: Pending seek target, buffered scrub position

**VideoSyncEngine**:
The deep module inside the Media Engine that evaluates video frames against the Master Clock, managing A/V sync and drift math.
_Avoid_: Sync controller, drift manager

**MasterClockPort**:
The minimal interface seam that allows the VideoSyncEngine to read the Master Clock PTS and pause/resume hardware audio playback without coupling to the cpal thread.
_Avoid_: Audio adapter, cpal interface

**VideoDecoderPipeline**:
A deep module sitting behind the Media Engine thread that isolates FFmpeg packet decoding, GOP caching, and sequential fast paths behind a `seek` and `next_frame` interface.
_Avoid_: Decoder thread loop, caching layer, packet manager

**PlaybackSession**:
A deep orchestrator sitting outside the UI that takes full ownership of the MediaEngine, AudioEngine, and the Scrub Loop state machine. It provides a testable, headless interface for timeline navigation and playback, ensuring the UI remains a "dumb" view layer.
_Avoid_: PlaybackController, MainApp state, PlayerCore

**Presenter Thread**:
A deep thread shell inside the Media Engine that evaluates video frames against the Master Clock, managing A/V synchronization, lookahead buffering, and seek dispatching directly via blocking channel selections.
_Avoid_: PresenterStateMachine, sync controller

**Decoder Thread**:
A deep thread shell inside the Media Engine that orchestrates FFmpeg packet demuxing, video/audio pipeline feeding, and queue backpressure using native control flow.
_Avoid_: DecoderStateMachine, Demuxer thread

**FrameSink / AudioSink**:
Real-time adapter seams that sit inside the DecoderStateMachine, providing pure, non-blocking interfaces for pushing decoded frames and audio samples to downstream queues. These eliminate nested blocking control flow in the thread shell.
_Avoid_: Queue adapters, senders

**Backpressure Boundary**:
The mechanism that restricts the presenter thread's lookahead queue from growing without bound, forcing the background decoder thread to pause when the player queue fills up.
_Avoid_: Unbounded caching, unconstrained queueing

**Microsecond Seek Target**:
The rescaled target timestamp (in microseconds, using `AV_TIME_BASE`) passed to the input container during a seek operation, converting it from the stream-specific timebase to prevent seeking errors.
_Avoid_: Raw packet PTS seeks, unscaled seeks

**Thumbnail Decoder Pool**:
The set of N independent background threads that decode thumbnail GOPs in parallel, each owning its own FFmpeg input context and decoder state.
_Avoid_: Shared decoder, single thumbnail thread

**GOP Work Item**:
A unit of work dispatched to a Thumbnail Decoder Pool worker: one keyframe seek target plus the list of `target_frames` to harvest in a single forward decode pass.
_Avoid_: Thumbnail task, decode job

**Thumbnail LRU Cache**:
The bounded `LruCache<usize, TextureHandle>` in the UI thread that evicts least-recently-accessed thumbnails when the entry count exceeds 300.
_Avoid_: Thumbnail HashMap, texture store

**Settings**:
The flat, grouped collection of user-controllable values that govern application behaviour, persisted as TOML in the OS config directory. Divided into UI-visible settings (surfaced in the Settings modal) and advanced settings (accessible only by hand-editing the TOML file directly). Owned by `MainApp` and loaded once at startup.
_Avoid_: Config, Preferences, Options

**Notification**:
A transient, dismissible in-app message held in a `VecDeque<Notification>` on `MainApp` and rendered as dismissible banners in the UI. Any subsystem can push a Notification to surface warnings to the user (e.g. corrupt Settings file, hardware acceleration fallback).
_Avoid_: Toast, Alert, Error message

**Zoom Interval**:
The amount of video time (in seconds or frames, depending on the configured unit) represented by a single thumbnail step on the filmstrip timeline. The slider position is derived from the Zoom Interval via an exponential or linear mapping against the configured min/max bounds.
_Avoid_: Zoom level, zoom scalar, normalised zoom

**Loudness Profile**:
The time-indexed array of RMS amplitude measurements owned by PlaybackSession and surfaced to the UI as a read-only slice. Covers the full video duration at sub-frame resolution; entries are absent until the corresponding audio has been opportunistically decoded.
_Avoid_: Waveform data, audio profile, amplitude array

**Loudness Bucket**:
A single entry in the Loudness Profile representing the root-mean-square amplitude of the audio samples in one sub-frame time window.
_Avoid_: Amplitude sample, waveform bar, loudness sample
