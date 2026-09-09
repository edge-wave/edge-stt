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

/// What a recognizer says about itself, so a session knows what it has
/// to supply and a caller knows what it will receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// It can produce results before an utterance has finished.
    pub live_interims: bool,
    /// A result it has already delivered may later be corrected.
    pub revises: bool,
    /// It decides where an utterance ends without a separate detector.
    pub self_endpointing: bool,
}

/// One session's own recognition state. Never shared between sessions,
/// which is what lets several run against one loaded recognizer.
pub trait LiveDecoder: Send {
    /// Given everything heard in this utterance so far -- not only the
    /// newest audio -- the words for it, or `None` if there are none
    /// worth delivering yet.
    fn push(&mut self, utterance_so_far: &[i16]) -> Result<Option<String>>;

    /// Forgets the utterance that just ended and starts the next clean.
    fn reset(&mut self);

    /// How much of the audio pushed so far belongs to a finished
    /// utterance, for a recognizer that decides that itself. `None`
    /// from one that leaves the question to a boundary detector, which
    /// is every recognizer shipped today.
    fn boundary(&self) -> Option<usize> {
        None
    }
}

/// Shared rather than owned, so the server can serve several clients
/// from one loaded model.
pub trait Backend: Send + Sync {
    fn transcribe(&self, utterance: &Utterance<'_>, work: &mut Work<'_, '_>) -> Result<Transcript>;

    fn kind(&self) -> BackendKind;

    /// Answering no to everything keeps a backend that never heard of
    /// this behaving exactly as it does today.
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }

    /// Only ever called after `capabilities` said it could; the default
    /// exists so a backend that cannot does not have to say so twice.
    fn open_live(&self) -> Result<Box<dyn LiveDecoder>> {
        Err(crate::error::Error::InvalidValue {
            setting: "live_interims",
            expected: "a recognizer that can produce results while speech continues".to_string(),
            got: format!("{}, which cannot", self.kind()),
        })
    }
}
