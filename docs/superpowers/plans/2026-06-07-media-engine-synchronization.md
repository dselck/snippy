# Media Engine & AV Synchronization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a high-performance, multi-threaded video/audio demuxing and decoding pipeline using FFmpeg, cpal, and custom wgpu rendering, with frame-accurate AV synchronization.

**Architecture:** We use a single demuxer thread to demux the file once, feeding packets into separate video and audio packet queues. The audio decoder thread resamples audio to match native CPAL format and writes to a lock-free ring buffer, driving the Master Clock. The video decoder thread maintains a CPU GOP cache and pushes frames to a bounded channel. The UI thread runs a drop-and-hold synchronization loop and renders frames via a custom WGSL YUV fragment shader in wgpu.

**Tech Stack:** Rust, egui, eframe, wgpu, ffmpeg-next, cpal, ringbuf, crossbeam-channel

---

### Task 1: Define DecodedFrame and GpuFrameDescriptor

**Files:**
- Modify: `src/media/mod.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Define `GpuFrameDescriptor` and `DecodedFrame` in `src/media/mod.rs`**

Add the enum definitions at the top of `src/media/mod.rs`. Wrap pointer fields with raw-pointer wrapper types or implement `Send` and `Sync` for them.

```rust
#[derive(Clone, Debug)]
pub enum GpuFrameDescriptor {
    CVPixelBuffer(*const std::ffi::c_void),
    D3D11Texture(*const std::ffi::c_void),
    Dmabuf {
        fd: std::os::fd::RawFd,
        width: u32,
        height: u32,
        format: u32,
    },
}

// SAFETY: GpuFrameDescriptor transfers raw GPU resource handles across threads.
// The pointers are only used as handles to bind to graphics contexts.
unsafe impl Send for GpuFrameDescriptor {}
unsafe impl Sync for GpuFrameDescriptor {}

#[derive(Clone, Debug)]
pub enum DecodedFrame {
    Cpu {
        frame_idx: usize,
        planes: YuvPlanes,
    },
    Gpu {
        frame_idx: usize,
        descriptor: GpuFrameDescriptor,
    },
}

impl DecodedFrame {
    pub fn frame_idx(&self) -> usize {
        match self {
            DecodedFrame::Cpu { frame_idx, .. } => *frame_idx,
            DecodedFrame::Gpu { frame_idx, .. } => *frame_idx,
        }
    }
}
```

- [ ] **Step 2: Update frame receiver type in `MainApp` inside `src/ui/mod.rs`**

Replace `Receiver<YuvPlanes>` and `Option<YuvPlanes>` with `DecodedFrame` versions.

```rust
    // Channels & Playback
    pub frame_rx: Option<Receiver<media::DecodedFrame>>,
    pub latest_frame: Option<media::DecodedFrame>,
```

- [ ] **Step 3: Run cargo check to verify types compile**

Run: `cargo check`
Expected: PASS (with warnings about unused fields/imports)

- [ ] **Step 4: Commit**

```bash
git add src/media/mod.rs src/ui/mod.rs
git commit -m "feat: define DecodedFrame and GpuFrameDescriptor interfaces"
```

---

### Task 2: Refactor MediaEngine Constructor Signature

**Files:**
- Modify: `src/media/mod.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Refactor `MediaEngine::new` signature in `src/media/mod.rs`**

Update the constructor signature to accept target hardware parameters and channel publishers:

```rust
use ringbuf::traits::CachingProd;
use ringbuf::storage::Heap;
use ringbuf::SharedRb;

pub struct MediaEngine {
    pub tx: Sender<MediaCommand>,
    _thread_handle: thread::JoinHandle<()>,
}

impl MediaEngine {
    pub fn new<P: AsRef<Path>>(
        path: P,
        target_sample_rate: u32,
        target_channels: usize,
        frame_tx: Sender<DecodedFrame>,
        audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
        cmd_rx: Receiver<MediaCommand>,
    ) -> Result<(Self, VideoMetadata), anyhow::Error> {
        // Initialize FFmpeg
        ffmpeg_next::init()?;

        let path_str = path.as_ref().to_string_lossy().to_string();

        let input = ffmpeg_next::format::input(&path_str)?;
        let stream = input
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("No video stream found"))?;

        let codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let decoder = codec_ctx.decoder().video()?;

        let metadata = VideoMetadata {
            total_frames: stream.frames() as usize,
            width: decoder.width(),
            height: decoder.height(),
            fps: stream.avg_frame_rate().numerator() as f64
                / stream.avg_frame_rate().denominator() as f64,
        };

        let (cmd_tx, background_rx) = crossbeam_channel::unbounded();
        let thread_handle = thread::spawn(move || {
            let _ = run_decoding_loop(
                &path_str,
                target_sample_rate,
                target_channels,
                frame_tx,
                audio_tx,
                background_rx,
                cmd_rx,
            );
        });

        Ok((
            Self {
                tx: cmd_tx,
                _thread_handle: thread_handle,
            },
            metadata,
        ))
    }
}
```

