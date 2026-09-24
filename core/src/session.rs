//! A caller-fed, open-ended audio stream. Where `transcribe`/
//! `transcribe_with` take one complete recording, this takes samples
//! as they arrive and ends an utterance where its boundaries say --
//! found here, or left to the caller -- then decodes it the same way.

use std::time::Duration;

use crate::EdgeStt;
use crate::backend::LiveDecoder;
use crate::cancel::CancelToken;
use crate::config::{Boundaries, SessionConfig};
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
    /// Absent for a recognizer that finds its own boundaries, and for
    /// a caller that decides them.
    endpointer: Option<Box<dyn Endpointer + 'a>>,
    /// Whether only `close` and the maximum duration end an utterance.
    caller_bounded: bool,
    /// Present only when the caller asked for words while speech
    /// continues, so a caller who did not pays nothing for this.
    live: Option<Box<dyn LiveDecoder>>,
    gate: InterimGate,
    /// Whether the caller asked to hear words during an utterance. A
    /// decoder may be present without this, to find boundaries.
    wants_interims: bool,
    /// Everything heard in the utterance in progress. Kept whenever a
    /// decoder needs it or no detector is holding it instead.
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
            caller_bounded: matches!(config.boundaries, Boundaries::Caller),
            live,
            gate: InterimGate::new(config.interim_min_interval),
            wants_interims: config.live_interims,
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
    /// one utterance finished. With boundaries left to the caller only
    /// the ceiling can. Otherwise `None`, with the audio staying
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
        if self.live.is_some() || self.endpointer.is_none() {
            self.utterance.extend_from_slice(samples);
        }

        if let Some(partial) = self.recognise_so_far()?
            && let Some(sink) = on_partial.as_deref_mut()
        {
            sink(partial);
        }

        if let Some(endpointer) = self.endpointer.as_mut() {
            if let Some(finished) = endpointer.push(samples)? {
                return self.finish(finished, on_partial).map(Some);
            }
        } else if !self.caller_bounded
            && let Some(at) = self.live.as_ref().and_then(|decoder| decoder.boundary())
        {
            let finished = self.utterance[..at.min(self.utterance.len())].to_vec();
            return self.finish(finished, on_partial).map(Some);
        }

        // The same ceiling either way: one canonical "too long", not a
        // second one for a recognizer that finds its own boundaries.
        if self.buffered >= self.max_duration
            && let Some(finished) = self.take_everything()
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

        match self.take_everything() {
            Some(finished) => self.finish(finished, on_partial).map(Some),
            None => Ok(None),
        }
    }

    /// Whatever is still buffered, from wherever it is held. A boundary
    /// detector keeps it; without one the session does.
    fn take_everything(&mut self) -> Option<Vec<i16>> {
        match self.endpointer.as_mut() {
            Some(endpointer) => endpointer.take_remainder(),
            None => match self.utterance.is_empty() {
                true => None,
                false => Some(self.utterance.clone()),
            },
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
            wants_interims,
            ..
        } = self;

        let Some(decoder) = live.as_mut() else {
            return Ok(None);
        };
        if !*wants_interims {
            // Still pushed, because a decoder that finds its own
            // boundaries has to hear the audio to find them.
            decoder.push(utterance)?;
            return Ok(None);
        }
        let heard = utterance.len();
        if !gate.due(heard) {
            return Ok(None);
        }
        let Some(text) = decoder.push(utterance)? else {
            return Ok(None);
        };
        if !gate.admit(heard, &text) {
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
        /// Where this one says an utterance ends, for the kind that
        /// decides that itself. Zero means it never does.
        closes_at: usize,
        heard: usize,
    }

    impl LiveDecoder for Scripted {
        fn push(&mut self, utterance_so_far: &[i16]) -> Result<Option<String>> {
            self.heard = utterance_so_far.len();
            let line = self.lines.get(self.at).cloned();
            if line.is_some() {
                self.at += 1;
            }
            Ok(line)
        }

        fn reset(&mut self) {
            self.at = 0;
            self.heard = 0;
        }

        fn boundary(&self) -> Option<usize> {
            match self.closes_at > 0 && self.heard >= self.closes_at {
                true => Some(self.closes_at),
                false => None,
            }
        }
    }

    struct Fake {
        can: Capabilities,
        lines: Vec<String>,
        closes_at: usize,
    }

    impl Backend for Fake {
        fn transcribe(
            &self,
            utterance: &Utterance<'_>,
            _: &mut Work<'_, '_>,
        ) -> Result<Transcript> {
            Ok(Transcript::empty(
                Language::new("en"),
                utterance.duration(),
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
                closes_at: self.closes_at,
                heard: 0,
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
            closes_at: 0,
        }))
    }

    /// One that ends an utterance after `closes_at` samples of its own
    /// accord, needing no boundary detector beside it.
    fn self_endpointing(closes_at: usize) -> EdgeStt {
        EdgeStt::with_backend(Box::new(Fake {
            can: Capabilities {
                live_interims: true,
                revises: true,
                self_endpointing: true,
            },
            lines: vec!["and so".to_string()],
            closes_at,
        }))
    }

    fn unable() -> EdgeStt {
        EdgeStt::with_backend(Box::new(Fake {
            can: Capabilities::default(),
            lines: Vec::new(),
            closes_at: 0,
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
    fn a_recognizer_that_endpoints_itself_needs_no_detector() {
        let stt = self_endpointing(3_200);
        let mut session = stt
            .open_session(SessionConfig::new())
            .expect("no detector should be needed");

        let mut finished = 0;
        for _ in 0..4 {
            if session.push(&quiet(1_600), None).expect("a push").is_some() {
                finished += 1;
            }
        }
        assert_eq!(finished, 2, "expected one utterance per two pushes");
    }

    #[test]
    fn the_same_ceiling_holds_without_a_detector() {
        let stt = self_endpointing(0);
        let mut session = stt
            .open_session(SessionConfig::new())
            .expect("no detector should be needed");

        // Never closed by the recogniser, so only the ceiling can end
        // it -- fed as a caller would, or the utterance would be over
        // the limit before anything got the chance to close it.
        let mut closed_after = None;
        for pushed in 1..=4_000 {
            if session.push(&quiet(1_600), None).expect("a push").is_some() {
                closed_after = Some(pushed);
                break;
            }
        }
        let closed_after = closed_after.expect("the maximum duration did not close it");
        // The default ceiling is five minutes, and a push is a tenth of
        // a second, so it should land there rather than anywhere else.
        assert_eq!(closed_after, 3_000);
    }

    #[test]
    fn a_caller_bounded_session_opens_without_a_detector() {
        let stt = unable();
        let mut session = stt
            .open_session(SessionConfig::new().with_caller_boundaries())
            .expect("a caller deciding the boundaries needs no detector");

        for _ in 0..30 {
            let early = session.push(&quiet(1_600), None).expect("a push");
            assert!(early.is_none(), "nothing but close should end it");
        }
        let finished = session
            .close(None)
            .expect("a close")
            .expect("the utterance");
        assert_eq!(finished.audio_duration, Duration::from_secs(3));
    }

    #[test]
    fn a_caller_bounded_session_ignores_where_the_recognizer_stops() {
        let stt = self_endpointing(3_200);
        let config = SessionConfig::new()
            .with_caller_boundaries()
            .with_live_interims()
            .with_interim_min_interval(Duration::from_nanos(1));
        let mut session = stt.open_session(config).unwrap();

        let mut seen: Vec<String> = Vec::new();
        {
            let mut sink = |partial: Partial| seen.push(partial.text);
            for _ in 0..4 {
                let early = session.push(&quiet(1_600), Some(&mut sink)).unwrap();
                assert!(early.is_none(), "the recognizer ended it");
            }
        }
        assert_eq!(seen, vec!["and so"], "interims should still arrive");
        let finished = session.close(None).unwrap().expect("the utterance");
        assert_eq!(finished.audio_duration, Duration::from_millis(400));
    }

    #[test]
    fn a_caller_bounded_session_is_still_held_to_the_ceiling() {
        let stt = unable();
        let mut session = stt
            .open_session(SessionConfig::new().with_caller_boundaries())
            .unwrap();

        let mut closed_after = None;
        for pushed in 1..=4_000 {
            if session.push(&quiet(1_600), None).expect("a push").is_some() {
                closed_after = Some(pushed);
                break;
            }
        }
        assert_eq!(closed_after, Some(3_000));
        assert!(
            session.close(None).unwrap().is_none(),
            "the ceiling should have taken everything"
        );
    }

    #[test]
    fn closing_a_caller_bounded_session_with_nothing_heard_delivers_nothing() {
        let stt = unable();
        let mut session = stt
            .open_session(SessionConfig::new().with_caller_boundaries())
            .unwrap();
        assert!(
            session.close(None).unwrap().is_none(),
            "nothing was pushed, so there is nothing to deliver"
        );
        assert!(session.push(&quiet(1_600), None).unwrap().is_none());
        assert!(
            session.close(None).unwrap().is_none(),
            "a closed session took audio"
        );
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
