# 3. Threading and Concurrency Model

To maintain a highly responsive UI and guarantee glitch-free audio playback, we decided to separate the application into three independent threads. The UI Thread runs the `egui` interface and renders video frames via `wgpu`. The background Media Engine thread demuxes and decodes the media file. The real-time Audio Callback thread (managed by `cpal`) pulls samples from a lock-free ring buffer and writes them to the audio hardware.
