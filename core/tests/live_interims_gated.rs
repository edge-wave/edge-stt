//! Nothing is delivered twice, and nothing arrives faster than the
//! interval allows. A speaker who pauses mid-sentence produces the same
//! hypothesis pass after pass, and sending it again is pure waste on
//! every path and visible flicker on some.

#![cfg(feature = "streaming")]

mod support;

use std::time::Duration;

use edge_stt_core::{Config, EdgeStt, EndpointConfig, Language, Partial, SessionConfig};

const SIMULATED_CHUNK: usize = 1_600; // 100ms of 16kHz audio

fn transcriber() -> EdgeStt {
    let config = Config::local(support::model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

fn interims_for(samples: &[i16], interval: Duration) -> Vec<String> {
    let stt = transcriber();
    let config = SessionConfig::new()
        .with_endpointing(EndpointConfig::new(support::vad_model_path()))
        .with_live_interims()
        .with_interim_min_interval(interval);
    let mut session = stt.open_session(config).expect("a session");

    let mut seen = Vec::new();
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        let mut show = |partial: Partial| seen.push(partial.text);
        session.push(chunk, Some(&mut show)).expect("a push");
    }
    seen
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_quiet_stretch_delivers_no_repeated_words() {
    let (sentence, _) = support::spoken_sample();
    // Speech, then a stretch of quiet that stays under the pause
    // tolerance, so the utterance is still open and passes keep running.
    let mut samples = sentence;
    samples.extend(support::silence(2.0));

    let seen = interims_for(&samples, Duration::from_millis(100));
    assert!(!seen.is_empty(), "nothing arrived at all");

    for pair in seen.windows(2) {
        assert_ne!(
            pair[0], pair[1],
            "the same words were delivered twice in a row: {seen:?}"
        );
    }
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_longer_interval_delivers_less() {
    let (samples, _) = support::spoken_sample();
    let brisk = interims_for(&samples, Duration::from_millis(100));
    let patient = interims_for(&samples, Duration::from_secs(3));

    assert!(
        patient.len() < brisk.len(),
        "the interval changed nothing: {} against {}",
        patient.len(),
        brisk.len()
    );
}
