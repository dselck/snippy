use std::time::Duration;
use crate::audio::clock::MasterClock;

#[derive(Debug, Clone, Copy)]
pub enum SyncAction {
    Present,
    Wait(Duration),
}

#[derive(Debug, Clone, Copy)]
pub enum SyncError {
    ClockUnavailable,
}

pub struct VideoSyncEngine {
    clock: MasterClock,
    max_drift_sec: f64,
    is_stalled: bool,
}

impl VideoSyncEngine {
    pub fn new(clock: MasterClock) -> Self {
        Self {
            clock,
            max_drift_sec: 0.15,
            is_stalled: false,
        }
    }

    pub fn reset(&mut self) {
        self.clock.resume_from_catchup();
        self.is_stalled = false;
    }

    pub fn evaluate_frame(&mut self, frame_pts_sec: f64) -> Result<SyncAction, SyncError> {
        let clock_pts = match self.clock.current_pts() {
            Some(pts) => pts,
            None => {
                self.clock.resume_from_catchup();
                self.is_stalled = false;
                return Ok(SyncAction::Present);
            }
        };

        if frame_pts_sec > clock_pts {
            // Frame is early, we need to wait
            self.clock.resume_from_catchup();
            self.is_stalled = false;
            let wait_sec = frame_pts_sec - clock_pts;
            Ok(SyncAction::Wait(Duration::from_secs_f64(wait_sec)))
        } else {
            // Frame is late
            let drift = clock_pts - frame_pts_sec;
            if drift > self.max_drift_sec {
                // Video is way behind, stall audio!
                if !self.is_stalled {
                    self.clock.stall_for_catchup();
                    self.is_stalled = true;
                }
            } else {
                // Video has caught up or is close to clock!
                self.clock.resume_from_catchup();
                self.is_stalled = false;
            }
            Ok(SyncAction::Present)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_engine_reset_unstalls_clock() {
        let clock = MasterClock::new(48000);
        clock.set_has_audio(true);
        clock.set_position(1.0);
        clock.play();
        
        let mut sync_engine = VideoSyncEngine::new(clock.clone());

        // 1. Force a stall by presenting a late frame (frame is at 0.5s, clock is at 1.0s)
        let _action = sync_engine.evaluate_frame(0.5).unwrap();
        assert!(!clock.is_active(), "Clock should be stalled");

        // 2. Reset the sync engine (e.g. after a seek)
        sync_engine.reset();
        assert!(clock.is_active(), "Clock should be unstalled after reset");
    }

    #[test]
    fn test_sync_engine_reset_always_unstalls_even_if_not_stalled_internally() {
        let clock = MasterClock::new(48000);
        clock.set_has_audio(true);
        clock.set_position(1.0);
        clock.play();
        clock.stall_for_catchup();
        assert!(!clock.is_active());

        let mut sync_engine = VideoSyncEngine::new(clock.clone());

        // Sync engine internal state has is_stalled = false
        assert!(!sync_engine.is_stalled);

        // Reset should still resume the clock from catchup
        sync_engine.reset();
        assert!(clock.is_active(), "Clock should be unstalled by reset even if sync engine wasn't stalled");
    }
}

