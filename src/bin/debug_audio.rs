use snippy2::bench::TestMediaLocator;
use snippy2::media::{MediaEngine, MediaCommand};
use snippy2::audio::clock::MasterClock;
use crossbeam_channel::{bounded, unbounded};
use std::time::Duration;
use ringbuf::traits::*;

fn main() {
    let video_path_buf = TestMediaLocator::resolve().unwrap();
    let (frame_tx, frame_rx) = bounded(4);
    let (_cmd_tx, cmd_rx) = unbounded();
    let audio_rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(f32, u64)>>::new(48000 * 2 * 2);
    let (audio_tx, mut audio_cons) = audio_rb.split();
    let tags_rb = ringbuf::SharedRb::<ringbuf::storage::Heap<(u64, u64, u64)>>::new(256);
    let (tags_tx, mut tags_cons) = tags_rb.split();
    let (clock, _) = MasterClock::new(48000);
    clock.set_has_audio(true);
    let (me, _meta) = MediaEngine::new(&video_path_buf, 48000, 1, frame_tx, audio_tx, tags_tx, cmd_rx, clock.clone()).unwrap();
    
    clock.reset(0.0);
    me.tx.send(MediaCommand::Seek { frame_index: 0, keyframe_only: false, sequence_id: 1, audio_enabled: false }).unwrap();
    
    let frame = frame_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    println!("Seeked to frame: {}, seq: {}", frame.frame_idx(), frame.sequence_id());
    
    clock.play();
    me.tx.send(MediaCommand::Play).unwrap();
    
    for i in 0..15 {
        if let Ok(f) = frame_rx.recv_timeout(Duration::from_secs(1)) {
            println!("Got frame: {}", f.frame_idx());
        } else {
            println!("Timeout waiting for frame {}", i);
            break;
        }
    }
    // Inspect tags
    while let Some((tag_generation, idx, pts)) = tags_cons.try_pop() {
        println!("TAG: gen={}, idx={}, pts_us={}", tag_generation, idx, pts);
    }
    let mut num_samples = 0;
    while let Some(_) = audio_cons.try_pop() { num_samples += 1; }
    println!("Audio samples in queue: {}", num_samples);
}
