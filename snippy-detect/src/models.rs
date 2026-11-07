use crate::{Config, CutSegment, features::BlockFeatures};
use ort::session::Session;
use ort::value::Tensor;
use ndarray::ArrayD;
use std::fs::File;


pub struct ClipModel {
    pub session: Session,
}

impl ClipModel {
    pub fn new(model_path: &str) -> Result<Self, anyhow::Error> {
        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create SessionBuilder: {}", e))?
            .with_intra_threads(1)
            .map_err(|e| anyhow::anyhow!("Failed to set intra threads: {}", e))?
            .commit_from_file(model_path)
            .map_err(|e| anyhow::anyhow!("Failed to load CLIP model: {}", e))?;
        Ok(Self { session })
    }

    pub fn encode_image(&mut self, image_data: Vec<f32>, shape: Vec<usize>) -> Result<Vec<f32>, anyhow::Error> {
        let shape_ix = ndarray::IxDyn(&shape);
        let array = ArrayD::from_shape_vec(shape_ix, image_data)?;
        let tensor = Tensor::from_array(array)
            .map_err(|e| anyhow::anyhow!("Failed to create CLIP input tensor: {}", e))?;
        
        // Get the name of the first input dynamically
        let input_name = self.session.inputs()[0].name().to_string();
        
        let outputs = self.session.run(ort::inputs![input_name.as_str() => tensor])
            .map_err(|e| anyhow::anyhow!("CLIP model run error: {}", e))?;
        let output_value = &outputs[0];
        let binding = output_value.try_extract_array::<f32>()?;
        
        Ok(binding.iter().copied().collect())
    }
}

pub struct YamnetModel {
    pub session: Session,
}

impl YamnetModel {
    pub fn new(model_path: &str) -> Result<Self, anyhow::Error> {
        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create SessionBuilder: {}", e))?
            .with_intra_threads(1)
            .map_err(|e| anyhow::anyhow!("Failed to set intra threads: {}", e))?
            .commit_from_file(model_path)
            .map_err(|e| anyhow::anyhow!("Failed to load YAMNet model: {}", e))?;
        Ok(Self { session })
    }

    pub fn predict_audio(&mut self, pcm_data: Vec<f32>, shape: Vec<usize>) -> Result<Vec<f32>, anyhow::Error> {
        let shape_ix = ndarray::IxDyn(&shape);
        let array = ArrayD::from_shape_vec(shape_ix, pcm_data)?;
        let tensor = Tensor::from_array(array)
            .map_err(|e| anyhow::anyhow!("Failed to create YAMNet input tensor: {}", e))?;
        
        // Get the name of the first input dynamically
        let input_name = self.session.inputs()[0].name().to_string();
        
        let outputs = self.session.run(ort::inputs![input_name.as_str() => tensor])
            .map_err(|e| anyhow::anyhow!("YAMNet model run error: {}", e))?;
        let output_value = &outputs[0];
        let binding = output_value.try_extract_array::<f32>()?;
        
        Ok(binding.iter().copied().collect())
    }
}

pub struct NanogbmModel {
    model: nanogbm::Model,
    fb: nanogbm::FeatureBuilder<Vec<f64>>,
}

impl NanogbmModel {
    pub fn new(model_path: &str) -> Result<Self, anyhow::Error> {
        let file = File::open(model_path)?;
        let model: nanogbm::Model = serde_json::from_reader(file)?;


        
        // Features (0-indexed dense): [duration, ste_mean, ste_var, zcr_mean, zcr_var, ecr_mean, ecr_var]
        let fb = nanogbm::FeatureBuilder::<Vec<f64>>::new()
            .add_continuous("duration".to_string(), |v| v[0])
            .add_continuous("ste_mean".to_string(), |v| v[1])
            .add_continuous("ste_var".to_string(), |v| v[2])
            .add_continuous("zcr_mean".to_string(), |v| v[3])
            .add_continuous("zcr_var".to_string(), |v| v[4])
            .add_continuous("ecr_mean".to_string(), |v| v[5])
            .add_continuous("ecr_var".to_string(), |v| v[6]);
        
        Ok(Self { model, fb })
    }

