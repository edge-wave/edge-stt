//! Deciding when a result produced while someone is still speaking is
//! worth delivering. Both conditions live here, so every surface
//! inherits them rather than reimplementing them.

use std::time::{Duration, Instant};

/// The two conditions an interim result must pass: it must say
/// something the caller has not already been told, and it must not
/// arrive sooner than the configured interval allows.
#[derive(Debug)]
pub struct InterimGate {
    last_text: Option<String>,
    last_at: Option<Instant>,
    min_interval: Duration,
}

impl InterimGate {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            last_text: None,
            last_at: None,
            min_interval,
        }
    }

    /// Whether enough time has passed to run a pass at all. Asked
    /// before one runs, so a pass that would be dropped is never paid
    /// for -- on a device that saving is most of the point.
    pub fn due(&self) -> bool {
        match self.last_at {
            None => true,
            Some(at) => at.elapsed() >= self.min_interval,
        }
    }

    /// Takes a produced result and says whether to deliver it, recording
    /// it when the answer is yes. Identical text is dropped silently;
    /// it is not an error, just nothing new to say.
    pub fn admit(&mut self, text: &str) -> bool {
        if !self.due() {
            return false;
        }
        if self.last_text.as_deref() == Some(text) {
            return false;
        }
        self.last_text = Some(text.to_string());
        self.last_at = Some(Instant::now());
        true
    }

    /// Called when an utterance ends, so the first interim of the next
    /// one is never withheld for resembling the last one of this.
    pub fn reset(&mut self) {
        self.last_text = None;
        self.last_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_is_not_delivered_twice() {
        let mut gate = InterimGate::new(Duration::ZERO);
        assert!(gate.admit("and so my fellow"));
        assert!(!gate.admit("and so my fellow"));
        assert!(gate.admit("and so my fellow Americans"));
    }

    #[test]
    fn nothing_is_delivered_before_the_interval_passes() {
        let mut gate = InterimGate::new(Duration::from_secs(60));
        assert!(gate.admit("first"));
        assert!(!gate.due());
        assert!(!gate.admit("second"));
    }

    #[test]
    fn a_finished_utterance_clears_what_came_before() {
        let mut gate = InterimGate::new(Duration::ZERO);
        assert!(gate.admit("same words"));
        gate.reset();
        assert!(gate.admit("same words"));
    }
}
