---
status: accepted
supersedes: ADR-0005
---

# 8. Monolithic Media Engine (Single-Threaded Demuxer & Decoder)

To resolve deadlocks and eliminate inter-thread sync friction, we decided to merge the Demuxer and Video Decoder into a single sequential `MediaEngine` event loop.

Previously (ADR-0005), we split demuxing and decoding into separate threads to maximize parallelism. However, this forced the Demuxer to synchronously query the Video Decoder's GOP Cache over a channel when responding to seek requests, risking a deadlock if the Video Decoder was simultaneously blocked waiting for packets.

By making the Media Engine monolithic, we achieve perfect locality: seek requests synchronously hit the GOP cache, and if there's a miss, they synchronously seek FFmpeg. To mitigate audio starvation when video decoding is slow, we accept the requirement of a deeper audio ringbuffer (e.g., 500ms).
