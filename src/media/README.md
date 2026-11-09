# src/media

## Overview
Core media decoding and playback engine handling multi-threaded FFmpeg demuxing, hardware acceleration, video/audio synchronization, GOP caching, thumbnail background extraction, audio loudness analysis, and smart cut video export.

## Interface
- `[mod.rs:MediaEngine]` - Core pipeline manager coordinating video/audio demuxers, decoder threads, and frame dispatch
- `[mod.rs:MediaEngine::new]` - Constructor initializing media reader, metadata parser, and worker threads for a given file
- `[mod.rs:PresenterState]` - State container for currently presented frame, active seek targets, and pending presentation actions
- `[types.rs:DecodedFrame]` - Container holding decoded video frame data (CPU YUV/NV12 planes or GPU texture handle)
- `[types.rs:GpuFrameDescriptor]` - Metadata for hardware-decoded frames (Metal/VAAPI texture bindings)
- `[types.rs:YuvPlanes]` - CPU-side planar buffers: Y, U, V byte slices with respective strides
- `[types.rs:MediaCommand]` - Command enum dispatched to engine: `Play`, `Pause`, `Seek`, `Step`, `Stop`
- `[types.rs:VideoMetadata]` - Video stream properties: dimensions, FPS, duration, keyframe indices, and timebase
- `[types.rs:ExportStatus]` - Thread-safe export progress tracking struct: current frame, total frames, and percentage
- `[types.rs:get_nearest_keyframe]` - Binary search helper returning closest preceding keyframe index
- `[cache.rs:GopCache]` - LRU memory cache holding decoded frames for the active GOP to accelerate scrubbing
- `[sinks.rs:CrossbeamFrameSink]` - Channel sink pushing decoded video frames to the GUI presenter thread
- `[sinks.rs:CrossbeamAudioSink]` - Channel sink pushing decoded audio samples to real-time playback buffers
- `[hw_context.rs:HwDeviceInfo]` - Descriptor of detected hardware decoder devices (VAAPI, VideoToolbox, CUDA, D3D11VA)
- `[hw_context.rs:probe_and_attach_hw_context]` - Probes supported GPU acceleration APIs and initializes hardware codec context
- `[mac_hw_surface.rs:create_texture_from_videotoolbox_frame]` - Direct zero-copy IOSurface-to-Metal texture mapping from VideoToolbox hardware frames on macOS
- `[decoder_pipeline.rs:VideoDecoderPipeline]` - Multi-threaded video decoding loop handling demuxing, packet queues, and flush logic
- `[decoder_pipeline.rs:DecoderAdapter]` - Trait abstracting hardware vs software video decoders
- `[decoder_pipeline.rs:DecodeError]` - Decoder error categorization enum
- `[ffmpeg_adapter.rs:FfmpegDecoderAdapter]` - Implementation of `DecoderAdapter` using `ffmpeg-next`
- `[audio_pipeline.rs:AudioDecoderPipeline]` - Background audio decoding loop resampling audio and calculating loudness metrics
- `[audio_pipeline.rs:AudioChunk]` - Decoded raw audio PCM chunk with presentation timestamps
- `[audio_pipeline.rs:compute_rms_buckets]` - Computes per-frame RMS loudness buckets for timeline waveform visualization
- `[sync.rs:VideoSyncEngine]` - Precision AV sync regulator comparing frame presentation timestamps to the master clock
- `[sync.rs:SyncAction]` - Sync engine verdict enum: `Present`, `Drop`, `Wait`
- `[session.rs:PlaybackSession]` - High-level session manager coordinating media loading, transport controls, and scrub state
- `[session.rs:InputEvent]` - Session event enum: `SeekRequested`, `PlayToggled`, `StepForward`, `StepBackward`
- `[session.rs:ArrowSeekState]` - State tracker for smooth arrow-key seeking and GOP navigation
- `[thumbnails.rs:ThumbnailDecoderPool]` - Asynchronous worker pool generating timeline filmstrip thumbnail textures
- `[thumbnails.rs:ThumbnailCommand]` - Work request for thumbnail generation at specific frame intervals
- `[thumbnails.rs:ThumbnailResponse]` - Completed thumbnail texture payload with frame index
- `[export.rs:export_video]` - Smart-cut video export pipeline re-encoding only GOP boundaries while stream-copying untouched GOPs
- `[export.rs:GopType]` - Classification of video GOPs during export: `Untouched` (stream-copy) or `Cut` (re-encode)

## Invariants
- `[sync.rs:VideoSyncEngine::evaluate_frame]` - Frames are evaluated against `MasterClock.current_pts()`; returns `SyncAction::Wait` when presentation timestamp is ahead of clock by more than sync threshold, or `SyncAction::Present` when within presentation window
- `[export.rs:export_video]` - Lossless smart cut preserves stream bitrates by stream-copying untouched GOPs and only re-encoding partial boundary GOPs with x264
- `[decoder_pipeline.rs:VideoDecoderPipeline]` - Caches decoded GOP frames in `GopCache` to provide instant scrub responsiveness during forward/backward scrubbing within keyframe intervals
- `[session.rs:PlaybackSession::update]` - Synchronously pops decoded video frames, updates loudness telemetry vectors, and maintains playhead timeline tracking

## Side Effects
- `[hw_context.rs:probe_and_attach_hw_context]` - Probes GPU hardware contexts (VAAPI, VideoToolbox, D3D11) via native FFmpeg device initializers
- `[session.rs:PlaybackSession::load_video]` - Opens video file handles and spawns background demuxing, decoding, and thumbnail worker threads
- `[export.rs:export_video]` - Reads source video file and encodes/remuxes target file on the filesystem with atomic progress updates

## Verification
- Run: `cargo test`
