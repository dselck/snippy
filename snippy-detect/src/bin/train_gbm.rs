use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use nanogbm::{Config, FeatureBuilder, GbdtTrainer};

// ─── CLI ─────────────────────────────────────────────────────────────────────

/// Training mode: UCI LibSVM dataset OR real CSV from `extract-features`.
enum TrainSource {
    /// One or more CSV files produced by `snippy-detect extract-features`.
    Csv(Vec<PathBuf>),
    /// The original UCI TV News LibSVM dataset directory.
    LibSvm(PathBuf),
}

fn parse_args() -> (TrainSource, PathBuf) {
    let args: Vec<String> = std::env::args().collect();

    let mut csv_files: Vec<PathBuf> = Vec::new();
    let mut manifest: Option<PathBuf> = None;
    let mut libsvm_dir: Option<PathBuf> = None;
    let mut output = PathBuf::from("resources/models/classifier.json");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--csv" => {
                i += 1;
                csv_files.push(PathBuf::from(&args[i]));
            }
            "--manifest" => {
                i += 1;
                manifest = Some(PathBuf::from(&args[i]));
            }
            "--dataset" => {
                i += 1;
                libsvm_dir = Some(PathBuf::from(&args[i]));
            }
            "--output" => {
                i += 1;
                output = PathBuf::from(&args[i]);
            }
            _ => {}
        }
        i += 1;
    }

    // Resolve manifest → csv_files
    if let Some(mpath) = manifest {
        let file = File::open(&mpath).expect("Cannot open manifest file");
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            let line = line.trim().to_string();
            if !line.is_empty() && !line.starts_with('#') {
                csv_files.push(PathBuf::from(line));
            }
        }
    }

    let source = if !csv_files.is_empty() {
        TrainSource::Csv(csv_files)
    } else {
        TrainSource::LibSvm(
            libsvm_dir.unwrap_or_else(|| PathBuf::from("test-files/tv_news_dataset")),
        )
    };

    (source, output)
}


/// A training instance backed by a fixed 7-element dense feature vector.
///
/// Feature layout (0-indexed, must match NanogbmModel::predict in models.rs):
///   [0] duration  – shot length (LibSVM idx 1)
///   [1] ste_mean  – short-time energy mean (LibSVM idx 6)
///   [2] ste_var   – short-time energy variance (LibSVM idx 7)
///   [3] zcr_mean  – zero-crossing rate mean (LibSVM idx 8)
///   [4] zcr_var   – zero-crossing rate variance (LibSVM idx 9)
///   [5] ecr_mean  – edge-change ratio mean (LibSVM idx 4124)
///   [6] ecr_var   – edge-change ratio variance (LibSVM idx 4125)
#[derive(Clone, Default)]
struct Instance {
    label: f32, // 1.0 = commercial, 0.0 = show content
    features: [f64; 7],
}

fn parse_libsvm_line(line: &str) -> Option<Instance> {
    let mut parts = line.split_whitespace();
    let label_str = parts.next()?;
    let label = if label_str == "+1" || label_str == "1" { 1.0 } else { 0.0 };

    let mut feat = [0.0f64; 7];

    for part in parts {
        let mut kv = part.split(':');
        let k: usize = kv.next()?.parse().ok()?;
        let v: f64 = kv.next()?.parse().ok()?;
        match k {
            1    => feat[0] = v, // duration / shot length
            6    => feat[1] = v, // ste_mean
            7    => feat[2] = v, // ste_var
            8    => feat[3] = v, // zcr_mean
            9    => feat[4] = v, // zcr_var
            4124 => feat[5] = v, // ecr_mean
            4125 => feat[6] = v, // ecr_var
            _    => {}
        }
    }

    Some(Instance { label, features: feat })
}

fn load_dataset<P: AsRef<Path>>(path: P) -> Result<Vec<Instance>, anyhow::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut instances = Vec::new();
    for line_result in reader.lines() {
        let line = line_result?;
        if let Some(inst) = parse_libsvm_line(&line) {
            instances.push(inst);
        }
    }
    Ok(instances)
}

fn load_csv_dataset<P: AsRef<Path>>(path: P) -> Result<Vec<Instance>, anyhow::Error> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut instances = Vec::new();
    let mut lines = reader.lines();
    
    // Skip header
    if let Some(header_line) = lines.next() {
        let _ = header_line?;
    }

    for line_result in lines {
        let line = line_result?;
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() < 7 {
            continue;
        }

        let label_val: u8 = parts[0].parse().unwrap_or(0);
        // Exclude Transition (3) from binary training, or map it. Let's exclude to keep clean training.
        if label_val == 3 {
            continue;
        }
        
        let label = if label_val == 1 || label_val == 2 { 1.0f32 } else { 0.0 };
        
        let duration: f64 = parts[2].parse().unwrap_or(0.0);
        let lufs: f64 = parts[3].parse().unwrap_or(0.0);
        let zcr: f64 = parts[4].parse().unwrap_or(0.0);

        let mut feat = [0.0f64; 7];
        feat[0] = duration;
        feat[1] = lufs;       // ste_mean proxy
        feat[2] = 0.0;        // ste_var placeholder
        feat[3] = zcr;        // zcr_mean
        feat[4] = 0.0;        // zcr_var placeholder
        feat[5] = 0.0;        // ecr_mean placeholder (CLIP/visual not extracted yet)
        feat[6] = 0.0;        // ecr_var placeholder

        instances.push(Instance { label, features: feat });
    }
    Ok(instances)
}

