//! Deciding when a result produced while someone is still speaking is
//! worth delivering. Both conditions live here, so every surface
//! inherits them rather than reimplementing them.

use std::time::{Duration, Instant};

#[cfg(feature = "whisper")]
pub mod whisper_window;

/// The two conditions an interim result must pass: it must say
/// something the caller has not already been told, and it must not
/// arrive sooner than the configured interval allows.
#[derive(Debug)]
pub struct InterimGate {
    last_text: Option<String>,
    last_at: Option<Instant>,
    /// How much audio had been heard when the last pass ran, so that
    /// passes are bounded by the speech rather than by the machine.
    heard_at_last: usize,
    min_interval: Duration,
    fresh_samples: usize,
}

impl InterimGate {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            last_text: None,
            last_at: None,
            heard_at_last: 0,
            min_interval,
            fresh_samples: (min_interval.as_secs_f64() * 16_000.0) as usize,
        }
    }

    /// Whether a pass is worth running over `heard` samples of speech.
    /// Asked before one runs, so a pass that would be dropped is never
    /// paid for -- on a device that saving is most of the point.
    ///
    /// Time alone is not enough. A machine that cannot keep up has
    /// always had the interval elapse by the time the next chunk lands,
    /// so it would run a pass per chunk and fall further behind with
    /// each one. Requiring the interval's worth of new audio as well
    /// bounds the work by what was said.
    pub fn due(&self, heard: usize) -> bool {
        // The first pass has no earlier one to be measured against, and
        // when it may start is the recogniser's own business.
        let Some(at) = self.last_at else {
            return true;
        };
        if at.elapsed() < self.min_interval {
            return false;
        }
        heard.saturating_sub(self.heard_at_last) >= self.fresh_samples
    }

    /// Takes a produced result and says whether to deliver it, recording
    /// it when the answer is yes. Identical text is dropped silently;
    /// it is not an error, just nothing new to say.
    pub fn admit(&mut self, heard: usize, text: &str) -> bool {
        if !self.due(heard) {
            return false;
        }
        // Recorded whether or not the words are new: the pass was run,
        // and running it again immediately would find the same thing.
        self.last_at = Some(Instant::now());
        self.heard_at_last = heard;
        if self.last_text.as_deref() == Some(text) {
            return false;
        }
        self.last_text = Some(text.to_string());
        true
    }

    /// Called when an utterance ends, so the first interim of the next
    /// one is never withheld for resembling the last one of this.
    pub fn reset(&mut self) {
        self.last_text = None;
        self.last_at = None;
        self.heard_at_last = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of audio, in samples, so a test can say "more audio
    /// arrived" without spelling out the rate every time.
    const SECOND: usize = 16_000;

    #[test]
    fn identical_text_is_not_delivered_twice() {
        let mut gate = InterimGate::new(Duration::ZERO);
        assert!(gate.admit(SECOND, "and so my fellow"));
        assert!(!gate.admit(2 * SECOND, "and so my fellow"));
        assert!(gate.admit(3 * SECOND, "and so my fellow Americans"));
    }

    #[test]
    fn nothing_is_delivered_before_the_interval_passes() {
        let mut gate = InterimGate::new(Duration::from_secs(60));
        assert!(gate.admit(60 * SECOND, "first"));
        assert!(!gate.due(120 * SECOND));
        assert!(!gate.admit(120 * SECOND, "second"));
    }

    /// A machine that cannot keep up always finds the interval long
    /// past, so the audio is what has to bound the work.
    #[test]
    fn a_backlog_does_not_buy_extra_passes() {
        let mut gate = InterimGate::new(Duration::from_millis(1));
        assert!(gate.admit(SECOND, "first"));
        std::thread::sleep(Duration::from_millis(5));

        assert!(!gate.due(SECOND), "a pass over the same audio is waste");
        assert!(
            !gate.due(SECOND + 8),
            "half an interval of speech is not one"
        );
        assert!(gate.due(SECOND + 16));
    }

    #[test]
    fn the_first_pass_is_not_held_back_by_the_audio_floor() {
        let gate = InterimGate::new(Duration::from_secs(60));
        assert!(gate.due(1_600), "nothing has run yet to measure against");
    }

    #[test]
    fn a_finished_utterance_clears_what_came_before() {
        let mut gate = InterimGate::new(Duration::ZERO);
        assert!(gate.admit(SECOND, "same words"));
        gate.reset();
        assert!(gate.admit(SECOND, "same words"));
    }
}