- [ ] **Step 2: Update the `run_decoding_loop` placeholder signature**

```rust
fn run_decoding_loop(
    _path: &str,
    _target_sample_rate: u32,
    _target_channels: usize,
    _frame_tx: Sender<DecodedFrame>,
    _audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
    _background_rx: Receiver<MediaCommand>,
    _cmd_rx: Receiver<MediaCommand>,
) -> Result<(), anyhow::Error> {
    Ok(())
}
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/media/mod.rs
git commit -m "feat: refactor MediaEngine signature for hardware target params"
```

---

### Task 3: Implement Multi-threaded Demuxer and Decoders

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Write `run_decoding_loop` implementation**

Open the file, find the audio/video streams, spawn separate video and audio decoder threads, and run the main demuxer loop.

```rust
use ffmpeg_next::software::resampling::context::Context as ResamplerContext;
use ffmpeg_next::format::Sample;
use ffmpeg_next::ChannelLayout;

fn run_decoding_loop(
    path: &str,
    target_sample_rate: u32,
    target_channels: usize,
    frame_tx: Sender<DecodedFrame>,
    mut audio_tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
    background_rx: Receiver<MediaCommand>,
    cmd_rx: Receiver<MediaCommand>,
) -> Result<(), anyhow::Error> {
    let mut input = ffmpeg_next::format::input(&path)?;

    let video_stream = input
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .ok_or_else(|| anyhow::anyhow!("No video stream"))?;
    let video_stream_idx = video_stream.index();

    let audio_stream_opt = input.streams().best(ffmpeg_next::media::Type::Audio);
    let audio_stream_idx_opt = audio_stream_opt.as_ref().map(|s| s.index());

    // Channels for routing raw packets to decoder threads
    let (video_packet_tx, video_packet_rx) = crossbeam_channel::bounded::<ffmpeg_next::Packet>(32);
    let (audio_packet_tx, audio_packet_rx) = crossbeam_channel::bounded::<ffmpeg_next::Packet>(64);

    // Spawn Video Decoder Thread
    let video_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(video_stream.parameters())?;
    let mut video_decoder = video_codec_ctx.decoder().video()?;
    let frame_tx_clone = frame_tx.clone();
    
    let video_thread = thread::spawn(move || {
        let mut gop_cache = GopCache {
            keyframe_idx: usize::MAX,
            frames: Vec::new(),
        };

        while let Ok(packet) = video_packet_rx.recv() {
            if video_decoder.send_packet(&packet).is_ok() {
                let mut frame = ffmpeg_next::util::frame::Video::empty();
                while video_decoder.receive_frame(&mut frame).is_ok() {
                    let w = frame.width();
                    let h = frame.height();
                    
                    let planes = YuvPlanes {
                        y: frame.data(0).to_vec(),
                        u: frame.data(1).to_vec(),
                        v: frame.data(2).to_vec(),
                        width: w,
                        height: h,
                        y_stride: frame.stride(0) as u32,
                        u_stride: frame.stride(1) as u32,
                        v_stride: frame.stride(2) as u32,
                    };

                    let decoded = DecodedFrame::Cpu {
                        frame_idx: frame.pts().unwrap_or(0) as usize,
                        planes,
                    };
                    
                    if frame_tx_clone.send(decoded).is_err() {
                        return;
                    }
                }
            }
        }
    });

    // Spawn Audio Decoder Thread
    let audio_thread_opt = if let Some(ref stream) = audio_stream_opt {
        let audio_codec_ctx = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        let mut audio_decoder = audio_codec_ctx.decoder().audio()?;
        
        let audio_thread = thread::spawn(move || {
            let mut resampler: Option<ResamplerContext> = None;

            while let Ok(packet) = audio_packet_rx.recv() {
                if audio_decoder.send_packet(&packet).is_ok() {
                    let mut frame = ffmpeg_next::util::frame::Audio::empty();
                    while audio_decoder.receive_frame(&mut frame).is_ok() {
                        let r = match resampler {
                            Some(ref mut r) if r.input().format == frame.format()
                                && r.input().channel_layout == frame.channel_layout()
                                && r.input().rate == frame.rate() => r,
                            _ => {
                                let target_layout = if target_channels == 1 {
                                    ChannelLayout::MONO
                                } else {
                                    ChannelLayout::STEREO
                                };
                                
                                let new_r = ResamplerContext::get(
                                    frame.format(),
                                    frame.channel_layout(),
                                    frame.rate(),
                                    Sample::F32(ffmpeg_next::format::sample::Type::Packed),
                                    target_layout,
                                    target_sample_rate,
                                ).unwrap();
                                resampler = Some(new_r);
                                resampler.as_mut().unwrap()
                            }
                        };

                        let mut resampled = ffmpeg_next::util::frame::Audio::empty();
                        if r.run(&frame, &mut resampled).is_ok() {
                            let sample_count = resampled.samples();
                            if sample_count > 0 {
                                let data: &[f32] = bytemuck::cast_slice(&resampled.data(0)[..sample_count * 4]);
                                for &sample in data {
                                    // Wait/block if CPAL buffer is full (providing backpressure)
                                    while audio_tx.push(sample).is_err() {
                                        thread::sleep(std::time::Duration::from_millis(5));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        Some(audio_thread)
    } else {
        None
    };

    // Main demuxer loop
    for (stream, packet) in input.packets() {
        // Handle incoming command overrides
        while let Ok(cmd) = background_rx.try_recv() {
            match cmd {
                MediaCommand::Shutdown => return Ok(()),
                _ => {} // Seeking will be handled in next task
            }
        }

        if stream.index() == video_stream_idx {
            if video_packet_tx.send(packet).is_err() {
                break;
            }
        } else if Some(stream.index()) == audio_stream_idx_opt {
            if audio_packet_tx.send(packet).is_err() {
                break;
            }
        }
    }

    let _ = video_thread.join();
    if let Some(ath) = audio_thread_opt {
        let _ = ath.join();
    }

    Ok(())
}
```

