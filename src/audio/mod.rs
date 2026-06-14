#![allow(clippy::type_complexity)]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{storage::Heap, traits::*, CachingProd, SharedRb};
use std::sync::Arc;

pub mod clock;

pub struct AudioEngine {
    pub stream: cpal::Stream,
    pub tx: Option<CachingProd<Arc<SharedRb<Heap<(f32, u64)>>>>>,
    pub tags_tx: Option<CachingProd<Arc<SharedRb<Heap<(u64, u64, u64)>>>>>,
    pub master_clock: clock::MasterClock,
    pub channels: usize,
    muted: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl AudioEngine {
    pub fn new() -> Result<Self, anyhow::Error> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No default output audio device found"))?;

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate();
        let channels = config.channels() as usize;

        // Create a thread-safe ring buffer for sending decoded float samples to CPAL
        let rb = SharedRb::<Heap<(f32, u64)>>::new(sample_rate as usize * channels * 2);
        let (prod, mut cons) = rb.split();

        // Create tags ring buffer: tags hold (frames_written_total, pts_us)
        let tags_rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(u64, u64, u64)>>::new(256);
        let (tags_prod, mut tags_cons) = tags_rb.split();

        let (master_clock, ticker) = clock::MasterClock::new(sample_rate);

        let mut samples_read_total = 0u64; // In frames
        let mut last_tag: Option<(u64, u64)> = None;
        let mut pending_tag: Option<(u64, u64, u64)> = None;

        let mut current_consumer_generation = 0u64;

        let muted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let muted_clone = muted.clone();

        // CPAL high-priority real-time audio thread callback
        let stream = device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let active = ticker.is_active();
                let is_muted = muted_clone.load(std::sync::atomic::Ordering::Relaxed);
                
                let producer_gen = ticker.playback_generation();
                let mut first_sample = None;
                if producer_gen != current_consumer_generation {
                    // Drop stale samples
                    while let Some((sample, generation)) = cons.try_pop() {
                        if generation == producer_gen {
                            first_sample = Some(sample);
                            break; // Stop dropping, we hit fresh data
                        }
                    }
                    samples_read_total = 0;
                    last_tag = None;
                    pending_tag = None;
                    current_consumer_generation = producer_gen;
                }

                let mut written = 0;
                for frame in data.chunks_mut(channels) {
                    if active {
                        let mut has_data = false;
                        for channel_sample in frame.iter_mut() {
                            let sample_opt = first_sample.take().or_else(|| cons.try_pop().map(|(s, _)| s));
                            if let Some(sample) = sample_opt {
                                *channel_sample = if is_muted { 0.0 } else { sample };
                                has_data = true;
                            } else {
                                *channel_sample = 0.0;
                            }
                        }

                        if has_data {
                            written += 1;
                            samples_read_total += 1;

                            // Process tags
                            loop {
                                let next_tag = pending_tag.take().or_else(|| tags_cons.try_pop());
                                if let Some((tag_generation, tag_idx, tag_pts_us)) = next_tag {
                                    if tag_generation != current_consumer_generation {
                                        continue;
                                    }
                                    if tag_idx <= samples_read_total {
                                        last_tag = Some((tag_idx, tag_pts_us));
                                    } else {
                                        pending_tag = Some((tag_generation, tag_idx, tag_pts_us));
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }
                    } else {
                        for channel_sample in frame.iter_mut() {
                            *channel_sample = 0.0;
                        }
                    }
                }
                
                ticker.advance(written, last_tag, samples_read_total);
            },
            |err| eprintln!("CPAL audio stream error: {:?}", err),
            None,
        )?;

        stream.play()?;

        Ok(Self {
            stream,
            tx: Some(prod),
            tags_tx: Some(tags_prod),
            master_clock,
            channels,
            muted,
        })
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn play(&self) -> Result<(), cpal::PlayStreamError> {
        self.stream.play()
    }

    pub fn pause(&self) -> Result<(), cpal::PauseStreamError> {
        self.stream.pause()
    }
}
