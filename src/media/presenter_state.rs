use crate::media::{MediaCommand, DecodedFrame};
use crate::media::sync::{VideoSyncEngine, SyncAction};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PresenterState {
    Paused,
    Playing,
    PlayingUnpaced { sequence_id: usize },
    Seeking {
        target_frame_idx: Option<usize>,
        sequence_id: usize,
        resume_playing_after: bool,
    },
}

#[derive(Debug)]
pub enum PresenterEvent {
    Command(MediaCommand),
    FrameDecoded(DecodedFrame),
    Wakeup,
}

#[derive(Debug)]
pub enum PresenterAction {
    ClearLookaheadQueue,
    SendDecoderCommand(MediaCommand),
    DispatchFrame(DecodedFrame),
    ScheduleWakeup(Duration),
}

impl PartialEq for PresenterAction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (PresenterAction::ClearLookaheadQueue, PresenterAction::ClearLookaheadQueue) => true,
            (PresenterAction::SendDecoderCommand(cmd1), PresenterAction::SendDecoderCommand(cmd2)) => {
                match (cmd1, cmd2) {
                    (MediaCommand::Play, MediaCommand::Play) => true,
                    (MediaCommand::Pause, MediaCommand::Pause) => true,
                    (MediaCommand::Shutdown, MediaCommand::Shutdown) => true,
                    (MediaCommand::PlayUnpaced { sequence_id: s1 }, MediaCommand::PlayUnpaced { sequence_id: s2 }) => s1 == s2,
                    (MediaCommand::Seek { frame_index: f1, keyframe_only: k1, sequence_id: s1, audio_enabled: a1 },
                     MediaCommand::Seek { frame_index: f2, keyframe_only: k2, sequence_id: s2, audio_enabled: a2 }) => {
                        f1 == f2 && k1 == k2 && s1 == s2 && a1 == a2
                    }
                    _ => false,
                }
            }
            (PresenterAction::DispatchFrame(f1), PresenterAction::DispatchFrame(f2)) => {
                f1.frame_idx() == f2.frame_idx() && f1.sequence_id() == f2.sequence_id() && f1.pts_sec() == f2.pts_sec()
            }
            (PresenterAction::ScheduleWakeup(d1), PresenterAction::ScheduleWakeup(d2)) => d1 == d2,
            _ => false,
        }
    }
}

pub struct PresenterStateMachine {
    sync_engine: VideoSyncEngine,
    state: PresenterState,
    current_frame: Option<DecodedFrame>,
    lookahead_queue: std::collections::VecDeque<DecodedFrame>,
    last_wait_frame: Option<(usize, usize)>,
}

impl PresenterStateMachine {
    pub fn new(sync_engine: VideoSyncEngine) -> Self {
        Self {
            sync_engine,
            state: PresenterState::Paused,
            current_frame: None,
            lookahead_queue: std::collections::VecDeque::new(),
            last_wait_frame: None,
        }
    }

    pub fn state(&self) -> PresenterState {
        self.state
    }

    pub fn step(&mut self, event: PresenterEvent) -> Vec<PresenterAction> {
        let mut actions = Vec::new();
        match event {
            PresenterEvent::Command(cmd) => {
                match cmd {
                    MediaCommand::Shutdown => {
                        actions.push(PresenterAction::SendDecoderCommand(MediaCommand::Shutdown));
                    }
                    MediaCommand::Play => {
                        self.state = PresenterState::Playing;
                        self.lookahead_queue.clear();
                        self.current_frame = None;
                        self.sync_engine.reset();
                        actions.push(PresenterAction::ClearLookaheadQueue);
                        actions.push(PresenterAction::SendDecoderCommand(MediaCommand::Play));
                    }
                    MediaCommand::PlayUnpaced { sequence_id } => {
                        self.state = PresenterState::PlayingUnpaced { sequence_id };
                        self.lookahead_queue.clear();
                        self.current_frame = None;
                        actions.push(PresenterAction::ClearLookaheadQueue);
                        actions.push(PresenterAction::SendDecoderCommand(MediaCommand::PlayUnpaced { sequence_id }));
                    }
                    MediaCommand::Pause => {
                        self.state = PresenterState::Paused;
                        self.lookahead_queue.clear();
                        self.current_frame = None;
                        actions.push(PresenterAction::ClearLookaheadQueue);
                        actions.push(PresenterAction::SendDecoderCommand(MediaCommand::Pause));
                    }
                    MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled } => {
                        let resume_playing_after = match self.state {
                            PresenterState::Playing | PresenterState::PlayingUnpaced { .. } => true,
                            PresenterState::Seeking { resume_playing_after: r, .. } => r,
                            PresenterState::Paused => false,
                        };
                        self.state = PresenterState::Seeking {
                            target_frame_idx: Some(frame_index),
                            sequence_id,
                            resume_playing_after,
                        };
                        self.current_frame = None;
                        self.lookahead_queue.clear();
                        actions.push(PresenterAction::ClearLookaheadQueue);
                        actions.push(PresenterAction::SendDecoderCommand(MediaCommand::Seek {
                            frame_index,
                            keyframe_only,
                            sequence_id,
                            audio_enabled,
                        }));
                    }
                }
            }
            PresenterEvent::FrameDecoded(frame) => {
                match &mut self.state {
                    PresenterState::Playing | PresenterState::PlayingUnpaced { .. } => {
                        self.lookahead_queue.push_back(frame);
                    }
                    PresenterState::Seeking { target_frame_idx, sequence_id, resume_playing_after } => {
                        let seq = frame.sequence_id();
                        if seq == *sequence_id {
                            if let Some(target_idx) = *target_frame_idx {
                                let f_idx = frame.frame_idx();
                                actions.push(PresenterAction::DispatchFrame(frame.clone()));
                                if f_idx >= target_idx {
                                    *target_frame_idx = None;
                                }
                            }
                            if *resume_playing_after {
                                self.state = PresenterState::Playing;
                                self.sync_engine.reset();
                                actions.push(PresenterAction::SendDecoderCommand(MediaCommand::Play));
                            }
                        }
                    }
                    PresenterState::Paused => {}
                }
            }
            PresenterEvent::Wakeup => {}
        }

