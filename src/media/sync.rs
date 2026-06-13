use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum SyncAction {
    Present,
    Wait(Duration),
}

#[derive(Debug, Clone, Copy)]
pub enum SyncError {
    ClockUnavailable,
}

pub trait MasterClockPort: Send + Sync {
    /// Returns the current playback time in seconds.
    fn current_pts(&self) -> Option<f64>;
    
    /// Pauses audio playback to let video catch up.
    fn stall_for_catchup(&self);
    
    /// Resumes audio playback once video has caught up.
    fn resume_from_catchup(&self);
}

pub struct VideoSyncEngine {
    clock: Box<dyn MasterClockPort>,
    max_drift_sec: f64,
    is_stalled: bool,
}

impl VideoSyncEngine {
    pub fn new(clock: Box<dyn MasterClockPort>) -> Self {
        Self {
            clock,
            max_drift_sec: 0.15,
            is_stalled: false,
        }
    }

    pub fn reset(&mut self) {
        if self.is_stalled {
            self.clock.resume_from_catchup();
            self.is_stalled = false;
        }
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
