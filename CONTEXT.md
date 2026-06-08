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

