use crate::audio::AudioEngine;
use crate::audio::clock::MasterClock;
use crate::media::{MediaEngine, MediaCommand, DecodedFrame, VideoMetadata};
use crossbeam_channel::{Receiver, Sender};
use ringbuf::{storage::Heap, traits::*, SharedRb};
use std::time::{Duration, Instant};
use std::thread;
use std::sync::{Arc, RwLock};
use crate::media::types::{DefectMap, StreamTypeMask, DefectErrorType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowSeekState {
    Idle,
    SteppingLeft { held_since: Instant },
    SteppingRight { held_since: Instant, unpaced_started: bool },
}

pub enum InputEvent {
    ArrowLeftPressed,
    ArrowLeftDown,
    ArrowLeftReleased,
    ArrowRightPressed,
    ArrowRightDown,
    ArrowRightReleased,
    ScrubDragStart,
    ScrubDragUpdate { target_frame: usize },
    ScrubDragEnd,
}

pub struct PlaybackSession {
    current_frame: usize,
    total_frames: usize,
    is_playing: bool,
    video_fps: f64,
    
    pub(crate) frame_rx: Option<Receiver<DecodedFrame>>,
    latest_frame: Option<DecodedFrame>,

    audio_engine: Option<AudioEngine>,
    pub(crate) media_engine: Option<MediaEngine>,
    pub(crate) media_tx: Option<Sender<MediaCommand>>,
    pub(crate) metadata: Option<VideoMetadata>,

    latest_sequence_id: usize,
    in_flight_seek_seq: Option<usize>,
    desired_scrub_frame: Option<usize>,
    last_sent_seek_frame: Option<usize>,
    was_playing_before_scrub: bool,

    // Headless support
    master_clock: Option<MasterClock>,

    arrow_seek_state: ArrowSeekState,
    is_scrubbing: bool,

    pub(crate) thumbnail_tx: Option<Sender<crate::media::ThumbnailCommand>>,
    pub(crate) thumbnail_rx: Option<Receiver<crate::media::ThumbnailResponse>>,
    thumbnail_threads: Vec<thread::JoinHandle<()>>,
    loudness_profile: Vec<Option<f32>>,
    loudness_rx: Option<crossbeam_channel::Receiver<(usize, f32)>>,
    pub(crate) loudness_shutdown_tx: Option<Sender<()>>,
    loudness_threads: Vec<thread::JoinHandle<()>>,

    pub(crate) defect_map: Arc<RwLock<DefectMap>>,
    concealment_active: bool,
    concealed_held_frame: Option<usize>,
    concealment_start_time: Option<Instant>,
}

impl Default for PlaybackSession {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PlaybackSession {
    fn drop(&mut self) {
        self.shutdown_thumbnail_thread();
        self.shutdown_loudness_thread();
    }
}

impl PlaybackSession {
    pub fn current_frame(&self) -> usize {
        self.current_frame
    }

    pub fn total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing
    }

    pub fn video_fps(&self) -> f64 {
        self.video_fps
    }

    pub fn metadata(&self) -> Option<VideoMetadata> {
        self.metadata.clone()
    }


    pub fn latest_frame(&self) -> Option<&DecodedFrame> {
        self.latest_frame.as_ref()
    }

    pub fn master_clock(&self) -> Option<MasterClock> {
        self.master_clock.clone()
    }

    pub fn defect_map(&self) -> Arc<RwLock<DefectMap>> {
        Arc::clone(&self.defect_map)
    }

    pub fn concealment_telemetry(&self) -> (bool, Option<usize>, f64) {
        (
            self.concealment_active,
            self.concealed_held_frame,
            self.concealment_start_time.map_or(0.0, |t| t.elapsed().as_secs_f64()),
        )
    }

    pub fn new() -> Self {
        Self {
            current_frame: 0,
            total_frames: 0,
            is_playing: false,
            video_fps: 24.0,
            frame_rx: None,
            latest_frame: None,
            audio_engine: None,
            media_engine: None,
            media_tx: None,
            metadata: None,
            latest_sequence_id: 0,
            in_flight_seek_seq: None,
            desired_scrub_frame: None,
            last_sent_seek_frame: None,
            was_playing_before_scrub: false,
            master_clock: None,
            arrow_seek_state: ArrowSeekState::Idle,
            is_scrubbing: false,
            thumbnail_tx: None,
            thumbnail_rx: None,
            thumbnail_threads: Vec::new(),
            loudness_profile: Vec::new(),
            loudness_rx: None,
            loudness_shutdown_tx: None,
            loudness_threads: Vec::new(),
            defect_map: Arc::new(RwLock::new(DefectMap::new())),
            concealment_active: false,
            concealed_held_frame: None,
            concealment_start_time: None,
        }
    }

    pub fn shutdown_thumbnail_thread(&mut self) {
        // Dropping the sender signals the coordinator to exit.
        drop(self.thumbnail_tx.take());
        self.thumbnail_rx = None;
        for handle in self.thumbnail_threads.drain(..) {
            let _ = handle.join();
        }
    }

    pub fn shutdown_loudness_thread(&mut self) {
        drop(self.loudness_shutdown_tx.take());
        for handle in self.loudness_threads.drain(..) {
            let _ = handle.join();
        }
    }

    pub fn load_video(&mut self, path: &str, settings: &crate::settings::Settings) {
        self.shutdown_thumbnail_thread();
        self.shutdown_loudness_thread();
        self.defect_map = Arc::new(RwLock::new(DefectMap::new()));
        self.concealment_active = false;
        self.concealed_held_frame = None;
        self.concealment_start_time = None;
        if let Ok(mut ae) = AudioEngine::new() {
            let channels = ae.channels;
            let audio_tx = ae.tx.take().unwrap();
            let tags_tx = ae.tags_tx.take().unwrap();
            let master_clock = ae.master_clock.clone();
            let (frame_tx, frame_rx) = crossbeam_channel::bounded(30);
            let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
            let (loudness_tx, loudness_rx) = crossbeam_channel::unbounded::<(usize, f32)>();
            
            if let Ok((engine, meta)) = MediaEngine::new(
                path,
                ae.master_clock.sample_rate(),
                channels,
                frame_tx,
                audio_tx,
                tags_tx,
                cmd_rx,
                master_clock.clone(),
                loudness_tx.clone(),
                settings.hardware.playback_hw_accel,
                settings.advanced.presenter_lookahead_depth,
            ) {
                self.video_fps = meta.fps;
                self.total_frames = meta.total_frames;
                self.media_tx = Some(engine.tx.clone());
                self.media_engine = Some(engine);
                self.metadata = Some(meta.clone());
                self.frame_rx = Some(frame_rx);
                self.master_clock = Some(master_clock);
                self.audio_engine = Some(ae);
                self.loudness_profile = vec![None; meta.total_frames * 4];
                self.loudness_rx = Some(loudness_rx);

                let (t_tx, t_rx, t_handles) = crate::media::spawn_thumbnail_decoder_pool(
                    path.to_string(),
                    meta.keyframes.clone(),
                    meta.keyframes_pts.clone(),
                    meta.fps,
                    settings.thumbnails.decoder_threads,
                    settings.hardware.thumbnails_hw_accel,
                );
                self.thumbnail_tx = Some(t_tx);
                self.thumbnail_rx = Some(t_rx);
                self.thumbnail_threads = t_handles;

                let (shutdown_tx, shutdown_rx) = crossbeam_channel::unbounded::<()>();
                let loudness_tx_clone = loudness_tx;
                let path_clone = path.to_string();
                let fps_clone = meta.fps;
                let defect_map_clone = Arc::clone(&self.defect_map);
                let keyframes_clone = meta.keyframes.clone();
                let keyframes_pts_clone = meta.keyframes_pts.clone();
                let handle = thread::spawn(move || {
                    run_background_media_scanner(
                        path_clone,
                        fps_clone,
                        shutdown_rx,
                        loudness_tx_clone,
                        defect_map_clone,
                        keyframes_clone,
                        keyframes_pts_clone,
                    );
                });
                self.loudness_shutdown_tx = Some(shutdown_tx);
                self.loudness_threads = vec![handle];

                self.seek_internal(0, false, true);
            }
        } else {
            println!("[PlaybackSession] No default audio device found. Falling back to real-time paced simulation.");
            let master_clock = MasterClock::new(48000);
            let ticker = master_clock.clone();
            master_clock.set_has_audio(true); // Treat as audio video

            let rb = SharedRb::<Heap<(f32, u64)>>::new(48000 * 2 * 2);
            let (audio_tx, mut audio_cons) = rb.split();

            let tags_rb = SharedRb::<Heap<(u64, u64, u64)>>::new(256);
            let (tags_tx, mut tags_cons) = tags_rb.split();

            let ticker_clone = ticker.clone();
            let _audio_consumer = thread::spawn(move || {
                let sample_rate = 48000;
                let mut start_time = Instant::now();
                let mut samples_read_total = 0u64;
                let mut last_tag = None;
                let mut pending_tag: Option<(u64, u64, u64)> = None;
                let mut last_active = false;
                let mut current_consumer_generation = 0;

                loop {
                    let active = ticker_clone.is_active();

                    // Always drop stale samples if the generation changed, even when inactive
                    let producer_gen = ticker_clone.playback_generation();
                    if producer_gen != current_consumer_generation {
                        while let Some((_, generation)) = audio_cons.try_pop() {
                            if generation == producer_gen {
                                break;
                            }
                        }
                        samples_read_total = 0;
                        last_tag = None;
                        pending_tag = None;
                        current_consumer_generation = producer_gen;
                    }

                    if !active {
                        last_active = false;
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }

                    if !last_active {
                        let already_read_secs = samples_read_total as f64 / sample_rate as f64;
                        start_time = Instant::now() - Duration::from_secs_f64(already_read_secs);
                        last_active = true;
                    }

                    let elapsed = start_time.elapsed();
                    let target_samples = (elapsed.as_secs_f64() * sample_rate as f64) as u64;

                    if target_samples > samples_read_total {
                        let mut to_read = target_samples - samples_read_total;
                        let mut written = 0;

                        while to_read > 0 {
                            if let Some((_sample, sample_generation)) = audio_cons.try_pop() {
                                if sample_generation != current_consumer_generation {
                                    samples_read_total = 0;
                                    last_tag = None;
                                    pending_tag = None;
                                    current_consumer_generation = sample_generation;
                                }

                                written += 1;
                                samples_read_total += 1;
                                to_read -= 1;

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
                            } else {
                                break;
                            }
                        }

                        if written > 0 {
                            ticker_clone.advance(written, last_tag, samples_read_total);
                        }
                    }

                    thread::sleep(Duration::from_millis(5));
                }
            });

            let (frame_tx, frame_rx) = crossbeam_channel::bounded(30);
            let (_cmd_tx, cmd_rx) = crossbeam_channel::unbounded();
            let (loudness_tx, loudness_rx) = crossbeam_channel::unbounded::<(usize, f32)>();

            if let Ok((engine, meta)) = MediaEngine::new(
                path,
                48000,
                1,
                frame_tx,
                audio_tx,
                tags_tx,
                cmd_rx,
                master_clock.clone(),
                loudness_tx.clone(),
                settings.hardware.playback_hw_accel,
                settings.advanced.presenter_lookahead_depth,
            ) {
                self.video_fps = meta.fps;
                self.total_frames = meta.total_frames;
                self.media_tx = Some(engine.tx.clone());
                self.media_engine = Some(engine);
                self.metadata = Some(meta.clone());
                self.frame_rx = Some(frame_rx);
                self.master_clock = Some(master_clock);
                self.audio_engine = None;
                self.loudness_profile = vec![None; meta.total_frames * 4];
                self.loudness_rx = Some(loudness_rx);

                let (t_tx, t_rx, t_handles) = crate::media::spawn_thumbnail_decoder_pool(
                    path.to_string(),
                    meta.keyframes.clone(),
                    meta.keyframes_pts.clone(),
                    meta.fps,
                    settings.thumbnails.decoder_threads,
                    settings.hardware.thumbnails_hw_accel,
                );
                self.thumbnail_tx = Some(t_tx);
                self.thumbnail_rx = Some(t_rx);
                self.thumbnail_threads = t_handles;

                let (shutdown_tx, shutdown_rx) = crossbeam_channel::unbounded::<()>();
                let loudness_tx_clone = loudness_tx;
                let path_clone = path.to_string();
                let fps_clone = meta.fps;
                let defect_map_clone = Arc::clone(&self.defect_map);
                let keyframes_clone = meta.keyframes.clone();
                let keyframes_pts_clone = meta.keyframes_pts.clone();
                let handle = thread::spawn(move || {
                    run_background_media_scanner(
                        path_clone,
                        fps_clone,
                        shutdown_rx,
                        loudness_tx_clone,
                        defect_map_clone,
                        keyframes_clone,
                        keyframes_pts_clone,
                    );
                });
                self.loudness_shutdown_tx = Some(shutdown_tx);
                self.loudness_threads = vec![handle];

                self.seek_internal(0, false, true);
            }
        }
    }

    pub fn update(&mut self) -> (Option<&DecodedFrame>, usize, bool) {
        let mut got_frame = false;
        let mut frames_received = 0;

        if let Some(rx) = &self.frame_rx {
            while let Ok(frame) = rx.try_recv() {
                if frame.sequence_id() < self.latest_sequence_id {
                    continue;
                }
                self.latest_frame = Some(frame);
                got_frame = true;
                frames_received += 1;
            }

            if let Some(frame) = &self.latest_frame
                && got_frame {
                    if frame.is_concealed() {
                        if !self.concealment_active {
                            self.concealment_active = true;
                            self.concealment_start_time = Some(Instant::now());
                            self.concealed_held_frame = frame.held_frame_idx();
                        }
                    } else {
                        self.concealment_active = false;
                        self.concealment_start_time = None;
                        self.concealed_held_frame = None;
                    }

                    let frame_idx = frame.frame_idx();
                    
                    if self.in_flight_seek_seq.is_some_and(|seq| frame.sequence_id() >= seq) {
                        self.in_flight_seek_seq = None;
                        
                        if let Some(desired) = self.desired_scrub_frame.take()
                            && self.last_sent_seek_frame != Some(desired) {
                                self.seek_internal(desired, true, false);
                                self.in_flight_seek_seq = Some(self.latest_sequence_id);
                            }
                    }

                    if !self.is_playing || self.in_flight_seek_seq.is_none() {
                        self.current_frame = frame_idx;
                    }
                }
        }
        
        if self.current_frame >= self.total_frames {
            self.current_frame = self.total_frames;
            self.set_playing(false);
        }
        
        // Drain loudness side-channel — write each bucket at most once
        let mut loudness_updated = false;
        if let Some(rx) = &self.loudness_rx {
            while let Ok((bucket_idx, rms)) = rx.try_recv() {
                if let Some(slot) = self.loudness_profile.get_mut(bucket_idx) {
                    if slot.is_none() {
                        *slot = Some(rms);
                        loudness_updated = true;
                    }
                }
            }
        }
        
        (self.latest_frame.as_ref(), frames_received, loudness_updated)
    }

    pub fn loudness_profile(&self) -> &[Option<f32>] {
        &self.loudness_profile
    }

    pub fn set_playing(&mut self, play: bool) {
        if self.is_playing == play { return; }
        if let Some(tx) = &self.media_tx {
            let cmd = if play { MediaCommand::Play } else { MediaCommand::Pause };
            let _ = tx.send(cmd);
        }
        self.is_playing = play;
        if let Some(mc) = &self.master_clock {
            if play { mc.play(); } else { mc.pause(); }
        }
    }

    pub fn toggle_playback(&mut self) {
        self.set_playing(!self.is_playing);
    }

    pub fn seek_internal(&mut self, frame: usize, keyframe_only: bool, audio_enabled: bool) {
        self.latest_sequence_id += 1;
        self.last_sent_seek_frame = Some(frame);
        if let Some(tx) = &self.media_tx {
            let _ = tx.send(MediaCommand::Seek {
                frame_index: frame,
                keyframe_only,
                sequence_id: self.latest_sequence_id,
                audio_enabled,
            });
        }
        self.current_frame = frame;
        if let Some(mc) = &self.master_clock {
            mc.reset(frame as f64 / self.video_fps);
        }
    }

    pub fn scrub_to(&mut self, target_frame: usize) {
        self.set_playing(false);
        if self.in_flight_seek_seq.is_some() {
            self.desired_scrub_frame = Some(target_frame);
            self.current_frame = target_frame;
        } else {
            self.seek_internal(target_frame, true, false);
            self.in_flight_seek_seq = Some(self.latest_sequence_id);
            self.desired_scrub_frame = None;
        }
    }

    pub fn handle_input(&mut self, event: InputEvent) {
        match event {
            InputEvent::ArrowLeftPressed => {
                if matches!(self.arrow_seek_state, ArrowSeekState::Idle) {
                    self.set_playing(false);
                    self.arrow_seek_state = ArrowSeekState::SteppingLeft {
                        held_since: Instant::now(),
                    };
                    self.current_frame = self.current_frame.saturating_sub(1);
                    self.seek_internal(self.current_frame, false, false);
                    self.in_flight_seek_seq = Some(self.latest_sequence_id);
                }
            }
            InputEvent::ArrowLeftDown => {
                if let ArrowSeekState::SteppingLeft { held_since } = self.arrow_seek_state
                    && held_since.elapsed() >= Duration::from_millis(250)
                        && self.in_flight_seek_seq.is_none() {
                            self.is_playing = false;
                            let target_frame = self.current_frame.saturating_sub(1);
                            self.seek_internal(target_frame, false, false);
                            self.in_flight_seek_seq = Some(self.latest_sequence_id);
                        }
            }
            InputEvent::ArrowLeftReleased => {
                if matches!(self.arrow_seek_state, ArrowSeekState::SteppingLeft { .. }) {
                    self.arrow_seek_state = ArrowSeekState::Idle;
                }
            }
            InputEvent::ArrowRightPressed => {
                if matches!(self.arrow_seek_state, ArrowSeekState::Idle) {
                    self.set_playing(false);
                    self.arrow_seek_state = ArrowSeekState::SteppingRight {
                        held_since: Instant::now(),
                        unpaced_started: false,
                    };
                    self.current_frame = (self.current_frame + 1).min(self.total_frames);
                    self.seek_internal(self.current_frame, false, false);
                    self.in_flight_seek_seq = Some(self.latest_sequence_id);
                }
            }
            InputEvent::ArrowRightDown => {
                if let ArrowSeekState::SteppingRight { held_since, unpaced_started } = self.arrow_seek_state
                    && held_since.elapsed() >= Duration::from_millis(250)
                        && !unpaced_started {
                            self.latest_sequence_id += 1;
                            if let Some(tx) = &self.media_tx {
                                let _ = tx.send(MediaCommand::PlayUnpaced { sequence_id: self.latest_sequence_id });
                            }
                            self.arrow_seek_state = ArrowSeekState::SteppingRight {
                                held_since,
                                unpaced_started: true,
                            };
                        }
            }
            InputEvent::ArrowRightReleased => {
                if let ArrowSeekState::SteppingRight { unpaced_started, .. } = self.arrow_seek_state {
                    if unpaced_started {
                        if let Some(tx) = &self.media_tx {
                            let _ = tx.send(MediaCommand::Pause);
                        }
                        self.is_playing = false;
                        if let Some(ae) = &self.audio_engine {
                            ae.master_clock.reset(self.current_frame as f64 / self.video_fps);
                        }
                    }
                    self.arrow_seek_state = ArrowSeekState::Idle;
                }
            }
            InputEvent::ScrubDragStart => {
                self.was_playing_before_scrub = self.is_playing;
                self.is_scrubbing = true;
            }
            InputEvent::ScrubDragUpdate { target_frame } => {
                self.scrub_to(target_frame);
            }
            InputEvent::ScrubDragEnd => {
                let target = self.desired_scrub_frame.unwrap_or(
                    self.latest_frame.as_ref().map(|f| f.frame_idx()).unwrap_or(self.current_frame)
                );
                self.current_frame = target;
                self.is_playing = self.was_playing_before_scrub;
                self.in_flight_seek_seq = None;
                self.desired_scrub_frame = None;
                self.seek_internal(target, false, true);
                self.set_playing(self.is_playing);
                self.is_scrubbing = false;
            }
        }
    }

    pub fn is_scrubbing(&self) -> bool {
        self.is_scrubbing
    }

    pub fn is_stepping(&self) -> bool {
        matches!(self.arrow_seek_state, ArrowSeekState::SteppingLeft { .. } | ArrowSeekState::SteppingRight { .. })
    }

    pub fn step_forward(&mut self) {
        self.handle_input(InputEvent::ArrowRightPressed);
        self.handle_input(InputEvent::ArrowRightReleased);
    }

    pub fn step_backward(&mut self) {
        self.handle_input(InputEvent::ArrowLeftPressed);
        self.handle_input(InputEvent::ArrowLeftReleased);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playback_session_scrub_states() {
        let mut session = PlaybackSession::new();
        assert!(!session.is_scrubbing());

        session.handle_input(InputEvent::ScrubDragStart);
        assert!(session.is_scrubbing());

        session.handle_input(InputEvent::ScrubDragUpdate { target_frame: 42 });
        assert_eq!(session.current_frame(), 42);
        assert!(session.is_scrubbing());

        session.handle_input(InputEvent::ScrubDragEnd);
        assert!(!session.is_scrubbing());
    }

    #[test]
    fn test_playback_session_arrow_keys() {
        let mut session = PlaybackSession::new();
        assert!(!session.is_stepping());

        session.handle_input(InputEvent::ArrowLeftPressed);
        assert!(session.is_stepping());

        session.handle_input(InputEvent::ArrowLeftReleased);
        assert!(!session.is_stepping());

        session.handle_input(InputEvent::ArrowRightPressed);
        assert!(session.is_stepping());

        session.handle_input(InputEvent::ArrowRightReleased);
        assert!(!session.is_stepping());
    }

    #[test]
    fn test_arrow_seek_stop_sends_pause() {
        let mut session = PlaybackSession::new();
        let (tx, rx) = crossbeam_channel::unbounded();
        session.media_tx = Some(tx);

        session.handle_input(InputEvent::ArrowRightPressed);
        
        // Emulate holding for 250ms+
        session.arrow_seek_state = ArrowSeekState::SteppingRight {
            held_since: Instant::now() - Duration::from_millis(300),
            unpaced_started: false,
        };

        session.handle_input(InputEvent::ArrowRightDown);
        
        // Consume the Seek command from ArrowRightPressed
        let _seek_cmd = rx.try_recv().unwrap();

        // Assert PlayUnpaced was sent
        let cmd = rx.try_recv().unwrap();
        assert!(matches!(cmd, MediaCommand::PlayUnpaced { .. }));

        // Now release
        session.handle_input(InputEvent::ArrowRightReleased);
        
        // Assert Pause was sent
        let cmd2 = rx.try_recv().unwrap();
        assert!(matches!(cmd2, MediaCommand::Pause));
        assert!(!session.is_playing());
    }

    #[test]
    fn test_scrub_end_uses_desired_frame() {
        let mut session = PlaybackSession::new();
        let (tx, rx) = crossbeam_channel::unbounded();
        session.media_tx = Some(tx);

        session.handle_input(InputEvent::ScrubDragStart);
        
        // Simulate dragging to frame 150
        session.in_flight_seek_seq = Some(1);
        session.handle_input(InputEvent::ScrubDragUpdate { target_frame: 150 });
        assert_eq!(session.desired_scrub_frame, Some(150));

        // Now release drag
        session.handle_input(InputEvent::ScrubDragEnd);
        
        // Verify it sought exactly to 150
        let cmd = rx.try_recv().unwrap();
        match cmd {
            MediaCommand::Seek { frame_index, keyframe_only, .. } => {
                assert_eq!(frame_index, 150);
                assert_eq!(keyframe_only, false);
            }
            _ => panic!("Expected Seek command"),
        }
    }

    #[test]
    fn test_playback_session_defect_map() {
        let session = PlaybackSession::new();
        let map_arc = session.defect_map();
        let (ranges, max_sec, total_pkts) = map_arc.read().unwrap().summary();
        assert_eq!(ranges, 0);
        assert_eq!(total_pkts, 0);
        assert_eq!(max_sec, 0.0);

        map_arc.write().unwrap().insert_or_coalesce(
            2.5,
            0.1,
            StreamTypeMask::VIDEO,
            DefectErrorType::CorruptBitstreamPayload,
            Some(3),
        );

        let (ranges_after, _, total_after) = session.defect_map().read().unwrap().summary();
        assert_eq!(ranges_after, 1);
        assert_eq!(total_after, 1);
    }

    #[test]
    fn test_playback_session_concealment_telemetry() {
        let mut session = PlaybackSession::new();
        let (active, held, dur) = session.concealment_telemetry();
        assert!(!active);
        assert_eq!(held, None);
        assert_eq!(dur, 0.0);

        let (frame_tx, frame_rx) = crossbeam_channel::unbounded();
        session.frame_rx = Some(frame_rx);

        // Send a concealed frame holding frame index 10
        let concealed_frame = DecodedFrame::Cpu {
            frame_idx: 12,
            planes: std::sync::Arc::new(crate::media::types::YuvPlanes::default()),
            sequence_id: 0,
            pts_sec: 0.5,
            color_space: ffmpeg_next::color::Space::Unspecified,
            color_range: ffmpeg_next::color::Range::Unspecified,
            is_concealed: true,
            held_frame_idx: Some(10),
        };
        frame_tx.send(concealed_frame).unwrap();

        session.update();
        let (active2, held2, dur2) = session.concealment_telemetry();
        assert!(active2);
        assert_eq!(held2, Some(10));
        assert!(dur2 >= 0.0);

        // Send a regular (non-concealed) frame
        let normal_frame = DecodedFrame::Cpu {
            frame_idx: 13,
            planes: std::sync::Arc::new(crate::media::types::YuvPlanes::default()),
            sequence_id: 0,
            pts_sec: 0.54,
            color_space: ffmpeg_next::color::Space::Unspecified,
            color_range: ffmpeg_next::color::Range::Unspecified,
            is_concealed: false,
            held_frame_idx: None,
        };
        frame_tx.send(normal_frame).unwrap();

        session.update();
        let (active3, held3, dur3) = session.concealment_telemetry();
        assert!(!active3);
        assert_eq!(held3, None);
        assert_eq!(dur3, 0.0);
    }
}

struct PendingDefect {
    pts_sec: f64,
    duration_sec: f64,
    stream: StreamTypeMask,
    error: DefectErrorType,
    gop_idx: Option<usize>,
}

fn flush_pending_defects(
    defect_map: &Arc<RwLock<DefectMap>>,
    pending: &mut Vec<PendingDefect>,
) {
    if !pending.is_empty() {
        if let Ok(mut map) = defect_map.write() {
            for d in pending.drain(..) {
                map.insert_or_coalesce(d.pts_sec, d.duration_sec, d.stream, d.error, d.gop_idx);
            }
        }
    }
}

pub fn run_background_media_scanner(
    path: String,
    fps: f64,
    shutdown_rx: Receiver<()>,
    loudness_tx: Sender<(usize, f32)>,
    defect_map: Arc<RwLock<DefectMap>>,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
) {
    let _ = keyframes;
    ffmpeg_next::init().ok();
    let mut input = match ffmpeg_next::format::input(&path) {
        Ok(i) => i,
        Err(_) => return,
    };

    let video_stream_info = input.streams().best(ffmpeg_next::media::Type::Video).map(|s| {
        let idx = s.index();
        let tb = s.time_base();
        let tb_f64 = if tb.denominator() > 0 {
            tb.numerator() as f64 / tb.denominator() as f64
        } else {
            1.0 / 1_000_000.0
        };
        (idx, tb_f64)
    });

    let audio_stream_info = input.streams().best(ffmpeg_next::media::Type::Audio).map(|s| {
        let idx = s.index();
        let tb = s.time_base();
        let tb_f64 = if tb.denominator() > 0 {
            tb.numerator() as f64 / tb.denominator() as f64
        } else {
            1.0 / 1_000_000.0
        };
        let params = s.parameters();
        (idx, tb_f64, params)
    });

    let mut audio_pipeline = if let Some((_, audio_tb_f64, ref params)) = audio_stream_info {
        if let Ok(codec_ctx) = ffmpeg_next::codec::context::Context::from_parameters(params.clone()) {
            if let Ok(decoder) = codec_ctx.decoder().audio() {
                Some(crate::media::audio_pipeline::AudioDecoderPipeline::new(
                    decoder,
                    48000,
                    1,
                    audio_tb_f64,
                    0.0,
                ))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let nominal_video_frame_interval = if fps > 0.0 { 1.0 / fps } else { 1.0 / 24.0 };
    let mut nominal_audio_frame_duration = 1024.0 / 48000.0;

    let mut last_video_dts: Option<f64> = None;
    let mut last_audio_pts: Option<f64> = None;
    let mut pending_defects: Vec<PendingDefect> = Vec::with_capacity(32);

    let mut packets = input.packets();
    let mut check_counter = 0usize;

    while let Some((stream, packet)) = packets.next() {
        check_counter += 1;
        if check_counter % 64 == 0 {
            if shutdown_rx.try_recv().is_ok() {
                break;
            }
            flush_pending_defects(&defect_map, &mut pending_defects);
        }

        let s_idx = stream.index();

        if let Some((video_idx, video_tb_f64)) = video_stream_info {
            if s_idx == video_idx {
                let is_corrupt = packet.is_corrupt()
                    || (packet.flags().bits() & (ffmpeg_next::sys::AV_PKT_FLAG_CORRUPT as i32)) != 0;

                let pkt_pts_sec = packet
                    .pts()
                    .or_else(|| packet.dts())
                    .map(|p| p as f64 * video_tb_f64)
                    .unwrap_or(0.0);

                let pkt_dur_sec = if packet.duration() > 0 {
                    (packet.duration() as f64 * video_tb_f64).max(0.001)
                } else {
                    nominal_video_frame_interval
                };

                let gop_idx = if keyframes_pts.is_empty() {
                    None
                } else {
                    let pkt_pts_i64 = packet.pts().or_else(|| packet.dts()).unwrap_or(0);
                    match keyframes_pts.binary_search(&pkt_pts_i64) {
                        Ok(idx) => Some(idx),
                        Err(0) => Some(0),
                        Err(idx) => Some(idx - 1),
                    }
                };

                if is_corrupt {
                    pending_defects.push(PendingDefect {
                        pts_sec: pkt_pts_sec,
                        duration_sec: pkt_dur_sec,
                        stream: StreamTypeMask::VIDEO,
                        error: DefectErrorType::CorruptBitstreamPayload,
                        gop_idx,
                    });
                }

                if let Some(dts) = packet.dts().or_else(|| packet.pts()) {
                    let current_dts = dts as f64 * video_tb_f64;
                    if let Some(last_dts) = last_video_dts {
                        let delta_dts = current_dts - last_dts;
                        if delta_dts > 2.0 * nominal_video_frame_interval || delta_dts < -0.001 {
                            let defect_dur = if delta_dts > 2.0 * nominal_video_frame_interval {
                                (delta_dts - nominal_video_frame_interval).max(nominal_video_frame_interval)
                            } else {
                                nominal_video_frame_interval
                            };
                            pending_defects.push(PendingDefect {
                                pts_sec: pkt_pts_sec,
                                duration_sec: defect_dur,
                                stream: StreamTypeMask::VIDEO,
                                error: DefectErrorType::TimestampDiscontinuity,
                                gop_idx,
                            });
                        }
                    }
                    last_video_dts = Some(current_dts);
                }
            }
        }

        if let Some((audio_idx, audio_tb_f64, _)) = audio_stream_info {
            if s_idx == audio_idx {
                if let Some(pts) = packet.pts().or_else(|| packet.dts()) {
                    let current_pts = pts as f64 * audio_tb_f64;
                    let pkt_dur = if packet.duration() > 0 {
                        packet.duration() as f64 * audio_tb_f64
                    } else {
                        nominal_audio_frame_duration
                    };
                    if pkt_dur > 0.001 {
                        nominal_audio_frame_duration = pkt_dur;
                    }

                    if let Some(last_pts) = last_audio_pts {
                        let delta_pts = current_pts - last_pts;
                        if delta_pts > 1.5 * nominal_audio_frame_duration {
                            let defect_dur = (delta_pts - nominal_audio_frame_duration).max(nominal_audio_frame_duration);
                            pending_defects.push(PendingDefect {
                                pts_sec: last_pts + nominal_audio_frame_duration,
                                duration_sec: defect_dur,
                                stream: StreamTypeMask::AUDIO,
                                error: DefectErrorType::TimestampDiscontinuity,
                                gop_idx: None,
                            });
                        }
                    }
                    last_audio_pts = Some(current_pts);
                }

                if let Some(ref mut pipeline) = audio_pipeline {
                    if pipeline.send_packet(&packet).is_ok() {
                        if let Ok(chunks) = pipeline.receive_samples() {
                            for chunk in chunks {
                                let buckets = crate::media::audio_pipeline::compute_rms_buckets(
                                    &chunk, fps, 1, 48000,
                                );
                                for (b_idx, rms) in buckets {
                                    let _ = loudness_tx.send((b_idx, rms));
                                }
                            }
                        }
                    }
                }
            }
        }

        if pending_defects.len() >= 64 {
            flush_pending_defects(&defect_map, &mut pending_defects);
        }
    }

    flush_pending_defects(&defect_map, &mut pending_defects);
}
