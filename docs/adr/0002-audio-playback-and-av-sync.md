# 2. Audio Playback and AV Synchronization

To achieve frame-accurate synchronization without audio stuttering or timing drift, we decided to use the low-level `cpal` library for raw audio output and implement an "Audio-as-Master" synchronization model. The master clock will be calculated directly from the count of audio samples played, and the video render loop will align frame display to this clock. FFmpeg's `libswresample` will handle converting audio stream formats to match the hardware's native format and sample rate.
