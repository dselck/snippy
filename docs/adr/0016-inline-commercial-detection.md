# Inline Commercial Detection Pipeline

We are establishing the architectural decisions for Snippy's inline commercial detection capabilities:

## 1. SCTE-35 Digital Splice Cueing
For broadcast streams containing digital program insertion signaling (MPEG-TS/DVB), the demuxer will intercept SCTE-35 PIDs. Splice tables (`splice_insert` or `time_signal`) are parsed at the container level. If valid markers are present, the frame-accurate cutlist is generated directly, bypassing visual and audio decoding entirely to achieve zero CPU-overhead commercial detection.

## 2. Multimodal Feature Extraction (CLIP + YAMNet)
When in-band cues are unavailable, the stream is analyzed using lightweight, pre-trained models loaded dynamically as external resource files at runtime:
* **Visual (CLIP ViT-B/32 or MobileNet-CLIP)**: Keyframes (I-frames) are decoded at a configurable interval (typically every 2 seconds) and converted into 512-dimensional visual semantic embeddings.
* **Audio (YAMNet)**: The audio stream is decoded to PCM and analyzed to extract momentary/short-term loudness (EBU R128 LUFS) alongside semantic class probabilities (e.g. Speech, Music, and Silence) using a 3.7MB YAMNet model.

## 3. Block-Level Sequence Classification (GBDT)
Rather than raw frame-level predictions, metrics are aggregated over segment blocks bounded by scene cuts (color histogram spikes) and silence points. The resulting tabular block data (durations, average loudness, audio class ratios, pooled CLIP features) is classified using a Gradient-Boosted Decision Tree (GBDT) model like LightGBM/XGBoost. This offers superior accuracy and speed on tabular inputs on a CPU compared to deep neural networks.

## 4. Cut-Point Boundary Synchronization
To ensure clean cuts without clipped words or ad bleeding:
* Cut points are synchronized by cross-correlating the visual scene change frame with the local audio energy envelope minima within a short temporal window (typically 1.5 seconds).
* The final cutlist is exported programmatically using a custom JSON structure and standard Edit Decision List (EDL) formats for external player compatibility.
