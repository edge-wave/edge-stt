//! Deciding, on its own, where one spoken utterance ends inside audio
//! that arrives with no predetermined end.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};

/// edge-ear's own default is the same order of magnitude; there is no
/// reason to invent a different number for the same kind of pause.
const DEFAULT_PAUSE_TOLERANCE: Duration = Duration::from_secs(3);

/// The integrator-adjustable settings governing when a continuous
/// session decides an utterance has ended. The forced-close ceiling
/// for a runaway utterance is deliberately not a field here -- it
/// reuses `Config::max_duration`, the same limit a pre-bounded
/// utterance is already held to, rather than a second one.
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    pub vad_model: PathBuf,
    pub pause_tolerance: Duration,
}

impl EndpointConfig {
    pub fn new(vad_model: impl AsRef<Path>) -> Self {
        Self {
            vad_model: vad_model.as_ref().to_path_buf(),
            pause_tolerance: DEFAULT_PAUSE_TOLERANCE,
        }
    }

    pub fn with_pause_tolerance(mut self, pause_tolerance: Duration) -> Self {
        self.pause_tolerance = pause_tolerance;
        self
    }

    pub fn check(&self) -> Result<()> {
        if self.pause_tolerance.is_zero() {
            return Err(Error::InvalidValue {
                setting: "pause_tolerance",
                expected: "greater than zero".to_string(),
                got: "zero".to_string(),
            });
        }
        Ok(())
    }
}

/// What decides where one utterance ends inside a stream of pushed
/// audio. `whisper_vad::WhisperVad` is the only implementation today;
/// the trait exists so `AudioSession` does not depend on it directly.
pub trait Endpointer: Send {
    /// Returns the finished utterance's samples once enough trailing
    /// silence has been seen, never later than that.
    fn push(&mut self, samples: &[i16]) -> Result<Option<Vec<i16>>>;

    /// Whatever is buffered, taken for a clean close. Leaves nothing
    /// buffered -- not ready to be reused for a second utterance.
    fn take_remainder(&mut self) -> Option<Vec<i16>>;
}

#[cfg(feature = "streaming")]
pub mod whisper_vad;
