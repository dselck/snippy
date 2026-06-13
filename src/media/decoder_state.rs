use crate::media::{MediaCommand, DecodedFrame};
use crate::media::decoder_pipeline::VideoDecoderPipeline;
use crate::media::audio_pipeline::AudioDecoderPipeline;
use std::time::Duration;

pub enum DecoderEvent {
    Command(MediaCommand),
    PacketDemuxed { packet: ffmpeg_next::Packet, stream_index: usize },
    QueueStatus { video_queue_len: usize, audio_vacant_len: usize },
}

impl std::fmt::Debug for DecoderEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecoderEvent::Command(cmd) => f.debug_tuple("Command").field(cmd).finish(),
            DecoderEvent::PacketDemuxed { stream_index, .. } => {
                f.debug_struct("PacketDemuxed")
                    .field("stream_index", stream_index)
                    .finish()
            }
            DecoderEvent::QueueStatus { video_queue_len, audio_vacant_len } => {
                f.debug_struct("QueueStatus")
                    .field("video_queue_len", video_queue_len)
                    .field("audio_vacant_len", audio_vacant_len)
                    .finish()
            }
        }
    }
}

#[derive(Debug)]
pub enum DecoderAction {
    DemuxNextPacket,
    SeekDemuxer { pts: i64 },
    PushVideo(DecodedFrame),
    PushAudio { data: Vec<f32>, tag: Option<(u64, u64)>, generation: u64 },
    ScheduleWakeup(Duration),
}

impl PartialEq for DecoderAction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DecoderAction::DemuxNextPacket, DecoderAction::DemuxNextPacket) => true,
            (DecoderAction::SeekDemuxer { pts: p1 }, DecoderAction::SeekDemuxer { pts: p2 }) => p1 == p2,
            (DecoderAction::PushVideo(f1), DecoderAction::PushVideo(f2)) => {
                f1.frame_idx() == f2.frame_idx() && f1.sequence_id() == f2.sequence_id() && f1.pts_sec() == f2.pts_sec()
            }
            (DecoderAction::PushAudio { data: d1, tag: t1, generation: g1 },
             DecoderAction::PushAudio { data: d2, tag: t2, generation: g2 }) => {
                d1 == d2 && t1 == t2 && g1 == g2
            }
            (DecoderAction::ScheduleWakeup(d1), DecoderAction::ScheduleWakeup(d2)) => d1 == d2,
            _ => false,
        }
    }
}

pub struct DecoderStateMachine {
    pub video_pipeline: VideoDecoderPipeline,
    pub audio_pipeline_opt: Option<AudioDecoderPipeline>,
    master_clock: crate::audio::clock::MasterClock,
    keyframes: Vec<usize>,
    keyframes_pts: Vec<i64>,
    audio_enabled: bool,
    fps: f64,
    video_stream_index: usize,
    audio_stream_index_opt: Option<usize>,
    has_audio_stream: bool,
}

fn get_nearest_keyframe(keyframes: &[usize], target: usize) -> usize {
    if keyframes.is_empty() {
        return target;
    }
    match keyframes.binary_search(&target) {
        Ok(i) => keyframes[i],
        Err(0) => keyframes[0],
        Err(i) => keyframes[i - 1],
    }
}

impl DecoderStateMachine {
    pub fn new(
        video_pipeline: VideoDecoderPipeline,
        audio_pipeline_opt: Option<AudioDecoderPipeline>,
        master_clock: crate::audio::clock::MasterClock,
        keyframes: Vec<usize>,
        keyframes_pts: Vec<i64>,
        fps: f64,
        video_stream_index: usize,
        audio_stream_index_opt: Option<usize>,
        has_audio_stream: bool,
    ) -> Self {
        Self {
            video_pipeline,
            audio_pipeline_opt,
            master_clock,
            keyframes,
            keyframes_pts,
            audio_enabled: true,
            fps,
            video_stream_index,
            audio_stream_index_opt,
            has_audio_stream,
        }
    }

    pub fn is_decoding_active(&self) -> bool {
        self.video_pipeline.is_active()
    }

    pub fn audio_enabled(&self) -> bool {
        self.audio_enabled
    }

