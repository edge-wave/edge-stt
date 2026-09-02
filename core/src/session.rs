//! A caller-fed, open-ended audio stream. Where `transcribe`/
//! `transcribe_with` take one complete recording, this takes samples
//! as they arrive and decides for itself where one utterance ends --
//! then runs the exact same decode path either way.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::EdgeStt;
use crate::cancel::CancelToken;
use crate::endpoint::Endpointer;
use crate::error::Result;
use crate::transcript::{Partial, Transcript};
use crate::utterance::Utterance;

// `Buffering` is only ever constructed by `AudioSession::new`, which
// is itself gated -- expected, not a bug, without `streaming`.
#[cfg_attr(not(feature = "streaming"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Buffering,
    Closed,
}

pub struct AudioSession<'a> {
    stt: &'a EdgeStt,
    endpointer: Box<dyn Endpointer + 'a>,
    max_duration: Duration,
    buffered: Duration,
    state: State,
    open_flag: &'a AtomicBool,
}

impl<'a> AudioSession<'a> {
    /// Only ever called from `EdgeStt::open_session`'s `streaming`
    /// branch -- without that feature nothing can produce an
    /// `Endpointer` to hand it, so this stays behind the same gate.
    #[cfg(feature = "streaming")]
    pub(crate) fn new(
        stt: &'a EdgeStt,
        endpointer: Box<dyn Endpointer + 'a>,
        max_duration: Duration,
        open_flag: &'a AtomicBool,
    ) -> Self {
        Self {
            stt,
            endpointer,
            max_duration,
            buffered: Duration::ZERO,
            state: State::Buffering,
            open_flag,
        }
    }

    /// Feeds one piece of newly-captured audio. Delivers a `Transcript`
    /// the moment the endpointer -- or the same maximum-duration
    /// ceiling a pre-bounded utterance is already held to -- considers
    /// one utterance finished; otherwise `None`, with the audio staying
    /// buffered for the next call.
    pub fn push(
        &mut self,
        samples: &[i16],
        mut on_partial: impl FnMut(Partial),
    ) -> Result<Option<Transcript>> {
        if self.state == State::Closed {
            return Ok(None);
        }

        self.buffered += sample_duration(samples.len());
        if let Some(finished) = self.endpointer.push(samples)? {
            return self.decode(finished, &mut on_partial).map(Some);
        }

        if self.buffered >= self.max_duration
            && let Some(finished) = self.endpointer.take_remainder()
        {
            return self.decode(finished, &mut on_partial).map(Some);
        }

        Ok(None)
    }

    /// Finalizes and delivers whatever utterance was in progress
    /// (FR-013), then closes the session. A second call is a no-op
    /// returning `Ok(None)`, not an error.
    pub fn close(&mut self) -> Result<Option<Transcript>> {
        if self.state == State::Closed {
            return Ok(None);
        }
        self.state = State::Closed;
        self.open_flag.store(false, Ordering::Release);

        match self.endpointer.take_remainder() {
            Some(finished) => self.decode(finished, &mut |_partial: Partial| {}).map(Some),
            None => Ok(None),
        }
    }

    fn decode(
        &mut self,
        samples: Vec<i16>,
        on_partial: &mut dyn FnMut(Partial),
    ) -> Result<Transcript> {
        self.buffered = Duration::ZERO;
        let utterance = Utterance::mono_16k(&samples);
        let cancel = CancelToken::new();
        self.stt.run(&utterance, Some(on_partial), &cancel)
    }
}

impl Drop for AudioSession<'_> {
    /// Closing without calling `close()` first still frees the slot
    /// FR-015 holds open, but discards anything still in progress --
    /// the same as a dropped connection on the remote path. Does
    /// nothing if `close()` already ran: by then a *different* session
    /// may have claimed the slot, and this one must not clear it.
    fn drop(&mut self) {
        if self.state != State::Closed {
            self.open_flag.store(false, Ordering::Release);
        }
    }
}

fn sample_duration(count: usize) -> Duration {
    Duration::from_secs_f64(count as f64 / 16_000.0)
}

/// One `AtomicBool` per `EdgeStt`, so `open_session` can enforce
/// FR-015 without needing `&mut self` -- which would break the
/// existing shared-`&self` usage the server relies on. Only meaningful
/// alongside `streaming`, which is the only feature that ever claims it.
#[cfg(feature = "streaming")]
pub(crate) struct SessionSlot(AtomicBool);

#[cfg(feature = "streaming")]
impl SessionSlot {
    pub(crate) fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Claims the slot, or fails if a session is already open.
    pub(crate) fn claim(&self) -> Result<&AtomicBool> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| &self.0)
            .map_err(|_| crate::error::Error::InvalidValue {
                setting: "session",
                expected: "no other continuous session open on this transcriber".to_string(),
                got: "one is already open".to_string(),
            })
    }
}
