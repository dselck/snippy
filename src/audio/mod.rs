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
}

impl AudioPlaybackState {
    /// Returns the current master clock position in seconds.
    pub fn current_time(&self) -> f64 {
        if self.has_audio.load(Ordering::Relaxed) {
            let samples = self.samples_played.load(Ordering::Relaxed) as f64;
            samples / (self.sample_rate as f64)
        } else {
            let lock = self.fallback_start_time.lock().unwrap();
            if let Some(start) = *lock {
                start.elapsed().as_secs_f64()
            } else {
                0.0
            }
        }
    }

    pub fn start_fallback_clock(&self) {
        let mut lock = self.fallback_start_time.lock().unwrap();
        *lock = Some(Instant::now());
    }

    pub fn stop_fallback_clock(&self) {
        let mut lock = self.fallback_start_time.lock().unwrap();
        *lock = None;
    }
}

pub struct AudioEngine {
    _stream: cpal::Stream,
    pub tx: CachingProd<Arc<SharedRb<Heap<f32>>>>,
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
        let rb = SharedRb::<Heap<f32>>::new(sample_rate as usize * channels * 2);
        let (prod, mut cons) = rb.split();

        let samples_played = Arc::new(AtomicU64::new(0));
        let samples_played_callback = Arc::clone(&samples_played);

        // CPAL high-priority real-time audio thread callback
        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut written = 0;
                for frame in data.chunks_mut(channels) {
                    // Pull sample from our ring buffer or write silence if empty
                    let sample = cons.try_pop().unwrap_or(0.0);
                    for channel_sample in frame.iter_mut() {
                        *channel_sample = sample;
                    }
                    written += 1;
                }
                // Update master clock samples counter
                samples_played_callback.fetch_add(written, Ordering::Relaxed);
            },
            |err| eprintln!("CPAL audio stream error: {:?}", err),
            None,
        )?;

        stream.play()?;

        let has_audio = Arc::new(AtomicBool::new(false));
        let fallback_start_time = Arc::new(std::sync::Mutex::new(None));

        Ok(Self {
            _stream: stream,
            tx: prod,
            playback_state: Arc::new(AudioPlaybackState {
                samples_played,
                sample_rate,
                has_audio,
                fallback_start_time,
            }),
        })
    }
}