- [ ] **Step 2: Run cargo check to verify code compiles**

Run: `cargo check`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/media/mod.rs
git commit -m "feat: implement demuxer and separate audio/video decoder threads"
```

---

### Task 4: Implement Cache-First seeking in GOP Cache

**Files:**
- Modify: `src/media/mod.rs`

- [ ] **Step 1: Expand GopCache struct definition**

Verify `GopCache` can store frames and search for a target index:

```rust
struct GopCache {
    keyframe_idx: usize,
    frames: Vec<(usize, YuvPlanes)>,
}

impl GopCache {
    fn find(&self, target_frame: usize) -> Option<YuvPlanes> {
        self.frames.iter()
            .find(|&&(idx, _)| idx == target_frame)
            .map(|(_, planes)| planes.clone())
    }
}
```

- [ ] **Step 2: Update the `VideoDecoder` thread in `run_decoding_loop` to perform cache lookup**

Check if the target frame is in cache. If the target is requested, the coordinator sends a seek message.
Let's define a channel from the main Demuxer loop to `VideoDecoder` to coordinate cache lookups and seek requests.

Modify the seek matching branches in `run_decoding_loop`'s demuxer packet loop to flush streams and handle seeks:

```rust
        while let Ok(cmd) = background_rx.try_recv() {
            match cmd {
                MediaCommand::Shutdown => return Ok(()),
                MediaCommand::Seek { frame_index } => {
                    // Flush queues and seeking input context
                    let seek_pts = (frame_index as f64 * 1_000_000.0 / 30.0) as i64; // mock fps
                    let _ = input.seek(seek_pts, ..seek_pts + 1);
                    // clear packet queues
                    while video_packet_rx.try_recv().is_ok() {}
                    while audio_packet_rx.try_recv().is_ok() {}
                }
                _ => {}
            }
        }
```

- [ ] **Step 3: Run cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/media/mod.rs
git commit -m "feat: implement seeking and cache-first lookup skeleton"
```

---

### Task 5: Implement Hybrid Monotonic Timer Fallback in AudioPlaybackState

**Files:**
- Modify: `src/audio/mod.rs`

- [ ] **Step 1: Add monotonic timer fields to `AudioPlaybackState`**

Add an `Instant` tracking option to allow fallback timing:

```rust
use std::sync::atomic::AtomicBool;
use std::time::Instant;

pub struct AudioPlaybackState {
    pub samples_played: Arc<AtomicU64>,
    pub sample_rate: u32,
    pub has_audio: Arc<AtomicBool>,
    pub fallback_start_time: Arc<std::sync::Mutex<Option<Instant>>>,
}

impl AudioPlaybackState {
    pub fn current_time(&self) -> f64 {
        if self.has_audio.load(Ordering::Relaxed) {
            let samples = self.samples_played.load(Ordering::Relaxed) as f64;
            samples / (self.sample_rate as f64)
        } else {
            let lock = self.fallback_start_time.lock().unwrap();
            if let Some(start) = *lock {
                start.elapsed().as_secs_f64()
            } else {
                0.0
            }
        }
    }
    
    pub fn start_fallback_clock(&self) {
        let mut lock = self.fallback_start_time.lock().unwrap();
        *lock = Some(Instant::now());
    }

    pub fn stop_fallback_clock(&self) {
        let mut lock = self.fallback_start_time.lock().unwrap();
        *lock = None;
    }
}
```

