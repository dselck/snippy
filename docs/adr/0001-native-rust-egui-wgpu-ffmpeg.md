# 1. Native Rust Architecture with egui, wgpu, and FFmpeg

To resolve the CPU-GPU transfer bottlenecks and IPC overhead of the previous Svelte/Tauri/CPU-decoding version, we decided to rebuild Snippy as a native, single-process Rust application using `egui` for the GUI, `wgpu` for custom GPU rendering, and `FFmpeg` for demuxing and decoding. This allows us to perform high-performance cross-platform video rendering (via a WGSL YUV shader) and keep audio/video processing in the same memory address space.
