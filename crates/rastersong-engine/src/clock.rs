//! The preview playback clock: plays as fast as rendering allows, up to real time.

/// Preview playback position, with speed driven by how much is rendered ahead:
/// `speed = min(buffered_seconds, 1)`. Light graphs play in real time, heavy ones slow down to a
/// rate rendering can sustain, and right after a seek or edit playback waits for frames and
/// speeds up as they arrive. Playback never moves onto a frame that hasn't been rendered.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackClock {
    /// Position in frames; the displayed frame is its whole part.
    position: f64,
    playing: bool,
    speed: f64,
    frame_rate: f64,
    frame_count: usize,
}

impl PlaybackClock {
    pub fn new(frame_rate: f64, frame_count: usize) -> Self {
        Self {
            position: 0.0,
            playing: false,
            speed: 0.0,
            frame_rate,
            frame_count,
        }
    }

    /// The frame to display.
    pub fn frame(&self) -> usize {
        self.position as usize
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Current playback speed, `0..=1` of real time.
    pub fn speed(&self) -> f64 {
        if self.playing { self.speed } else { 0.0 }
    }

    pub fn play(&mut self) {
        if self.frame() + 1 >= self.frame_count {
            self.position = 0.0;
        }
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn toggle(&mut self) {
        if self.playing {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn seek(&mut self, frame: usize) {
        self.position = frame.min(self.frame_count.saturating_sub(1)) as f64;
    }

    /// Advances by `dt` seconds of wall time, given how many consecutive frames starting at the
    /// current one are rendered. Stops at the last frame.
    pub fn advance(&mut self, dt: f64, buffered_frames: usize) {
        let buffered_seconds = buffered_frames as f64 / self.frame_rate;
        self.speed = buffered_seconds.min(1.0);
        if !self.playing {
            return;
        }
        let start = self.frame();
        let last = self.frame_count.saturating_sub(1) as f64;
        // Never pass the end of what's rendered, so the displayed frame is always available.
        let furthest = (start + buffered_frames) as f64 - 1e-9;
        let target = self.position + dt * self.frame_rate * self.speed;
        self.position = target.min(furthest.max(self.position)).min(last);
        if self.position >= last {
            self.playing = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plays_in_real_time_with_a_full_buffer() {
        let mut clock = PlaybackClock::new(30.0, 300);
        clock.play();
        clock.advance(0.5, 60);
        assert_eq!(clock.speed(), 1.0);
        assert_eq!(clock.frame(), 15);
    }

    #[test]
    fn slows_down_with_a_short_buffer() {
        let mut clock = PlaybackClock::new(30.0, 300);
        clock.play();
        // Half a second buffered: half speed.
        clock.advance(0.2, 15);
        assert_eq!(clock.speed(), 0.5);
        assert_eq!(clock.frame(), 3);
    }

    #[test]
    fn waits_when_nothing_is_rendered() {
        let mut clock = PlaybackClock::new(30.0, 300);
        clock.seek(100);
        clock.play();
        clock.advance(1.0, 0);
        assert_eq!((clock.frame(), clock.speed()), (100, 0.0));
    }

    #[test]
    fn never_moves_past_rendered_frames() {
        let mut clock = PlaybackClock::new(30.0, 300);
        clock.play();
        // A long hitch: 10 seconds pass but only 3 frames are ready.
        clock.advance(10.0, 3);
        assert_eq!(clock.frame(), 2);
    }

    #[test]
    fn stops_at_the_end_and_restarts_from_the_beginning() {
        let mut clock = PlaybackClock::new(30.0, 10);
        clock.play();
        clock.advance(5.0, 100);
        assert_eq!(clock.frame(), 9);
        assert!(!clock.is_playing());
        clock.play();
        assert_eq!(clock.frame(), 0);
    }

    #[test]
    fn paused_clock_does_not_move() {
        let mut clock = PlaybackClock::new(30.0, 300);
        clock.advance(1.0, 300);
        assert_eq!((clock.frame(), clock.speed()), (0, 0.0));
    }
}
