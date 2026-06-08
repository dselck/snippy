use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{storage::Heap, traits::*, CachingProd, SharedRb};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::time::Instant;

pub struct AudioPlaybackState {
    /// Total number of samples written to the output device.
    pub samples_played: Arc<AtomicU64>,
    /// Sample rate of the output device (e.g., 48000).
    pub sample_rate: u32,
    pub has_audio: Arc<AtomicBool>,
    pub fallback_start_time: Arc<std::sync::Mutex<Option<Instant>>>,
    pub fallback_base_time: Arc<std::sync::Mutex<f64>>,
    pub should_flush: Arc<AtomicBool>,
    pub is_playing: Arc<AtomicBool>,
}

impl AudioPlaybackState {
    /// Returns the current master clock position in seconds.
    pub fn current_time(&self) -> f64 {
        if self.has_audio.load(Ordering::Relaxed) {
            let samples = self.samples_played.load(Ordering::Relaxed) as f64;
            samples / (self.sample_rate as f64)
        } else {
            let lock = self.fallback_start_time.lock().unwrap();
            let base = *self.fallback_base_time.lock().unwrap();
            if let Some(start) = *lock {
                base + start.elapsed().as_secs_f64()
            } else {
                base
            }
        }
    }

    pub fn start_fallback_clock(&self) {
        let mut lock = self.fallback_start_time.lock().unwrap();
        if lock.is_none() {
            *lock = Some(Instant::now());
        }
    }

    pub fn stop_fallback_clock(&self) {
        let mut lock = self.fallback_start_time.lock().unwrap();
        *lock = None;
    }

    pub fn reset(&self, time_seconds: f64) {
        self.samples_played.store((time_seconds * self.sample_rate as f64) as u64, Ordering::Relaxed);
        let mut base = self.fallback_base_time.lock().unwrap();
        *base = time_seconds;
        let mut lock = self.fallback_start_time.lock().unwrap();
        if lock.is_some() {
            *lock = Some(Instant::now());
        }
        self.should_flush.store(true, Ordering::SeqCst);
    }
}

pub struct AudioEngine {
    pub stream: cpal::Stream,
    pub tx: Option<CachingProd<Arc<SharedRb<Heap<f32>>>>>,
    pub playback_state: Arc<AudioPlaybackState>,
}

impl AudioEngine {
    pub fn new() -> Result<Self, anyhow::Error> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No default output audio device found"))?;

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        // Create a thread-safe ring buffer for sending decoded float samples to CPAL
        let rb = SharedRb::<Heap<f32>>::new((sample_rate as usize * channels) / 5);
        let (prod, mut cons) = rb.split();

        let samples_played = Arc::new(AtomicU64::new(0));
        let samples_played_callback = Arc::clone(&samples_played);
        let should_flush = Arc::new(AtomicBool::new(false));
        let should_flush_callback = Arc::clone(&should_flush);
        let is_playing = Arc::new(AtomicBool::new(false));
        let is_playing_callback = Arc::clone(&is_playing);

        // CPAL high-priority real-time audio thread callback
        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if should_flush_callback.swap(false, Ordering::SeqCst) {
                    while cons.try_pop().is_some() {}
                }
                let mut written = 0;
                let active = is_playing_callback.load(Ordering::Relaxed);
                for frame in data.chunks_mut(channels) {
                    if active {
                        if let Some(sample) = cons.try_pop() {
                            for channel_sample in frame.iter_mut() {
                                *channel_sample = sample;
                            }
                            written += 1;
                        } else {
                            for channel_sample in frame.iter_mut() {
                                *channel_sample = 0.0;
                            }
                        }
                    } else {
                        for channel_sample in frame.iter_mut() {
                            *channel_sample = 0.0;
                        }
                    }
                }
                if active {
                    // Update master clock samples counter
                    samples_played_callback.fetch_add(written, Ordering::Relaxed);
                }
            },
            |err| eprintln!("CPAL audio stream error: {:?}", err),
            None,
        )?;

        stream.play()?;

        let has_audio = Arc::new(AtomicBool::new(false));
        let fallback_start_time = Arc::new(std::sync::Mutex::new(None));
        let fallback_base_time = Arc::new(std::sync::Mutex::new(0.0));

        Ok(Self {
            stream,
            tx: Some(prod),
            playback_state: Arc::new(AudioPlaybackState {
                samples_played,
                sample_rate,
                has_audio,
                fallback_start_time,
                fallback_base_time,
                should_flush,
                is_playing,
            }),
        })
    }

    pub fn play(&self) -> Result<(), cpal::PlayStreamError> {
        self.stream.play()
    }

    pub fn pause(&self) -> Result<(), cpal::PauseStreamError> {
        self.stream.pause()
    }
}
