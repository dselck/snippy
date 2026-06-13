use crate::media::{MediaCommand, DecodedFrame};
use std::time::Duration;

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
    // State goes here
}

impl PresenterStateMachine {
    pub fn new() -> Self {
        Self {}
    }

    pub fn step(&mut self, event: PresenterEvent) -> Vec<PresenterAction> {
        match event {
            PresenterEvent::Command(MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled }) => {
                vec![
                    PresenterAction::ClearLookaheadQueue,
                    PresenterAction::SendDecoderCommand(MediaCommand::Seek { frame_index, keyframe_only, sequence_id, audio_enabled })
                ]
            }
            _ => vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaCommand;

    #[test]
    fn test_presenter_seek_outputs_pause() {
        let mut sm = PresenterStateMachine::new();
        let actions = sm.step(PresenterEvent::Command(MediaCommand::Seek {
            frame_index: 10,
            keyframe_only: true,
            sequence_id: 1,
            audio_enabled: false,
        }));
        
        assert!(matches!(actions[0], PresenterAction::ClearLookaheadQueue));
    }
}
