//! One complete recording handed over for transcription. edge-stt
//! never captures audio; an utterance always arrives from outside.

use std::time::{Duration, SystemTime};

use crate::config::AudioFormat;
use crate::error::{Error, Result};

/// Anything shorter than this is treated as silence rather than
/// rejected, because a false wake word should not be an error.
const SILENCE_FLOOR: Duration = Duration::from_millis(300);

#[derive(Debug, Clone)]
pub struct Utterance<'a> {
    pub samples: &'a [i16],
    pub format: AudioFormat,
    pub captured_at: Option<SystemTime>,
}

impl<'a> Utterance<'a> {
    pub fn new(samples: &'a [i16], format: AudioFormat) -> Self {
        Self {
            samples,
            format,
            captured_at: None,
        }
    }

    /// What edge-ear hands over at end of speech.
    pub fn mono_16k(samples: &'a [i16]) -> Self {
        Self::new(samples, AudioFormat::mono_16k())
    }

    pub fn captured_at(mut self, when: SystemTime) -> Self {
        self.captured_at = Some(when);
        self
    }

    pub fn duration(&self) -> Duration {
        let rate = self.format.sample_rate.max(1) as u64;
        let channels = self.format.channels.max(1) as u64;
        let frames = self.samples.len() as u64 / channels;
        Duration::from_secs_f64(frames as f64 / rate as f64)
    }

    /// Too short is silence, too long is refused up front. Neither is
    /// ever quietly truncated.
    pub fn check(&self, max_duration: Duration) -> Result<()> {
        self.format.check_transcribable()?;
        let duration = self.duration();
        if duration > max_duration {
            return Err(Error::AudioTooLong {
                limit: max_duration,
                got: duration,
            });
        }
        Ok(())
    }

    pub fn is_below_silence_floor(&self) -> bool {
        self.duration() < SILENCE_FLOOR
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.samples.len() * 2);
        for sample in self.samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }
}
