# Selective GOP Smart Rendering and Audio Passthrough

We are establishing the architectural decisions for Snippy's video export:

## 1. GOP-Level Splicing Strategy
Instead of decoding and re-encoding the entire video, export utilizes selective GOP (Group of Pictures) processing. GOPs are classified into three types:
- **Fully Kept GOPs:** Remuxed (stream-copied) directly without decoding.
- **Fully Cut GOPs:** Discarded completely.
- **Partially Kept GOPs:** Decoded to frames, filtered, and re-encoded using software video encoders.
This guarantees lossless quality for untouched segments and maximizes performance.

## 2. Source-Matched Encoding
To prevent quality degradation on partially re-encoded segments (especially for HDR/10-bit video), the video encoder context's pixel format, colorspace, color range, frame rate, and resolution must be matched exactly to the input stream parameters. Constant Rate Factor (CRF) rate control is utilized to maintain visual consistency with the copied streams.

## 3. Lossless Audio Passthrough
Audio streams are processed using direct packet remuxing (passthrough) without decoding/re-encoding. Audio packets are selected if their presentation timestamp (PTS) falls within the kept video segments, and their PTS/DTS are adjusted to align with the output timeline. Out-of-bounds audio frames are discarded, and output packet timestamps are normalized to remain strictly monotonic to satisfy container-level compliance.
