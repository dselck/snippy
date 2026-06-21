# Opportunistic Loudness Profile

The waveform viewer displays a Loudness Profile built from RMS amplitude measurements. Rather than scanning the entire audio stream at file load, the profile is populated opportunistically: each `AudioChunk` produced by the `AudioDecoderPipeline` during normal playback or scrubbing computes RMS for the buckets it covers and sends `(bucket_idx, f32)` pairs to `PlaybackSession` via a dedicated side channel. `PlaybackSession` drains this channel on each update and owns the accumulated `Vec<Option<f32>>`, exposing it to the UI as a read-only slice. Absent entries render as a dim tinted fill, giving the user a clear signal of what has been analysed.

## Key decisions

**Opportunistic over eager.** An eager scan would complete the profile in seconds but delays first-render and requires a dedicated thread opening the file a second time. Opportunistic fills in exactly where the user is, which is the highest-value region of the profile, with zero additional I/O overhead.

**4 Loudness Buckets per video frame.** Bucket width = `(1 / fps) / 4`. This gives uniform sub-frame resolution regardless of file length and ensures the waveform remains sharp at frame-by-frame zoom levels. A fixed-time-window approach (e.g. 50 ms) would produce blocky bars at high zoom for high-fps content.

**Buckets are never invalidated on seek.** RMS is deterministic — the same audio always produces the same value. Filled buckets are kept across seeks; a bucket is only written once. This means the profile monotonically becomes more complete over the session without any consistency risk.

## Considered options (rejected)

- **Eager scan at load** — avoids the opportunistic "holes" problem but wastes I/O for regions the user never visits, and requires a second file handle alongside the Media Engine.
- **Fixed 50 ms time window** — simpler bucket math but produces visually blocky waveforms at high zoom for variable fps content.
- **`Arc<RwLock<Vec<f32>>>` shared state** — avoids the side channel but introduces a lock on the UI render path and shared mutable state across threads.