    pub fn predict(&mut self, features: &BlockFeatures) -> Result<f32, anyhow::Error> {
        let duration = (features.end_timestamp - features.start_timestamp) as f64;
        // ste_mean: use LUFS as short-time energy proxy
        let ste_mean = features.average_lufs as f64;
        let ste_var = 0.0_f64; // not extracted at runtime; placeholder
        let zcr_mean = features.zcr as f64;
        let zcr_var = 0.0_f64; // not extracted at runtime; placeholder

        // ecr_mean / ecr_var: use CLIP embedding mean/variance as visual change proxy
        let clip_sum: f32 = features.clip_embedding.iter().sum();
        let clip_mean = (clip_sum / (features.clip_embedding.len() as f32).max(1.0)) as f64;
        let clip_var = (features.clip_embedding.iter()
            .map(|&x| (x - clip_mean as f32).powi(2))
            .sum::<f32>() / (features.clip_embedding.len() as f32).max(1.0)) as f64;

        // Dense 7-element vector matching the FeatureBuilder order
        let input_data = vec![duration, ste_mean, ste_var, zcr_mean, zcr_var, clip_mean, clip_var];

        let probs = self.model.predict_proba(&self.fb, &[input_data]);
        Ok(probs.first().copied().unwrap_or(0.0) as f32)
    }
}


pub fn predict_commercial_probability(features: &BlockFeatures) -> f32 {
    let duration = (features.end_timestamp - features.start_timestamp) as f32;
    let lufs = features.average_lufs;
    let zcr = features.zcr;

    let speech_prob = features.yamnet_classes.first().copied().unwrap_or(0.0);
    let music_prob = features.yamnet_classes.get(132).copied().unwrap_or(0.0);
    let silence_prob = features.yamnet_classes.get(494).copied().unwrap_or(0.0);

    let clip_sum: f32 = features.clip_embedding.iter().sum();
    let clip_mean = clip_sum / (features.clip_embedding.len() as f32).max(1.0);
    let clip_variance: f32 = features.clip_embedding.iter()
        .map(|&x| (x - clip_mean).powi(2))
        .sum::<f32>() / (features.clip_embedding.len() as f32).max(1.0);

    let mut score = 0.5f32;

    if lufs > -18.0 {
        score += 0.25;
    } else if lufs > -23.0 {
        score += 0.15;
    } else if lufs < -30.0 {
        score -= 0.3;
    }

    if silence_prob > 0.4 {
        score -= 0.25;
    }

    let active_audio = speech_prob + music_prob;
    if active_audio > 0.6 {
        score += 0.15;
    } else if active_audio < 0.1 {
        score -= 0.15;
    }

    if zcr > 0.25 {
        score += 0.1;
    } else if zcr < 0.05 {
        score -= 0.1;
    }

    if clip_variance > 0.001 {
        score += 0.1;
    } else if clip_variance < 0.0001 {
        score -= 0.2;
    }

    if duration > 1.0 && duration < 45.0 {
        score += 0.05;
    } else if duration > 120.0 {
        score -= 0.2;
    }

    score.clamp(0.0, 1.0)
}

