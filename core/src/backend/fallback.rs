//! Remote first, the device second, and only when the caller asked.

use super::{Backend, Work};
use crate::config::BackendKind;
use crate::error::{Error, Result};
use crate::transcript::Transcript;
use crate::utterance::Utterance;

pub struct FallbackBackend {
    primary: Box<dyn Backend>,
    local: Box<dyn Backend>,
}

impl FallbackBackend {
    pub fn new(primary: Box<dyn Backend>, local: Box<dyn Backend>) -> Self {
        Self { primary, local }
    }
}

impl Backend for FallbackBackend {
    fn transcribe(&self, utterance: &Utterance<'_>, work: &mut Work<'_, '_>) -> Result<Transcript> {
        match self.primary.transcribe(utterance, work) {
            Ok(transcript) => Ok(transcript),
            Err(why) if worth_retrying_here(&why) && work.nothing_delivered_yet() => {
                self.local.transcribe(utterance, work)
            }
            Err(why) => Err(why),
        }
    }

    fn kind(&self) -> BackendKind {
        self.primary.kind()
    }
}

/// The server being unreachable is worth another try here. Bad audio
/// and a cancellation are not; neither would go differently.
fn worth_retrying_here(why: &Error) -> bool {
    matches!(
        why,
        Error::Network { .. } | Error::ServerAtCapacity { .. } | Error::ServerError { .. }
    )
}
