use crate::{Config, demux::MediaData};

pub struct BlockFeatures {
    pub start_timestamp: f64,
    pub end_timestamp: f64,
    pub clip_embedding: Vec<f32>,
    pub yamnet_classes: Vec<f32>,
    pub average_lufs: f32,
    pub zcr: f32,
}

struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let out = x * self.b0 + self.z1;
        self.z1 = x * self.b1 - out * self.a1 + self.z2;
        self.z2 = x * self.b2 - out * self.a2;
        out
    }
}

fn get_k_weighting_filters(fs: f64) -> (Biquad, Biquad) {
    // Stage 1: Pre-filter (high shelving)
    let fc_pre = 1681.974450;
    let q_pre = 0.707175236106;
    let g_pre = 3.999843854;

    let k_pre = (std::f64::consts::PI * fc_pre / fs).tan();
    let v_pre = 10.0f64.powf(g_pre / 20.0);
    let beta_pre = v_pre.sqrt() / q_pre;

    let a0_pre = 1.0 + 2.0f64.sqrt() * k_pre + k_pre * k_pre;
    let a1_pre = 2.0 * (k_pre * k_pre - 1.0) / a0_pre;
    let a2_pre = (1.0 - 2.0f64.sqrt() * k_pre + k_pre * k_pre) / a0_pre;
    let b0_pre = (v_pre + beta_pre * k_pre + k_pre * k_pre) / a0_pre;
    let b1_pre = 2.0 * (k_pre * k_pre - v_pre) / a0_pre;
    let b2_pre = (v_pre - beta_pre * k_pre + k_pre * k_pre) / a0_pre;

    let pre_filter = Biquad::new(b0_pre, b1_pre, b2_pre, a1_pre, a2_pre);

    // Stage 2: RLB filter (high pass)
    let fc_rlb = 38.1354709187;
    let k_rlb = (std::f64::consts::PI * fc_rlb / fs).tan();
    let a0_rlb = 1.0 + 2.0 * k_rlb + k_rlb * k_rlb;
    let a1_rlb = 2.0 * (k_rlb * k_rlb - 1.0) / a0_rlb;
    let a2_rlb = (1.0 - 2.0 * k_rlb + k_rlb * k_rlb) / a0_rlb;

    let b0_rlb = 1.0 / a0_rlb;
    let b1_rlb = -2.0 / a0_rlb;
    let b2_rlb = 1.0 / a0_rlb;

    let rlb_filter = Biquad::new(b0_rlb, b1_rlb, b2_rlb, a1_rlb, a2_rlb);

    (pre_filter, rlb_filter)
}