pub fn classify_segments(features: Vec<BlockFeatures>, config: &Config) -> Result<Vec<CutSegment>, anyhow::Error> {
    if features.is_empty() {
        return Ok(Vec::new());
    }

    // Try loading the Nanogbm model from a default location if it exists
    let classifier_path = "resources/models/classifier.json";
    let mut gbm = if std::path::Path::new(classifier_path).exists() {
        match NanogbmModel::new(classifier_path) {
            Ok(model) => Some(model),
            Err(e) => {
                eprintln!("Warning: Failed to load Nanogbm model from {}, falling back to decision tree: {}", classifier_path, e);
                None
            }
        }
    } else {
        None
    };

    let mut block_predictions = Vec::with_capacity(features.len());
    for block in &features {
        let prob = if let Some(ref mut model) = gbm {
            match model.predict(block) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("Warning: Nanogbm inference failed, using decision tree: {}", e);
                    predict_commercial_probability(block)
                }
            }
        } else {
            predict_commercial_probability(block)
        };
        block_predictions.push(prob);
    }


    let mut segments = Vec::new();
    let mut current_segment: Option<(usize, usize, Vec<f32>)> = None;

    for (i, &prob) in block_predictions.iter().enumerate() {
        let is_comm = prob >= config.classification_threshold;
        if is_comm {
            if let Some((start, _, mut confs)) = current_segment.take() {
                confs.push(prob);
                current_segment = Some((start, i, confs));
            } else {
                current_segment = Some((i, i, vec![prob]));
            }
        } else {
            if let Some((start, end, confs)) = current_segment.take() {
                let start_time = features[start].start_timestamp;
                let end_time = features[end].end_timestamp;
                let duration = end_time - start_time;
                if duration as f32 >= config.min_commercial_break_duration {
                    let avg_conf = confs.iter().sum::<f32>() / confs.len() as f32;
                    
                    let mut start_seconds = start_time - config.padding_start_seconds as f64;
                    if start_seconds < 0.0 {
                        start_seconds = 0.0;
                    }
                    let end_seconds = end_time + config.padding_end_seconds as f64;

                    segments.push(CutSegment {
                        start_seconds,
                        end_seconds,
                        start_frame: (start_seconds * 30.0).round() as usize,
                        end_frame: (end_seconds * 30.0).round() as usize,
                        confidence: avg_conf,
                    });
                }
            }
        }
    }

    if let Some((start, end, confs)) = current_segment {
        let start_time = features[start].start_timestamp;
        let end_time = features[end].end_timestamp;
        let duration = end_time - start_time;
        if duration as f32 >= config.min_commercial_break_duration {
            let avg_conf = confs.iter().sum::<f32>() / confs.len() as f32;
            
            let mut start_seconds = start_time - config.padding_start_seconds as f64;
            if start_seconds < 0.0 {
                start_seconds = 0.0;
            }
            let end_seconds = end_time + config.padding_end_seconds as f64;

            segments.push(CutSegment {
                start_seconds,
                end_seconds,
                start_frame: (start_seconds * 30.0).round() as usize,
                end_frame: (end_seconds * 30.0).round() as usize,
                confidence: avg_conf,
            });
        }
    }

    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predict_commercial_probability() {
        let mut ad_clip = vec![0.0; 512];
        for i in 0..512 {
            ad_clip[i] = (i as f32) * 0.01;
        }
        let ad_features = BlockFeatures {
            start_timestamp: 0.0,
            end_timestamp: 10.0,
            clip_embedding: ad_clip,
            yamnet_classes: {
                let mut c = vec![0.0; 521];
                c[0] = 0.5; // Speech
                c[132] = 0.4; // Music
                c
            },
            average_lufs: -15.0,
            zcr: 0.3,
        };

        let ad_prob = predict_commercial_probability(&ad_features);
        assert!(ad_prob > 0.7, "Expected high ad probability, got {}", ad_prob);

        let content_features = BlockFeatures {
            start_timestamp: 10.0,
            end_timestamp: 20.0,
            clip_embedding: vec![0.1; 512], // Static/zero variance
            yamnet_classes: {
                let mut c = vec![0.0; 521];
                c[494] = 0.9; // Silence
                c
            },
            average_lufs: -40.0,
            zcr: 0.02,
        };
        let content_prob = predict_commercial_probability(&content_features);
        assert!(content_prob < 0.3, "Expected low ad probability, got {}", content_prob);
    }

    #[test]
    fn test_classify_segments_merging_and_padding() {
        let mut features = Vec::new();
        
        let make_ad = |start, end| {
            let mut clip = vec![0.0; 512];
            for i in 0..512 { clip[i] = (i as f32) * 0.01; }
            BlockFeatures {
                start_timestamp: start,
                end_timestamp: end,
                clip_embedding: clip,
                yamnet_classes: {
                    let mut c = vec![0.0; 521];
                    c[0] = 0.5;
                    c
                },
                average_lufs: -15.0,
                zcr: 0.3,
            }
        };

        let make_content = |start, end| {
            BlockFeatures {
                start_timestamp: start,
                end_timestamp: end,
                clip_embedding: vec![0.1; 512],
                yamnet_classes: {
                    let mut c = vec![0.0; 521];
                    c[494] = 0.9;
                    c
                },
                average_lufs: -35.0,
                zcr: 0.02,
            }
        };

        features.push(make_ad(0.0, 10.0));
        features.push(make_ad(10.0, 20.0));
        features.push(make_ad(20.0, 30.0));
        features.push(make_content(30.0, 40.0));
        features.push(make_ad(40.0, 50.0));
        features.push(make_ad(50.0, 60.0));
        features.push(make_ad(60.0, 70.0));

        let config = Config {
            classification_threshold: 0.7,
            min_commercial_break_duration: 15.0,
            padding_start_seconds: 1.0,
            padding_end_seconds: 2.0,
            onnx_clip_path: "".to_string(),
            onnx_yamnet_path: "".to_string(),
        };

        let cuts = classify_segments(features, &config).unwrap();
        
        assert_eq!(cuts.len(), 2);
        
        assert_eq!(cuts[0].start_seconds, 0.0);
        assert_eq!(cuts[0].end_seconds, 32.0);
        assert!(cuts[0].confidence > 0.7);

        assert_eq!(cuts[1].start_seconds, 39.0);
        assert_eq!(cuts[1].end_seconds, 72.0);
        assert!(cuts[1].confidence > 0.7);
    }
}

