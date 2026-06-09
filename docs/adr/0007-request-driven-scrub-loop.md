# Request-Driven Scrub Loop and Gesture-Triggered Precise Settle

Timeline scrubbing previously sent a new keyframe seek on every egui repaint tick that the mouse moved. Because each seek increments `latest_sequence_id` and invalidates any in-flight decoded frame, rapid mouse movement produced a perpetual flush loop where the decoder never completed a decode and no frame was ever displayed.

The scrub dispatch is now request-driven: a single keyframe seek is sent, and the next seek is only sent once a decoded frame carrying the current `latest_sequence_id` arrives in the `frame_rx` loop. The UI tracks the latest scrubber position in `desired_scrub_frame`; when the in-flight seek completes, if the position has changed, the next seek fires immediately. On `drag_stopped()`, a single precise seek with audio priming is fired as the settled result — eliminating the previous 500ms debounce-to-precise timer entirely.

Arrow-key hold follows the same response-driven pattern: after the 250ms entrance guard (which prevents tap/hold ambiguity), each successive precise seek is gated on the previous frame arriving, with no timer involvement. Single arrow taps also go directly to `seek_internal` with no debounce.

## Considered Options

- **Timer-driven (previous design):** Seek on every repaint tick, debounce-to-precise after 500ms of idle. Simple to implement but produces a perpetual flush loop under fast scrubbing.
- **Request-driven with timeout escape hatch:** Clear the in-flight gate after a fixed timeout in case the decoder stalls. Rejected — a stalled decoder is a bug to fix at the source, not something to paper over with a timer that reintroduces a second source of truth.

## Consequences

The `seek_to_frame` function, `last_seek_time`, `target_seek_frame`, and `ui_state = Seeking` debounce machinery are removed entirely. All one-shot seeks (filmstrip click, file open, jump to start/end) call `seek_internal` directly.