fn main() -> Result<(), anyhow::Error> {
    let (source, model_path) = parse_args();
    let mut all_instances = Vec::new();

    match source {
        TrainSource::Csv(paths) => {
            println!("Loading features from CSV files...");
            for path in &paths {
                println!("Loading {}...", path.display());
                let instances = load_csv_dataset(path)?;
                println!("  Loaded {} instances", instances.len());
                all_instances.extend(instances);
            }
        }
        TrainSource::LibSvm(dataset_dir) => {
            println!("Loading TV News Channel Commercial Detection Dataset from {}...", dataset_dir.display());
            let files = ["BBC.txt", "CNN.txt", "CNNIBN.txt", "NDTV.txt", "TIMESNOW.txt"];
            for file_name in &files {
                let file_path = dataset_dir.join(file_name);
                println!("Loading {}...", file_path.display());
                let instances = load_dataset(&file_path)?;
                println!("  Loaded {} instances", instances.len());
                all_instances.extend(instances);
            }
        }
    }

    println!("Total instances: {}", all_instances.len());
    if all_instances.is_empty() {
        return Err(anyhow::anyhow!("No training instances found."));
    }

    // Shuffle deterministically with a fixed seed for reproducibility
    use rand::seq::SliceRandom;
    use rand::SeedableRng;
    let mut rng = rand::rngs::StdRng::seed_from_u64(42);
    all_instances.shuffle(&mut rng);

    // 80/20 train-test split
    let split_idx = (all_instances.len() as f64 * 0.8) as usize;
    let (train_set, test_set) = all_instances.split_at(split_idx);
    println!("Train: {}  Test: {}", train_set.len(), test_set.len());

    // FeatureBuilder – MUST match NanogbmModel::new() in models.rs
    let fb = FeatureBuilder::<Instance>::new()
        .add_continuous("duration".to_string(), |inst| inst.features[0])
        .add_continuous("ste_mean".to_string(),  |inst| inst.features[1])
        .add_continuous("ste_var".to_string(),   |inst| inst.features[2])
        .add_continuous("zcr_mean".to_string(),  |inst| inst.features[3])
        .add_continuous("zcr_var".to_string(),   |inst| inst.features[4])
        .add_continuous("ecr_mean".to_string(),  |inst| inst.features[5])
        .add_continuous("ecr_var".to_string(),   |inst| inst.features[6]);

    let config = Config {
        num_iterations: 200,
        learning_rate: 0.05,
        num_leaves: 63,
        max_depth: 8,
        min_data_in_leaf: 20,
        lambda_l2: 1.0,
        ..Config::default()
    };

    println!("Training GBDT... (release build recommended)");
    let train_labels: Vec<f32> = train_set.iter().map(|i| i.label).collect();
    let test_labels:  Vec<f32> = test_set.iter().map(|i| i.label).collect();

    let model = GbdtTrainer::new(&config, &fb)
        .fit(train_set, &train_labels, None)?;

    println!("Evaluating on test set...");
    let probs = model.predict_proba(&fb, test_set);

    let (mut tp, mut fp, mut tn, mut fn_) = (0usize, 0usize, 0usize, 0usize);
    for (prob, &actual) in probs.iter().zip(test_labels.iter()) {
        let pred = if *prob >= 0.5 { 1.0f32 } else { 0.0 };
        match (pred == 1.0, actual == 1.0) {
            (true,  true)  => tp += 1,
            (true,  false) => fp += 1,
            (false, false) => tn += 1,
            (false, true)  => fn_ += 1,
        }
    }

    let accuracy  = (tp + tn) as f64 / test_set.len() as f64;
    let precision = tp as f64 / (tp + fp).max(1) as f64;
    let recall    = tp as f64 / (tp + fn_).max(1) as f64;
    let f1        = 2.0 * precision * recall / (precision + recall).max(f64::EPSILON);

    println!("\n--- Evaluation Results ---");
    println!("Accuracy:  {:.4} ({:.2}%)", accuracy,  accuracy  * 100.0);
    println!("Precision: {:.4} ({:.2}%)", precision, precision * 100.0);
    println!("Recall:    {:.4} ({:.2}%)", recall,    recall    * 100.0);
    println!("F1 Score:  {:.4}", f1);

    // Save
    let model_dir = model_path.parent().unwrap_or(Path::new("resources/models"));
    std::fs::create_dir_all(model_dir)?;
    println!("\nSaving model to {}...", model_path.display());
    serde_json::to_writer(File::create(&model_path)?, &model)?;
    println!("Done!");

    Ok(())
}

