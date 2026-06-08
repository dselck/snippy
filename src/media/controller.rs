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
