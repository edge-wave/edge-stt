//! The trait both paths implement. Substituting one for the other is
//! what makes switching by configuration a property of the types.

use std::time::Duration;

use crate::cancel::CancelToken;
use crate::config::BackendKind;
use crate::error::Result;
use crate::transcript::{Partial, Transcript};
use crate::utterance::Utterance;

#[cfg(feature = "whisper")]
pub mod whisper;

/// Where a backend reports progress and checks whether to stop.
pub struct Work<'a> {
    pub on_partial: Option<&'a mut dyn FnMut(Partial)>,
    pub cancel: &'a CancelToken,
    pub timeout: Option<Duration>,
}

impl<'a> Work<'a> {
    pub fn new(cancel: &'a CancelToken) -> Self {
        Self { on_partial: None, cancel, timeout: None }
    }

    pub fn wants_partials(&self) -> bool {
        self.on_partial.is_some()
    }

    pub fn emit(&mut self, partial: Partial) {
        if let Some(sink) = self.on_partial.as_mut() {
            sink(partial);
        }
    }
}

/// Shared rather than owned, so the server can serve several clients
/// from one loaded model.
pub trait Backend: Send + Sync {
    fn transcribe(&self, utterance: &Utterance<'_>, work: &mut Work<'_>) -> Result<Transcript>;

    fn kind(&self) -> BackendKind;
}
