use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

struct ClockState {
    current_position_us: AtomicU64,
    has_audio: AtomicBool,
    is_playing: AtomicBool,
    audio_paused_for_sync: AtomicBool,
    fallback_start_us: AtomicU64,
    fallback_base_us: AtomicU64,
    playback_generation: AtomicU64,
    sample_rate: u32,
    fallback_epoch: Instant,
}

#[derive(Clone)]
pub struct MasterClock {
    state: Arc<ClockState>,
}

impl MasterClock {
    pub fn new(sample_rate: u32) -> Self {
        let state = Arc::new(ClockState {
            current_position_us: AtomicU64::new(0),
            has_audio: AtomicBool::new(false),
            is_playing: AtomicBool::new(false),
            audio_paused_for_sync: AtomicBool::new(false),
            fallback_start_us: AtomicU64::new(0),
            fallback_base_us: AtomicU64::new(0),
            playback_generation: AtomicU64::new(0),
            sample_rate,
            fallback_epoch: Instant::now(),
        });
        Self { state }
    }

    pub fn current_time(&self) -> f64 {
        let start_us = self.state.fallback_start_us.load(Ordering::Relaxed);
        let base_us = self.state.fallback_base_us.load(Ordering::Relaxed) as f64;
        if start_us > 0 {
            let elapsed_us = (self.state.fallback_epoch.elapsed().as_micros() as u64).saturating_sub(start_us);
            (base_us + elapsed_us as f64) / 1_000_000.0
        } else {
            base_us / 1_000_000.0
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.state.sample_rate
    }

    pub fn set_has_audio(&self, has_audio: bool) {
        self.state.has_audio.store(has_audio, Ordering::Relaxed);
    }

    pub fn has_audio(&self) -> bool {
        self.state.has_audio.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.state.is_playing.load(Ordering::Relaxed)
    }

    pub fn playback_generation(&self) -> u64 {
        self.state.playback_generation.load(Ordering::Relaxed)
    }

    pub fn inc_playback_generation(&self) {
        self.state.playback_generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn play(&self) {
        self.state.is_playing.store(true, Ordering::Relaxed);
        self.state.audio_paused_for_sync.store(false, Ordering::SeqCst);
        let start_us = self.state.fallback_start_us.load(Ordering::Relaxed);
        if start_us == 0 {
            let now_us = std::cmp::max(1, self.state.fallback_epoch.elapsed().as_micros() as u64);
            self.state.fallback_start_us.store(now_us, Ordering::Relaxed);
        }
    }

    pub fn pause(&self) {
        self.state.is_playing.store(false, Ordering::Relaxed);
        self.state.fallback_start_us.store(0, Ordering::Relaxed);
    }

    pub fn set_position(&self, time_seconds: f64) {
        let time_us = (time_seconds * 1_000_000.0) as u64;
        self.state.current_position_us.store(time_us, Ordering::Relaxed);
        self.state.fallback_base_us.store(time_us, Ordering::Relaxed);
        self.state.fallback_start_us.store(0, Ordering::Relaxed);
    }

    pub fn reset(&self, time_seconds: f64) {
        self.set_position(time_seconds);
        self.state.audio_paused_for_sync.store(true, Ordering::SeqCst);
        self.inc_playback_generation();
    }

    pub fn is_active(&self) -> bool {
        self.state.is_playing.load(Ordering::Relaxed)
            && !self.state.audio_paused_for_sync.load(Ordering::Relaxed)
    }

    pub fn advance(&self, written_samples: u64, last_tag: Option<(u64, u64)>, samples_read_total: u64) {
        if !self.is_active() || written_samples == 0 {
            return;
        }

        let current_pos_us = if let Some((tag_idx, tag_pts_us)) = last_tag {
            let elapsed_samples = samples_read_total.saturating_sub(tag_idx);
            let elapsed_us = (elapsed_samples * 1_000_000) / (self.state.sample_rate as u64);
            tag_pts_us + elapsed_us
        } else {
            let elapsed_us = (written_samples * 1_000_000) / (self.state.sample_rate as u64);
            self.state.current_position_us.load(Ordering::Relaxed) + elapsed_us
        };

        self.state.current_position_us.store(current_pos_us, Ordering::Relaxed);
        self.state.fallback_base_us.store(current_pos_us, Ordering::Relaxed);
        let now_us = std::cmp::max(1, self.state.fallback_epoch.elapsed().as_micros() as u64);
        self.state.fallback_start_us.store(now_us, Ordering::Relaxed);
    }
    pub fn current_pts(&self) -> Option<f64> {
        if !self.has_audio() {
            Some(self.current_time())
        } else {
            let current_us = self.state.current_position_us.load(Ordering::Relaxed);
            Some(current_us as f64 / 1_000_000.0)
        }
    }

    pub fn stall_for_catchup(&self) {
        self.state.audio_paused_for_sync.store(true, Ordering::SeqCst);
    }

    pub fn resume_from_catchup(&self) {
        self.state.audio_paused_for_sync.store(false, Ordering::SeqCst);
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_clock() {
        let clock = MasterClock::new(48000);
        clock.set_has_audio(false);
        clock.set_position(1.0);
        assert_eq!(clock.current_pts(), Some(1.0));

        clock.play();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let pts = clock.current_pts().unwrap();
        assert!(pts > 1.0, "pts is {}", pts);
        assert!(pts < 1.05);

        clock.pause();
        let pts2 = clock.current_pts().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert_eq!(clock.current_pts().unwrap(), pts2);
    }

    #[test]
    fn test_ticker_advance() {
        let clock = MasterClock::new(48000);
        let ticker = clock.clone();
        clock.set_has_audio(true);
        clock.play();
        
        ticker.advance(48000, None, 48000);
        
        assert_eq!(clock.current_pts(), Some(1.0));
        let time_diff = (clock.current_time() - 1.0).abs();
        assert!(time_diff < 0.01, "time_diff was {}", time_diff);
        
        ticker.advance(24000, None, 72000);
        assert_eq!(clock.current_pts(), Some(1.5));
    }

    #[test]
    fn test_play_clears_sync_pause() {
        let clock = MasterClock::new(48000);
        let ticker = clock.clone();
        clock.set_has_audio(true);
        clock.reset(0.0);
        assert!(!ticker.is_active(), "Ticker should be inactive after reset");
        clock.play();
        assert!(ticker.is_active(), "Ticker should be active after play");
    }
}