/// LibSVM parsing helpers reused by the dataset accuracy tests.
#[cfg(test)]
mod dataset_tests {
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    /// Parse one LibSVM line into (label, 7-element feature vec).
    ///
    /// Feature layout (must mirror train_gbm.rs AND NanogbmModel::new in models.rs):
    ///   [0] duration (LibSVM idx 1)  [1] ste_mean (idx 6)  [2] ste_var (idx 7)
    ///   [3] zcr_mean (idx 8)         [4] zcr_var  (idx 9)
    ///   [5] ecr_mean (idx 4124)      [6] ecr_var  (idx 4125)
    pub fn parse_libsvm_line(line: &str) -> Option<(f32, Vec<f64>)> {
        let mut parts = line.split_whitespace();
        let label_str = parts.next()?;
        let label = if label_str == "+1" || label_str == "1" { 1.0f32 } else { 0.0 };
        let mut feat = vec![0.0f64; 7];
        for part in parts {
            let mut kv = part.split(':');
            let k: usize = kv.next()?.parse().ok()?;
            let v: f64 = kv.next()?.parse().ok()?;
            match k {
                1    => feat[0] = v,
                6    => feat[1] = v,
                7    => feat[2] = v,
                8    => feat[3] = v,
                9    => feat[4] = v,
                4124 => feat[5] = v,
                4125 => feat[6] = v,
                _    => {}
            }
        }
        Some((label, feat))
    }

    pub fn load_dataset(path: &Path) -> Vec<(f32, Vec<f64>)> {
        let Ok(file) = File::open(path) else { return vec![] };
        BufReader::new(file)
            .lines()
            .filter_map(|l| l.ok())
            .filter_map(|l| parse_libsvm_line(&l))
            .collect()
    }
}

/// Verifies the trained GBDT classifier against the UCI TV News dataset.
///
/// Skips gracefully when the model or dataset files are absent so CI passes
/// without large LFS blobs.
#[cfg(test)]
mod gbm_accuracy_tests {
    use super::dataset_tests::*;
    use nanogbm::FeatureBuilder;
    use std::fs::File;
    use std::path::Path;

