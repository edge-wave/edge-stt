//! A caller-fed, open-ended audio stream. Where `transcribe`/
//! `transcribe_with` take one complete recording, this takes samples
//! as they arrive and decides for itself where one utterance ends --
//! then runs the exact same decode path either way.

use std::time::Duration;

use crate::EdgeStt;
use crate::backend::LiveDecoder;
use crate::cancel::CancelToken;
use crate::config::SessionConfig;
use crate::endpoint::Endpointer;
use crate::error::Result;
use crate::live::InterimGate;
use crate::transcript::{Partial, PartialKind, Transcript};
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
    /// Absent only for a recognizer that finds its own boundaries.
    endpointer: Option<Box<dyn Endpointer + 'a>>,
    /// Present only when the caller asked for words while speech
    /// continues, so a caller who did not pays nothing for this.
    live: Option<Box<dyn LiveDecoder>>,
    gate: InterimGate,
    /// Everything heard in the utterance in progress. Kept only when a
    /// live decoder needs it, since it needs the whole utterance.
    utterance: Vec<i16>,
    seq: u32,
    max_duration: Duration,
    buffered: Duration,
    state: State,
}

impl<'a> AudioSession<'a> {
    /// Only ever called from `EdgeStt::open_session`'s `streaming`
    /// branch, which is where the configuration is checked against
    /// what the recognizer says it can do.
    #[cfg(feature = "streaming")]
    pub(crate) fn new(
        stt: &'a EdgeStt,
        endpointer: Option<Box<dyn Endpointer + 'a>>,
        live: Option<Box<dyn LiveDecoder>>,
        config: &SessionConfig,
        max_duration: Duration,
    ) -> Self {
        Self {
            stt,
            endpointer,
            live,
            gate: InterimGate::new(config.interim_min_interval),
            utterance: Vec::new(),
            seq: 0,
            max_duration,
            buffered: Duration::ZERO,
            state: State::Buffering,
        }
    }

    /// Feeds one piece of newly-captured audio. Delivers a `Transcript`
    /// the moment the endpointer -- or the same maximum-duration
    /// ceiling a pre-bounded utterance is already held to -- considers
    /// one utterance finished; otherwise `None`, with the audio staying
    /// buffered for the next call. `on_partial` is `None` exactly like
    /// `transcribe` (as opposed to `transcribe_with`): a caller that
    /// asks for nothing registers no callback with the decoder and
    /// pays no cost for it.
    pub fn push(
        &mut self,
        samples: &[i16],
        mut on_partial: Option<&mut dyn FnMut(Partial)>,
    ) -> Result<Option<Transcript>> {
        if self.state == State::Closed {
            return Ok(None);
        }

        self.buffered += sample_duration(samples.len());
        if self.live.is_some() {
            self.utterance.extend_from_slice(samples);
        }

        if let Some(partial) = self.recognise_so_far()?
            && let Some(sink) = on_partial.as_deref_mut()
        {
            sink(partial);
        }

        if let Some(endpointer) = self.endpointer.as_mut()
            && let Some(finished) = endpointer.push(samples)?
        {
            return self.finish(finished, on_partial).map(Some);
        }

        if self.buffered >= self.max_duration
            && let Some(finished) = self.endpointer.as_mut().and_then(|e| e.take_remainder())
        {
            return self.finish(finished, on_partial).map(Some);
        }

        Ok(None)
    }

    /// Finalizes and delivers whatever utterance was in progress, then
    /// closes the session. A second call is a no-op returning `Ok(None)`,
    /// not an error. `on_partial` behaves exactly as it does for `push`:
    /// the finalized utterance decodes like any other, and can still
    /// have interim results on the way to its `Transcript`.
    pub fn close(
        &mut self,
        on_partial: Option<&mut dyn FnMut(Partial)>,
    ) -> Result<Option<Transcript>> {
        if self.state == State::Closed {
            return Ok(None);
        }
        self.state = State::Closed;

        match self.endpointer.as_mut().and_then(|e| e.take_remainder()) {
            Some(finished) => self.finish(finished, on_partial).map(Some),
            None => Ok(None),
        }
    }

