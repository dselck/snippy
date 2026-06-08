use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{storage::Heap, traits::*, CachingProd, SharedRb};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::time::Instant;

pub struct AudioPlaybackState {
    /// Current absolute playback position in microseconds since the start of the file.
    pub current_position_us: Arc<AtomicU64>,
    /// Sample rate of the output device (e.g., 48000).
    pub sample_rate: u32,
    pub has_audio: Arc<AtomicBool>,
    pub fallback_start_time: Arc<std::sync::Mutex<Option<Instant>>>,
    pub fallback_base_time: Arc<std::sync::Mutex<f64>>,
    pub should_flush: Arc<AtomicBool>,
    pub is_playing: Arc<AtomicBool>,
    pub audio_paused_for_sync: Arc<AtomicBool>,
}

impl AudioPlaybackState {
    /// Returns the current master clock position in seconds.
    pub fn current_time(&self) -> f64 {
        if self.has_audio.load(Ordering::Relaxed) {
            let us = self.current_position_us.load(Ordering::Relaxed) as f64;
            us / 1_000_000.0
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
        self.current_position_us.store((time_seconds * 1_000_000.0) as u64, Ordering::Relaxed);
        let mut base = self.fallback_base_time.lock().unwrap();
        *base = time_seconds;
        let mut lock = self.fallback_start_time.lock().unwrap();
        *lock = None; // Always pause fallback clock on seek/reset
        self.audio_paused_for_sync.store(true, Ordering::SeqCst);
        self.should_flush.store(true, Ordering::SeqCst);
    }
}

pub struct AudioEngine {
    pub stream: cpal::Stream,
    pub tx: Option<CachingProd<Arc<SharedRb<Heap<f32>>>>>,
    pub tags_tx: Option<CachingProd<Arc<SharedRb<Heap<(u64, u64)>>>>>,
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

        // Create tags ring buffer: tags hold (frames_written_total, pts_us)
        let tags_rb = SharedRb::<Heap<(u64, u64)>>::new(256);
        let (tags_prod, mut tags_cons) = tags_rb.split();

        let current_position_us = Arc::new(AtomicU64::new(0));
        let current_position_us_callback = Arc::clone(&current_position_us);
        let should_flush = Arc::new(AtomicBool::new(false));
        let should_flush_callback = Arc::clone(&should_flush);
        let is_playing = Arc::new(AtomicBool::new(false));
        let is_playing_callback = Arc::clone(&is_playing);
        let audio_paused_for_sync = Arc::new(AtomicBool::new(false));
        let audio_paused_for_sync_callback = Arc::clone(&audio_paused_for_sync);

        let mut samples_read_total = 0u64; // In frames
        let mut last_tag: Option<(u64, u64)> = None;
        let mut pending_tag: Option<(u64, u64)> = None;

        // CPAL high-priority real-time audio thread callback
        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if should_flush_callback.swap(false, Ordering::SeqCst) {
                    while cons.try_pop().is_some() {}
                    while tags_cons.try_pop().is_some() {}
                    samples_read_total = 0;
                    last_tag = None;
                    pending_tag = None;
                }
                let mut written = 0;
                let active = is_playing_callback.load(Ordering::Relaxed)
                    && !audio_paused_for_sync_callback.load(Ordering::Relaxed);
                for frame in data.chunks_mut(channels) {
                    if active {
                        if let Some(sample) = cons.try_pop() {
                            for channel_sample in frame.iter_mut() {
                                *channel_sample = sample;
                            }
                            written += 1;
                            samples_read_total += 1;

                            // Process tags
                            loop {
                                let next_tag = pending_tag.take().or_else(|| tags_cons.try_pop());
                                if let Some((tag_idx, tag_pts_us)) = next_tag {
                                    if tag_idx <= samples_read_total {
                                        last_tag = Some((tag_idx, tag_pts_us));
                                    } else {
                                        pending_tag = Some((tag_idx, tag_pts_us));
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
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
                    if let Some((tag_idx, tag_pts_us)) = last_tag {
                        let elapsed_samples = samples_read_total.saturating_sub(tag_idx);
                        let elapsed_us = (elapsed_samples * 1_000_000) / (sample_rate as u64);
                        let current_pos_us = tag_pts_us + elapsed_us;
                        current_position_us_callback.store(current_pos_us, Ordering::Relaxed);
                    } else {
                        let elapsed_us = (written as u64 * 1_000_000) / (sample_rate as u64);
                        current_position_us_callback.fetch_add(elapsed_us, Ordering::Relaxed);
                    }
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
            tags_tx: Some(tags_prod),
            playback_state: Arc::new(AudioPlaybackState {
                current_position_us,
                sample_rate,
                has_audio,
                fallback_start_time,
                fallback_base_time,
                should_flush,
                is_playing,
                audio_paused_for_sync,
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