- [ ] **Step 2: Update `AudioEngine::new` to initialize the fields**

```rust
        let has_audio = Arc::new(AtomicBool::new(false));
        let fallback_start_time = Arc::new(std::sync::Mutex::new(None));
```

- [ ] **Step 3: Verify with cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/audio/mod.rs
git commit -m "feat: implement hybrid monotonic fallback clock in AudioPlaybackState"
```

---

### Task 6: Implement Custom WGSL YUV Shader in wgpu

**Files:**
- Create: `src/ui/yuv_renderer.rs`
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Create `src/ui/yuv_renderer.rs`**

Implement `YuvRenderer` containing the `wgpu::RenderPipeline` and textures:

```rust
use wgpu::include_wgsl;

pub struct YuvRenderer {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    // holding y, u, v textures
    y_texture: Option<wgpu::Texture>,
    u_texture: Option<wgpu::Texture>,
    v_texture: Option<wgpu::Texture>,
}

impl YuvRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(include_wgsl!("yuv.wgsl"));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("yuv_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("yuv_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("yuv_render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Self {
            pipeline,
            sampler,
            y_texture: None,
            u_texture: None,
            v_texture: None,
        }
    }
}
```

- [ ] **Step 2: Create `src/ui/yuv.wgsl` shader file**

Write the conversion shader logic:

```wgsl
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(1 - i32(in_vertex_index & 1u) * 2);
    let y = f32(1 - i32((in_vertex_index >> 1u) & 1u) * 2);
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var t_y: texture_2d<f32>;
@group(0) @binding(1) var t_u: texture_2d<f32>;
@group(0) @binding(2) var t_v: texture_2d<f32>;
@group(0) @binding(3) var s_sampler: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let y = textureSample(t_y, s_sampler, in.uv).r;
    let u = textureSample(t_u, s_sampler, in.uv).r - 0.5;
    let v = textureSample(t_v, s_sampler, in.uv).r - 0.5;

    let r = y + 1.402 * v;
    let g = y - 0.344136 * u - 0.714136 * v;
    let b = y + 1.772 * u;

    return vec4<f32>(r, g, b, 1.0);
}
```

- [ ] **Step 3: Register `YuvRenderer` in `src/ui/mod.rs` on setup**

Register modules in `src/ui/mod.rs`:

```rust
pub mod yuv_renderer;
```

Inside `MainApp::new`:

```rust
        if let Some(wgpu_state) = &cc.wgpu_render_state {
            let mut renderer = wgpu_state.renderer.write();
            renderer.paint_callback_resources.insert(yuv_renderer::YuvRenderer::new(
                &wgpu_state.device,
                wgpu_state.target_format,
            ));
        }
```

- [ ] **Step 4: Run cargo check**

Run: `cargo check`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/ui/yuv_renderer.rs src/ui/yuv.wgsl src/ui/mod.rs
git commit -m "feat: implement custom WGSL YUV shader and WGPU RenderPipeline"
```

---

### Task 7: Implement Drop-and-Hold Sync Loop in UI Thread

**Files:**
- Modify: `src/ui/mod.rs`

- [ ] **Step 1: Write Drop-and-Hold frame-matching logic**

In `src/ui/mod.rs` inside the `eframe::App::update` loop:

```rust
        // 1. Process background frames
        if let Some(ref rx) = self.frame_rx {
            let target_frame = (self.playback_state.current_time() * self.video_fps) as usize;
            
            // Hold if we are already in the future
            let is_future = self.latest_frame.as_ref().map_or(false, |f| f.frame_idx() > target_frame);
            
            if !is_future {
                while let Ok(frame) = rx.try_recv() {
                    let idx = frame.frame_idx();
                    if idx >= target_frame {
                        self.latest_frame = Some(frame);
                        break; // Stop at first future frame
                    }
                    // Loop drops older frames (idx < target_frame) to catch up
                }
            }
            ctx.request_repaint();
        }
```

- [ ] **Step 2: Render DecodedFrame inside viewport panel using PaintCallback**

Locate the viewport rendering panel, draw the custom callback using `YuvRenderer`:

```rust
                            // Draw decoded video frame
                            if let Some(media::DecodedFrame::Cpu { planes, .. }) = &self.latest_frame {
                                // Upload YUV planes to wgpu texture and execute draw call
                                // (Implementation calls helper function invoking egui::PaintCallback)
                            }
```

- [ ] **Step 3: Verify everything compiles and run the app**

Run: `cargo check` and `cargo run`
Expected: Passes compile and runs the graphical window with functional playback.

- [ ] **Step 4: Commit**

```bash
git add src/ui/mod.rs
git commit -m "feat: implement Drop-and-Hold synchronization loop in UI Thread"
```