    /// Runs one pass over the utterance so far, when the caller asked
    /// for that and the gate says a pass is due. A pass that would be
    /// dropped is never paid for, which is most of the point on a
    /// device.
    fn recognise_so_far(&mut self) -> Result<Option<Partial>> {
        // Destructured so the decoder and the audio it reads are borrowed
        // from different fields rather than from the whole session.
        let Self {
            live,
            utterance,
            gate,
            seq,
            ..
        } = self;

        let Some(decoder) = live.as_mut() else {
            return Ok(None);
        };
        if !gate.due() {
            return Ok(None);
        }
        let Some(text) = decoder.push(utterance)? else {
            return Ok(None);
        };
        if !gate.admit(&text) {
            return Ok(None);
        }

        // A pass re-recognises everything, so it can differ anywhere.
        let partial = Partial {
            seq: *seq,
            kind: PartialKind::Replace,
            text,
            segment: None,
        };
        *seq += 1;
        Ok(Some(partial))
    }

    /// Decodes a finished utterance and clears everything the next one
    /// must not inherit.
    fn finish(
        &mut self,
        samples: Vec<i16>,
        on_partial: Option<&mut dyn FnMut(Partial)>,
    ) -> Result<Transcript> {
        self.buffered = Duration::ZERO;
        let taken = samples.len().min(self.utterance.len());
        self.utterance.drain(..taken);
        self.gate.reset();
        self.seq = 0;
        if let Some(decoder) = self.live.as_mut() {
            decoder.reset();
        }

        let utterance = Utterance::mono_16k(&samples);
        let cancel = CancelToken::new();
        self.stt.run(&utterance, on_partial, &cancel)
    }
}

fn sample_duration(count: usize) -> Duration {
    Duration::from_secs_f64(count as f64 / 16_000.0)
}

#[cfg(all(test, feature = "streaming"))]
mod tests {
    use super::*;
    use crate::backend::{Backend, Capabilities, Work};
    use crate::config::{BackendKind, Language};

    /// Says whatever it was told to say, one line per pass, so a test
    /// can assert on delivery rather than on recognition.
    struct Scripted {
        lines: Vec<String>,
        at: usize,
    }

    impl LiveDecoder for Scripted {
        fn push(&mut self, _utterance_so_far: &[i16]) -> Result<Option<String>> {
            let line = self.lines.get(self.at).cloned();
            if line.is_some() {
                self.at += 1;
            }
            Ok(line)
        }

        fn reset(&mut self) {
            self.at = 0;
        }
    }

    struct Fake {
        can: Capabilities,
        lines: Vec<String>,
    }

    impl Backend for Fake {
        fn transcribe(&self, _: &Utterance<'_>, _: &mut Work<'_, '_>) -> Result<Transcript> {
            Ok(Transcript::empty(
                Language::new("en"),
                Duration::ZERO,
                Duration::ZERO,
                BackendKind::Local,
            ))
        }

        fn kind(&self) -> BackendKind {
            BackendKind::Local
        }

        fn capabilities(&self) -> Capabilities {
            self.can
        }

        fn open_live(&self) -> Result<Box<dyn LiveDecoder>> {
            Ok(Box::new(Scripted {
                lines: self.lines.clone(),
                at: 0,
            }))
        }
    }

    fn able(lines: &[&str]) -> EdgeStt {
        EdgeStt::with_backend(Box::new(Fake {
            can: Capabilities {
                live_interims: true,
                revises: true,
                self_endpointing: true,
            },
            lines: lines.iter().map(|line| line.to_string()).collect(),
        }))
    }

    fn unable() -> EdgeStt {
        EdgeStt::with_backend(Box::new(Fake {
            can: Capabilities::default(),
            lines: Vec::new(),
        }))
    }

    fn quiet(count: usize) -> Vec<i16> {
        vec![0; count]
    }