        // Evaluate frames presentation loop
        loop {
            if self.current_frame.is_none() {
                if matches!(self.state, PresenterState::Playing | PresenterState::PlayingUnpaced { .. }) {
                    self.current_frame = self.lookahead_queue.pop_front();
                }
            }

            if let Some(frame) = &self.current_frame {
                match self.state {
                    PresenterState::Playing => {
                        let pts_sec = frame.pts_sec();
                        match self.sync_engine.evaluate_frame(pts_sec) {
                            Ok(SyncAction::Present) => {
                                self.last_wait_frame = None;
                                let frame = self.current_frame.take().unwrap();
                                actions.push(PresenterAction::DispatchFrame(frame));
                            }
                            Ok(SyncAction::Wait(remaining)) => {
                                let f_idx = frame.frame_idx();
                                let count = match self.last_wait_frame {
                                    Some((idx, c)) if idx == f_idx => c + 1,
                                    _ => 1,
                                };
                                self.last_wait_frame = Some((f_idx, count));
                                actions.push(PresenterAction::ScheduleWakeup(remaining));
                                break;
                            }
                            Err(_) => {
                                self.last_wait_frame = None;
                                let frame = self.current_frame.take().unwrap();
                                actions.push(PresenterAction::DispatchFrame(frame));
                            }
                        }
                    }
                    PresenterState::PlayingUnpaced { .. } => {
                        let frame = self.current_frame.take().unwrap();
                        actions.push(PresenterAction::DispatchFrame(frame));
                    }
                    _ => {
                        break;
                    }
                }
            } else {
                break;
            }
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaCommand;
    use crate::media::types::YuvPlanes;
    use crate::media::sync::MasterClockPort;
    use std::sync::Arc;
    use std::sync::Mutex;

    struct MockClock {
        pts: Mutex<Option<f64>>,
    }

    impl MasterClockPort for MockClock {
        fn current_pts(&self) -> Option<f64> {
            *self.pts.lock().unwrap()
        }
        fn stall_for_catchup(&self) {}
        fn resume_from_catchup(&self) {}
    }

    fn create_dummy_frame(frame_idx: usize, sequence_id: usize, pts_sec: f64) -> DecodedFrame {
        DecodedFrame::Cpu {
            frame_idx,
            planes: Arc::new(YuvPlanes::default()),
            sequence_id,
            pts_sec,
        }
    }

    #[test]
    fn test_presenter_seek_outputs_pause() {
        let clock = Box::new(MockClock { pts: Mutex::new(Some(0.0)) });
        let sync_engine = VideoSyncEngine::new(clock);
        let mut sm = PresenterStateMachine::new(sync_engine);
        let actions = sm.step(PresenterEvent::Command(MediaCommand::Seek {
            frame_index: 10,
            keyframe_only: true,
            sequence_id: 1,
            audio_enabled: false,
        }));
        
        assert!(matches!(actions[0], PresenterAction::ClearLookaheadQueue));
    }

    #[test]
    fn test_presenter_outputs_wakeup_on_early_frame() {
        let clock = Box::new(MockClock { pts: Mutex::new(Some(1.0)) });
        let sync_engine = VideoSyncEngine::new(clock);
        let mut sm = PresenterStateMachine::new(sync_engine);
        
        sm.step(PresenterEvent::Command(MediaCommand::Play));
        
        // Send a frame that is way ahead of the clock (pts = 2.0 while clock is 1.0)
        let early_frame = create_dummy_frame(1, 0, 2.0);
        let actions = sm.step(PresenterEvent::FrameDecoded(early_frame));
        
        assert!(actions.iter().any(|a| matches!(a, PresenterAction::ScheduleWakeup(_))));
    }

    #[test]
    fn test_presenter_presents_due_frame() {
        let clock = Box::new(MockClock { pts: Mutex::new(Some(2.0)) });
        let sync_engine = VideoSyncEngine::new(clock);
        let mut sm = PresenterStateMachine::new(sync_engine);
        
        sm.step(PresenterEvent::Command(MediaCommand::Play));
        
        // Send a frame that is due (pts = 1.9 while clock is 2.0)
        let due_frame = create_dummy_frame(1, 0, 1.9);
        let actions = sm.step(PresenterEvent::FrameDecoded(due_frame));
        
        assert!(actions.iter().any(|a| matches!(a, PresenterAction::DispatchFrame(_))));
    }
}
