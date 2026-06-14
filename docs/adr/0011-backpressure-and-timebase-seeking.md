# Backpressure and Timebase-Accurate Seeking

We are establishing strict rules regarding thread synchronization, queue boundaries, and timebase conversions within the `MediaEngine`:

## 1. Strict Backpressure Boundary
To prevent unbounded memory accumulation on the presenter thread, the lookahead queue size must be limited (currently to `4` frames). The presenter thread must only pull frames from the bounded `play_queue_rx` channel when the lookahead queue has vacant capacity. This ensures backpressure correctly propagates back to the background `Decoder Thread`, stalling packet demuxing and decoding when the presentation pipeline is full.

## 2. Microsecond Seek Targets
All container-level seeks (such as calls to `Input::seek` in the FFmpeg adapter context) must specify timestamps in microseconds (`AV_TIME_BASE`, i.e., units of `1/1_000_000` seconds) when using the default stream index `-1`. Stream-specific timestamps retrieved from packets or frame metadata must be rescaled to microseconds prior to dispatching container seeks, preventing timebase-mismatch bugs that cause seeking to fail or reset to the start of the video.

## 3. Playback Sync Recovery
Upon completing a seek or reset operation, the master clock must be explicitly un-stalled/resumed from catchup status to ensure its presentation timestamp progress is reactivated.
