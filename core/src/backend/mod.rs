//! The trait both paths implement. Substituting one for the other is
//! what makes switching by configuration a property of the types.

use std::time::Duration;

use crate::cancel::CancelToken;
use crate::config::BackendKind;
use crate::error::Result;
use crate::transcript::{Partial, Transcript};
use crate::utterance::Utterance;

pub mod fallback;
#[cfg(feature = "remote")]
pub mod remote;
#[cfg(feature = "whisper")]
pub mod whisper;

/// Where a backend reports progress and checks whether to stop.
/// Two lifetimes because the callback and the cancel token routinely
/// come from unrelated borrows (a session's own vs. a fresh one).
pub struct Work<'p, 'c> {
    pub on_partial: Option<&'p mut dyn FnMut(Partial)>,
    pub cancel: &'c CancelToken,
    pub timeout: Option<Duration>,
    emitted: u32,
}

impl<'p, 'c> Work<'p, 'c> {
    pub fn new(cancel: &'c CancelToken) -> Self {
        Self {
            on_partial: None,
            cancel,
            timeout: None,
            emitted: 0,
        }
    }

    pub fn wants_partials(&self) -> bool {
        self.on_partial.is_some()
    }

    pub fn emit(&mut self, partial: Partial) {
        if let Some(sink) = self.on_partial.as_mut() {
            self.emitted += 1;
            sink(partial);
        }
    }

    /// Falling back after the caller has already seen words would
    /// restart the sequence and contradict what they were shown.
    pub fn nothing_delivered_yet(&self) -> bool {
        self.emitted == 0
    }
}

/// Shared rather than owned, so the server can serve several clients
/// from one loaded model.
pub trait Backend: Send + Sync {
    fn transcribe(&self, utterance: &Utterance<'_>, work: &mut Work<'_, '_>) -> Result<Transcript>;

    fn kind(&self) -> BackendKind;
}
