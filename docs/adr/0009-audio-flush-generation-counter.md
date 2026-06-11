---
status: accepted
---

# 9. Zero-Allocation Generation Counter for Audio Flush

To eliminate the 100ms lag spike during paused scrubs, we decided to use a generation counter to "flush" the audio ringbuffer instead of spin-waiting on the hardware callback.

Previously, when scrubbing the timeline while paused, the audio decoder would set a `should_flush` flag and spin-wait for the `cpal` hardware callback to acknowledge it. Because the hardware callback never fires while audio is paused, this resulted in a timeout on every single scrub action.

We rejected allocating a new ringbuffer on every seek due to allocation costs. Instead, the Audio Decoder increments a `playback_generation` atomic. The hardware callback simply discards any samples matching an older generation. We accept the trade-off that the ringbuffer briefly contains garbage data (which is quickly skipped over) in exchange for zero-allocation, instant seek operations.