    #[test]
    fn test_gbm_dataset_accuracy_bbc() {
        let model_path  = Path::new("resources/models/classifier.json");
        let dataset_dir = Path::new("test-files/tv_news_dataset");

        if !model_path.exists() {
            eprintln!("SKIP: model not found at {}", model_path.display());
            return;
        }
        if !dataset_dir.exists() {
            eprintln!("SKIP: dataset directory not found");
            return;
        }

        let file = File::open(model_path).expect("open classifier.json");
        let model: nanogbm::Model = serde_json::from_reader(file).expect("deserialise model");

        // FeatureBuilder MUST match NanogbmModel::new() and train_gbm.rs
        let fb = FeatureBuilder::<Vec<f64>>::new()
            .add_continuous("duration".to_string(), |v| v[0])
            .add_continuous("ste_mean".to_string(),  |v| v[1])
            .add_continuous("ste_var".to_string(),   |v| v[2])
            .add_continuous("zcr_mean".to_string(),  |v| v[3])
            .add_continuous("zcr_var".to_string(),   |v| v[4])
            .add_continuous("ecr_mean".to_string(),  |v| v[5])
            .add_continuous("ecr_var".to_string(),   |v| v[6]);

        let bbc_data = load_dataset(&dataset_dir.join("BBC.txt"));
        assert!(!bbc_data.is_empty(), "BBC.txt loaded 0 instances");

        let features: Vec<Vec<f64>> = bbc_data.iter().map(|(_, f)| f.clone()).collect();
        let labels:   Vec<f32>      = bbc_data.iter().map(|(l, _)| *l).collect();

        let probs = model.predict_proba(&fb, &features);

        let (mut tp, mut fp, mut tn, mut fn_) = (0usize, 0usize, 0usize, 0usize);
        for (prob, &actual) in probs.iter().zip(labels.iter()) {
            let pred = if *prob >= 0.5 { 1.0f32 } else { 0.0 };
            match (pred == 1.0, actual == 1.0) {
                (true,  true)  => tp += 1,
                (true,  false) => fp += 1,
                (false, false) => tn += 1,
                (false, true)  => fn_ += 1,
            }
        }

        let accuracy  = (tp + tn) as f64 / bbc_data.len() as f64;
        let precision = tp as f64 / (tp + fp).max(1) as f64;
        let recall    = tp as f64 / (tp + fn_).max(1) as f64;
        let f1 = 2.0 * precision * recall / (precision + recall + f64::EPSILON);

        println!("BBC.txt  accuracy={:.3}  precision={:.3}  recall={:.3}  F1={:.3}",
            accuracy, precision, recall, f1);

        // Thresholds: conservative for the 7-feature model (audio/temporal only).
        // Full-feature (124-feature) training achieves ~92.6 % accuracy.
        assert!(precision >= 0.75,
            "Precision {:.3} below 0.75 on BBC.txt", precision);
        assert!(recall >= 0.80,
            "Recall {:.3} below 0.80 on BBC.txt", recall);
        assert!(accuracy >= 0.75,
            "Accuracy {:.3} below 0.75 on BBC.txt", accuracy);
    }

    #[test]
    fn test_gbm_dataset_accuracy_cnn() {
        let model_path  = Path::new("resources/models/classifier.json");
        let dataset_dir = Path::new("test-files/tv_news_dataset");
        if !model_path.exists() || !dataset_dir.exists() { return; }

        let model: nanogbm::Model =
            serde_json::from_reader(File::open(model_path).unwrap()).unwrap();
        let fb = FeatureBuilder::<Vec<f64>>::new()
            .add_continuous("duration".to_string(), |v| v[0])
            .add_continuous("ste_mean".to_string(),  |v| v[1])
            .add_continuous("ste_var".to_string(),   |v| v[2])
            .add_continuous("zcr_mean".to_string(),  |v| v[3])
            .add_continuous("zcr_var".to_string(),   |v| v[4])
            .add_continuous("ecr_mean".to_string(),  |v| v[5])
            .add_continuous("ecr_var".to_string(),   |v| v[6]);

        let data = load_dataset(&dataset_dir.join("CNN.txt"));
        assert!(!data.is_empty(), "CNN.txt loaded 0 instances");

        let features: Vec<Vec<f64>> = data.iter().map(|(_, f)| f.clone()).collect();
        let labels:   Vec<f32>      = data.iter().map(|(l, _)| *l).collect();
        let probs = model.predict_proba(&fb, &features);

        let (mut tp, mut fp, mut fn_) = (0usize, 0usize, 0usize);
        let mut total = 0usize;
        for (prob, &actual) in probs.iter().zip(labels.iter()) {
            let pred = if *prob >= 0.5 { 1.0f32 } else { 0.0 };
            total += 1;
            match (pred == 1.0, actual == 1.0) {
                (true,  true)  => tp += 1,
                (true,  false) => fp += 1,
                (false, true)  => fn_ += 1,
                _ => {}
            }
        }
        let precision = tp as f64 / (tp + fp).max(1) as f64;
        let recall    = tp as f64 / (tp + fn_).max(1) as f64;
        println!("CNN.txt  precision={:.3}  recall={:.3}  (n={})", precision, recall, total);

        assert!(precision >= 0.75, "CNN precision {:.3} below 0.75", precision);
        assert!(recall    >= 0.80, "CNN recall {:.3} below 0.80",    recall);
    }
}
