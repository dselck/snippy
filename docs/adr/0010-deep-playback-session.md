# Deep Playback Session

We are deepening the `PlaybackSession` module by encapsulating its internal state (like `current_frame` and bounds checking) behind an intent-based interface, primarily a `scrub_to(frame)` method. 

This decision ensures the `MainApp` UI Thread acts strictly as a dumb Viewport, avoiding timeline math, bounds checking, and manual orchestration of the Scrub Loop. The UI will no longer directly mutate the playback state.

To preserve the zero-copy GPU path for `wgpu` rendering, `PlaybackSession` will expose the current `DecodedFrame` via a getter that yields a cheap handle/reference. This avoids duplication of pixel data or buffers while ensuring the Viewport only has read access. This structural change significantly improves testing locality, as all orchestrator logic can now be exercised through a single interface without mocking UI behavior.
