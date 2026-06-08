use std::sync::Arc;
use crate::media::{DecodedFrame, VideoMetadata};
use crossbeam_channel::Receiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackCommand {
    Play,
    Pause,
    TogglePlay,
    Seek {
        frame_idx: usize,
        precise: bool,
    },
}

pub struct PlaybackStateView<'a> {
    pub frame_to_render: Option<&'a DecodedFrame>,
    pub current_time: f64,
    pub current_frame: usize,
    pub is_playing: bool,
    pub seek_in_progress: bool,
}

pub trait AudioBackend: Send + Sync {
    fn configure(&self, time_seconds: f64, playing: bool, flush: bool, paused_for_sync: bool);
    fn current_time(&self) -> f64;
    fn set_has_audio(&self, has_audio: bool);
    fn sample_rate(&self) -> u32;
}

pub trait MediaSource: Send {
    fn send_command(&self, cmd: crate::media::MediaCommand) -> Result<(), anyhow::Error>;
    fn poll_frame(&mut self) -> Result<Option<DecodedFrame>, anyhow::Error>;
}

use std::time::Instant;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};

pub struct NullAudioBackend {
    epoch: Instant,
    start_us: Arc<AtomicU64>,
    base_us: Arc<AtomicU64>,
    has_audio: Arc<AtomicBool>,
}

impl NullAudioBackend {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            start_us: Arc::new(AtomicU64::new(0)),
            base_us: Arc::new(AtomicU64::new(0)),
            has_audio: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl AudioBackend for NullAudioBackend {
    fn configure(&self, time_seconds: f64, playing: bool, _flush: bool, _paused_for_sync: bool) {
        let time_us = (time_seconds * 1_000_000.0) as u64;
        self.base_us.store(time_us, Ordering::Relaxed);
        if playing {
            let now_us = self.epoch.elapsed().as_micros() as u64;
            self.start_us.store(now_us, Ordering::Relaxed);
        } else {
            self.start_us.store(0, Ordering::Relaxed);
        }
    }

    fn current_time(&self) -> f64 {
        let start_us = self.start_us.load(Ordering::Relaxed);
        let base_us = self.base_us.load(Ordering::Relaxed) as f64;
        if start_us > 0 {
            let elapsed_us = self.epoch.elapsed().as_micros() as u64 - start_us;
            (base_us + elapsed_us as f64) / 1_000_000.0
        } else {
            base_us / 1_000_000.0
        }
    }

    fn set_has_audio(&self, has_audio: bool) {
        self.has_audio.store(has_audio, Ordering::Relaxed);
    }

    fn sample_rate(&self) -> u32 {
        44100
    }
}

pub struct MockMediaSource {
    pub cmd_tx: crossbeam_channel::Sender<crate::media::MediaCommand>,
    pub frame_rx: crossbeam_channel::Receiver<DecodedFrame>,
}

impl MediaSource for MockMediaSource {
    fn send_command(&self, cmd: crate::media::MediaCommand) -> Result<(), anyhow::Error> {
        self.cmd_tx.send(cmd)?;
        Ok(())
    }

    fn poll_frame(&mut self) -> Result<Option<DecodedFrame>, anyhow::Error> {
        match self.frame_rx.try_recv() {
            Ok(frame) => Ok(Some(frame)),
            Err(crossbeam_channel::TryRecvError::Empty) => Ok(None),
            Err(e) => Err(anyhow::Error::from(e)),
        }
    }
}