pub fn extract_features(media_data: &MediaData, _config: &Config) -> Result<Vec<BlockFeatures>, anyhow::Error> {
    let mut weighted_pcm = Vec::with_capacity(media_data.audio_pcm.len());
    let fs = media_data.audio_sample_rate as f64;

    if fs > 0.0 && !media_data.audio_pcm.is_empty() {
        let (mut pre, mut rlb) = get_k_weighting_filters(fs);
        for &x in &media_data.audio_pcm {
            let y = rlb.process(pre.process(x as f64));
            weighted_pcm.push(y as f32);
        }
    }

    let total_duration = if fs > 0.0 {
        media_data.audio_pcm.len() as f64 / fs
    } else if let Some(last_kf) = media_data.keyframes.last() {
        last_kf.timestamp_seconds + 2.0
    } else {
        0.0
    };

    let mut blocks = Vec::new();

    if !media_data.keyframes.is_empty() {
        for i in 0..media_data.keyframes.len() {
            let start_t = media_data.keyframes[i].timestamp_seconds;
            let end_t = if i + 1 < media_data.keyframes.len() {
                media_data.keyframes[i + 1].timestamp_seconds
            } else {
                total_duration.max(start_t)
            };
            blocks.push((start_t, end_t));
        }
    } else if total_duration > 0.0 {
        let block_dur = 2.0;
        let mut t = 0.0;
        while t < total_duration {
            let next_t = (t + block_dur).min(total_duration);
            blocks.push((t, next_t));
            t = next_t;
        }
    }

    let mut features_list = Vec::with_capacity(blocks.len());

    for (start_t, end_t) in blocks {
        let start_sample = if fs > 0.0 {
            (start_t * fs).round() as usize
        } else {
            0
        };
        let end_sample = if fs > 0.0 {
            (end_t * fs).round() as usize
        } else {
            0
        };

        let slice_len = if end_sample > start_sample {
            end_sample - start_sample
        } else {
            0
        };

        // 1. Compute Loudness (EBU R128 LUFS)
        let average_lufs = if slice_len > 0 && start_sample < weighted_pcm.len() {
            let end_idx = end_sample.min(weighted_pcm.len());
            let slice = &weighted_pcm[start_sample..end_idx];
            let sum_sq: f64 = slice.iter().map(|&y| (y as f64) * (y as f64)).sum();
            let mean_sq = sum_sq / slice.len() as f64;
            if mean_sq > 1e-10 {
                (-0.691 + 10.0 * mean_sq.log10()) as f32
            } else {
                -70.0
            }
        } else {
            -70.0
        };

        // 2. Compute Zero Crossing Rate (ZCR)
        let zcr = if slice_len > 1 && start_sample < media_data.audio_pcm.len() {
            let end_idx = end_sample.min(media_data.audio_pcm.len());
            let slice = &media_data.audio_pcm[start_sample..end_idx];
            let mut crossings = 0;
            for idx in 1..slice.len() {
                let prev = slice[idx - 1];
                let curr = slice[idx];
                if (prev >= 0.0 && curr < 0.0) || (prev < 0.0 && curr >= 0.0) {
                    crossings += 1;
                }
            }
            crossings as f32 / (slice.len() - 1) as f32
        } else {
            0.0
        };

        // 3. Stubs for embeddings
        let clip_embedding = vec![0.0; 512];
        let yamnet_classes = vec![0.0; 521];

        features_list.push(BlockFeatures {
            start_timestamp: start_t,
            end_timestamp: end_t,
            clip_embedding,
            yamnet_classes,
            average_lufs,
            zcr,
        });
    }

    Ok(features_list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demux::DecodedKeyframe;

    #[test]
    fn test_zcr_and_lufs() {
        // Generate a 1-second sine wave at 1000 Hz, sample rate 16000 Hz, amplitude 0.5
        let fs = 16000.0;
        let freq = 1000.0;
        let amp = 0.5;
        let mut audio_pcm = Vec::with_capacity(16000);
        for i in 0..16000 {
            let t = i as f64 / fs;
            let val = amp * (2.0 * std::f64::consts::PI * freq * t).sin();
            audio_pcm.push(val as f32);
        }

        // Mock MediaData
        let media_data = MediaData {
            keyframes: vec![
                DecodedKeyframe {
                    timestamp_seconds: 0.0,
                    rgb_data: vec![],
                    width: 0,
                    height: 0,
                    is_keyframe: true,
                }
            ],
            audio_pcm,
            audio_sample_rate: 16000,
        };

        let config = Config {
            classification_threshold: 0.8,
            min_commercial_break_duration: 15.0,
            padding_start_seconds: 0.0,
            padding_end_seconds: 0.0,
            onnx_clip_path: "".to_string(),
            onnx_yamnet_path: "".to_string(),
        };

        let features = extract_features(&media_data, &config).unwrap();
        assert_eq!(features.len(), 1);

        let feat = &features[0];
        // Expected zero crossing rate for 1000Hz sine wave at 16000Hz is:
        // crossings = 2000 in 16000 samples. ZCR = 2000 / 15999 ~ 0.125
        assert!((feat.zcr - 0.125).abs() < 0.01, "Expected ZCR ~ 0.125, got {}", feat.zcr);

        // LUFS check: K-weighting filters will shape the 1000Hz signal.
        // Let's make sure it is a reasonable value (not silent, around -10 to -15 LUFS).
        assert!(feat.average_lufs > -20.0 && feat.average_lufs < -5.0, "Expected LUFS in range, got {}", feat.average_lufs);
    }
}

