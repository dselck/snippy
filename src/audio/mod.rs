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
    pub should_flush: Arc<AtomicBool>,
    pub is_playing: Arc<AtomicBool>,
    pub audio_paused_for_sync: Arc<AtomicBool>,

    // Lock-free hybrid clock
    pub fallback_epoch: Instant,
    pub fallback_start_us: Arc<AtomicU64>, // us since fallback_epoch, 0 means None/paused
    pub fallback_base_us: Arc<AtomicU64>,  // base time in us
}

impl AudioPlaybackState {
    /// Returns the current master clock position in seconds.
    pub fn current_time(&self) -> f64 {
        let start_us = self.fallback_start_us.load(Ordering::Relaxed);
        let base_us = self.fallback_base_us.load(Ordering::Relaxed) as f64;
        if start_us > 0 {
            let elapsed_us = self.fallback_epoch.elapsed().as_micros() as u64 - start_us;
            (base_us + elapsed_us as f64) / 1_000_000.0
        } else {
            base_us / 1_000_000.0
        }
    }

    pub fn start_fallback_clock(&self) {
        let start_us = self.fallback_start_us.load(Ordering::Relaxed);
        if start_us == 0 {
            let now_us = self.fallback_epoch.elapsed().as_micros() as u64;
            self.fallback_start_us.store(now_us, Ordering::Relaxed);
        }
    }

    pub fn stop_fallback_clock(&self) {
        self.fallback_start_us.store(0, Ordering::Relaxed);
    }

    pub fn reset(&self, time_seconds: f64) {
        let time_us = (time_seconds * 1_000_000.0) as u64;
        self.current_position_us.store(time_us, Ordering::Relaxed);
        self.fallback_base_us.store(time_us, Ordering::Relaxed);
        self.fallback_start_us.store(0, Ordering::Relaxed);
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

        let fallback_epoch = Instant::now();
        let fallback_epoch_callback = fallback_epoch;
        let fallback_start_us = Arc::new(AtomicU64::new(0));
        let fallback_start_us_callback = Arc::clone(&fallback_start_us);
        let fallback_base_us = Arc::new(AtomicU64::new(0));
        let fallback_base_us_callback = Arc::clone(&fallback_base_us);

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
                    let current_pos_us = if let Some((tag_idx, tag_pts_us)) = last_tag {
                        let elapsed_samples = samples_read_total.saturating_sub(tag_idx);
                        let elapsed_us = (elapsed_samples * 1_000_000) / (sample_rate as u64);
                        tag_pts_us + elapsed_us
                    } else {
                        let elapsed_us = (written as u64 * 1_000_000) / (sample_rate as u64);
                        current_position_us_callback.load(Ordering::Relaxed) + elapsed_us
                    };
                    // Only update the master/fallback clock if samples were actually popped and played!
                    if written > 0 {
                        current_position_us_callback.store(current_pos_us, Ordering::Relaxed);

                        // Update fallback clock reference atomically
                        fallback_base_us_callback.store(current_pos_us, Ordering::Relaxed);
                        let now_us = fallback_epoch_callback.elapsed().as_micros() as u64;
                        fallback_start_us_callback.store(now_us, Ordering::Relaxed);
                    }
                }
            },
            |err| eprintln!("CPAL audio stream error: {:?}", err),
            None,
        )?;

        stream.play()?;

        let has_audio = Arc::new(AtomicBool::new(false));

        Ok(Self {
            stream,
            tx: Some(prod),
            tags_tx: Some(tags_prod),
            playback_state: Arc::new(AudioPlaybackState {
                current_position_us,
                sample_rate,
                has_audio,
                should_flush,
                is_playing,
                audio_paused_for_sync,
                fallback_epoch,
                fallback_start_us,
                fallback_base_us,
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