    /// An interval short enough that the gate never withholds anything,
    /// so a test can look at one condition at a time.
    fn brisk() -> SessionConfig {
        SessionConfig::new()
            .with_live_interims()
            .with_interim_min_interval(Duration::from_nanos(1))
    }

    #[test]
    fn a_callback_alone_does_not_ask_for_words_while_speaking() {
        let stt = able(&["and so", "and so my fellow"]);
        let mut session = stt.open_session(SessionConfig::new()).unwrap();

        let mut seen: Vec<String> = Vec::new();
        {
            let mut sink = |partial: Partial| seen.push(partial.text);
            session.push(&quiet(1600), Some(&mut sink)).unwrap();
            session.push(&quiet(1600), Some(&mut sink)).unwrap();
        }

        assert!(
            seen.is_empty(),
            "a session that never asked received {seen:?}"
        );
    }

    #[test]
    fn asking_for_words_while_speaking_delivers_them() {
        let stt = able(&["and so", "and so my fellow"]);
        let mut session = stt.open_session(brisk()).unwrap();

        let mut seen: Vec<String> = Vec::new();
        {
            let mut sink = |partial: Partial| {
                assert_eq!(partial.kind, PartialKind::Replace);
                seen.push(partial.text);
            };
            session.push(&quiet(1600), Some(&mut sink)).unwrap();
            session.push(&quiet(1600), Some(&mut sink)).unwrap();
        }

        assert_eq!(seen, vec!["and so", "and so my fellow"]);
    }

    #[test]
    fn the_same_words_are_not_delivered_twice() {
        let stt = able(&["and so", "and so", "and so my fellow"]);
        let mut session = stt.open_session(brisk()).unwrap();

        let mut seen: Vec<String> = Vec::new();
        {
            let mut sink = |partial: Partial| seen.push(partial.text);
            for _ in 0..3 {
                session.push(&quiet(1600), Some(&mut sink)).unwrap();
            }
        }

        assert_eq!(seen, vec!["and so", "and so my fellow"]);
    }

    #[test]
    fn the_interval_holds_a_second_pass_back() {
        let stt = able(&["and so", "and so my fellow"]);
        let config = SessionConfig::new()
            .with_live_interims()
            .with_interim_min_interval(Duration::from_secs(3600));
        let mut session = stt.open_session(config).unwrap();

        let mut seen: Vec<String> = Vec::new();
        {
            let mut sink = |partial: Partial| seen.push(partial.text);
            session.push(&quiet(1600), Some(&mut sink)).unwrap();
            session.push(&quiet(1600), Some(&mut sink)).unwrap();
        }

        assert_eq!(seen, vec!["and so"]);
    }

    #[test]
    fn asking_a_recognizer_that_cannot_fails_at_open() {
        let stt = unable();
        let Err(why) = stt.open_session(SessionConfig::new().with_live_interims()) else {
            panic!("a recognizer that cannot should refuse");
        };
        assert!(
            why.to_string().contains("live_interims"),
            "the reason should name the setting, and said: {why}"
        );
    }

    #[test]
    fn a_recognizer_without_boundaries_of_its_own_needs_a_detector() {
        let stt = unable();
        let Err(why) = stt.open_session(SessionConfig::new()) else {
            panic!("no detector and no self-endpointing should refuse");
        };
        assert!(
            why.to_string().contains("endpointing"),
            "the reason should name what is missing, and said: {why}"
        );
    }

    #[test]
    fn several_sessions_may_be_open_against_one_recognizer() {
        let stt = able(&["first"]);
        let one = stt.open_session(SessionConfig::new());
        let two = stt.open_session(SessionConfig::new());
        assert!(one.is_ok() && two.is_ok(), "a second session was refused");
    }

    #[test]
    fn a_zero_interval_is_refused_at_open() {
        let stt = able(&[]);
        let config = SessionConfig::new().with_interim_min_interval(Duration::ZERO);
        let Err(why) = stt.open_session(config) else {
            panic!("a zero interval should be refused");
        };
        assert!(why.to_string().contains("interim_min_interval"), "{why}");
    }
}
