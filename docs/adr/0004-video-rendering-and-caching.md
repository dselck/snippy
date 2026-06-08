# 4. Video Rendering and Caching

To optimize video playback and scrubbing performance, we decided to perform color space conversion on the GPU rather than the CPU. The background Media Engine thread will decode frames into raw YUV Planes. The UI Thread will upload these planes as three separate single-channel textures to `wgpu`, where a custom WGSL fragment shader (the YUV Shader) will convert them to RGB in real-time. To support smooth scrubbing (especially backwards), the Media Engine will cache decoded frames in a CPU RAM-based GOP Cache.