    pub fn step(&mut self, event: DecoderEvent) -> Vec<DecoderAction> {
        let mut actions = Vec::new();

        match event {
            DecoderEvent::Command(cmd) => {
                match cmd {
                    MediaCommand::Shutdown => {
                        // Handled by adapter thread
                    }
                    MediaCommand::Play => {
                        if !self.audio_enabled {
                            self.audio_enabled = true;
                            self.master_clock.set_has_audio(self.has_audio_stream && self.audio_enabled);
                            if let Some(audio_pipeline) = self.audio_pipeline_opt.as_mut() {
                                let current_frame = self.video_pipeline.fallback_frame_idx;
                                audio_pipeline.handle_seek(current_frame as f64 / self.fps);
                                self.master_clock.inc_playback_generation();
                            }
                        }
                        self.video_pipeline.play();
                    }
                    MediaCommand::PlayUnpaced { sequence_id } => {
                        self.audio_enabled = false;
                        self.master_clock.set_has_audio(false);
                        self.video_pipeline.play_unpaced(sequence_id);
                    }
                    MediaCommand::Pause => {
                        self.video_pipeline.pause();
                    }
                    MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled: ae } => {
                        self.audio_enabled = ae;
                        self.master_clock.set_has_audio(self.has_audio_stream && self.audio_enabled);
                        let handled = self.video_pipeline.seek(frame_index, keyframe_only, sequence_id, self.audio_enabled);

                        if let Some(audio_pipeline) = self.audio_pipeline_opt.as_mut() {
                            if self.audio_enabled {
                                audio_pipeline.handle_seek(frame_index as f64 / self.fps);
                                self.master_clock.inc_playback_generation();
                            }
                        }

                        if handled {
                            if let Some(frame) = self.video_pipeline.get_cached_frame(frame_index) {
                                actions.push(DecoderAction::PushVideo(frame));
                            }
                        } else {
                            let target_kf = get_nearest_keyframe(&self.keyframes, frame_index);
                            let kf_pts = self.keyframes.binary_search(&target_kf)
                                .map(|idx| self.keyframes_pts[idx])
                                .unwrap_or(0);

                            actions.push(DecoderAction::SeekDemuxer { pts: kf_pts });
                        }
                    }
                }
            }
            DecoderEvent::PacketDemuxed { packet, stream_index } => {
                if stream_index == self.video_stream_index {
                    if let Ok(frames) = self.video_pipeline.process_packet(&packet) {
                        for frame in frames {
                            actions.push(DecoderAction::PushVideo(frame));
                        }
                    }
                } else if Some(stream_index) == self.audio_stream_index_opt && self.audio_enabled {
                    if let Some(audio_pipeline) = self.audio_pipeline_opt.as_mut() {
                        if let Some(pts) = packet.pts() {
                            audio_pipeline.update_current_time_from_pts(pts);
                        }
                        if audio_pipeline.send_packet(&packet).is_ok() {
                            if let Ok(chunks) = audio_pipeline.receive_samples() {
                                let generation = self.master_clock.playback_generation();
                                for chunk in chunks {
                                    actions.push(DecoderAction::PushAudio {
                                        data: chunk.data,
                                        tag: chunk.tag,
                                        generation,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            DecoderEvent::QueueStatus { video_queue_len, audio_vacant_len } => {
                if self.video_pipeline.is_active() {
                    let audio_needs_frames = self.audio_enabled && (audio_vacant_len >= 2048);
                    let queue_needs_frames = (video_queue_len < 20) || audio_needs_frames;
                    if queue_needs_frames {
                        actions.push(DecoderAction::DemuxNextPacket);
                    }
                }
            }
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::decoder_pipeline::{DecoderAdapter, DecodeError};
    use crate::audio::clock::MasterClock;

    struct MockDecoderAdapter;
    impl DecoderAdapter for MockDecoderAdapter {
        fn send_packet(&mut self, _packet: &ffmpeg_next::Packet) -> Result<(), DecodeError> {
            Ok(())
        }
        fn receive_frame(&mut self) -> Result<Option<DecodedFrame>, DecodeError> {
            Ok(None)
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn test_decoder_play_activates_pipeline() {
        let adapter = Box::new(MockDecoderAdapter);
        let video_pipeline = VideoDecoderPipeline::new(adapter, vec![0, 10]);
        let (master_clock, _ticker) = MasterClock::new(44100);
        let mut sm = DecoderStateMachine::new(
            video_pipeline,
            None,
            master_clock,
            vec![0, 10],
            vec![0, 1000],
            24.0,
            0,
            None,
            false,
        );

        assert!(sm.is_decoding_active());
        sm.step(DecoderEvent::Command(MediaCommand::Pause));
        assert!(!sm.is_decoding_active());
        sm.step(DecoderEvent::Command(MediaCommand::Play));
        assert!(sm.is_decoding_active());
    }

    #[test]
    fn test_decoder_queue_status_triggers_demux() {
        let adapter = Box::new(MockDecoderAdapter);
        let video_pipeline = VideoDecoderPipeline::new(adapter, vec![0, 10]);
        let (master_clock, _ticker) = MasterClock::new(44100);
        let mut sm = DecoderStateMachine::new(
            video_pipeline,
            None,
            master_clock,
            vec![0, 10],
            vec![0, 1000],
            24.0,
            0,
            None,
            false,
        );

        sm.step(DecoderEvent::Command(MediaCommand::Play));
        let actions = sm.step(DecoderEvent::QueueStatus {
            video_queue_len: 5,
            audio_vacant_len: 1000,
        });

        assert!(actions.contains(&DecoderAction::DemuxNextPacket));
    }
}
