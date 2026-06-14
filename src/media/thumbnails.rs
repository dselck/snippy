use crossbeam_channel::{Receiver, Sender};
use std::thread;

#[derive(Debug, Clone)]
pub enum ThumbnailCommand {
    Request { frames: Vec<usize> },
    Clear,
}

#[derive(Debug)]
pub struct ThumbnailResponse {
    pub frame_idx: usize,
    pub width: u32,
    pub height: u32,
    pub rgba_data: Vec<u8>,
}
