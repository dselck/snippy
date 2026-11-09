# snippy-detect/src/bin

## Overview
CLI binaries for training GBDT commercial classifiers (`train_gbm`) and extracting labeled training features from media recordings (`extract-features`).

## Interface
- `[train_gbm.rs:main]` - Binary entry point for training GBDT classifier from LibSVM or CSV datasets
- `[train_gbm.rs:parse_args]` - CLI argument parser supporting `--csv`, `--manifest`, `--dataset`, and `--output`
- `[train_gbm.rs:TrainSource]` - Source data enum: `Csv(Vec<PathBuf>)` or `LibSvm(PathBuf)`
- `[train_gbm.rs:Instance]` - Training record containing binary label and 7-element dense feature vector
- `[train_gbm.rs:load_dataset]` - Parses LibSVM datasets into training instances
- `[train_gbm.rs:load_csv_dataset]` - Parses feature CSV files into training instances
- `[extract_features.rs:main]` - Binary entry point for feature extraction pipeline producing CSV rows
- `[extract_features.rs:Args]` - CLI argument struct accepting video, ground-truth JSON, output CSV, and append flag

## Invariants
- `[train_gbm.rs:Instance.features]` - Feature vector layout must strictly match `NanogbmModel::predict` 7-feature order: `[duration, ste_mean, ste_var, zcr_mean, zcr_var, ecr_mean, ecr_var]`
- `[train_gbm.rs:load_csv_dataset]` - Transition label (`3`) is excluded; labels `1` and `2` map to commercial (`1.0`), and `0` maps to content (`0.0`)
- `[train_gbm.rs:main]` - Train/test split is deterministically seeded with `StdRng::seed_from_u64(42)` at 80/20 ratio
- `[extract_features.rs:Args.output]` - Defaults to input video path with extension replaced by `.training.csv`
- `[extract_features.rs:main]` - CSV header is only written when creating a new file or not appending
- `[extract_features.rs:main]` - Start and end frame boundaries are scaled from timestamps by ground-truth `frame_rate`

## Side Effects
- `[train_gbm.rs:main]` - Reads LibSVM/CSV dataset files from disk; writes classifier model JSON to `resources/models/classifier.json`
- `[train_gbm.rs:main]` - Outputs training metrics (accuracy, precision, recall, F1) to stdout
- `[extract_features.rs:main]` - Reads ground-truth JSON and decodes media files via FFmpeg
- `[extract_features.rs:main]` - Creates, truncates, or appends to output CSV files on disk

## Verification
- Run: `cargo test -p snippy-detect`
