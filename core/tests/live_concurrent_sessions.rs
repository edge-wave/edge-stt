//! Two sessions open at once against one loaded recogniser, each
//! producing exactly what it produces alone. This is the limit that was
//! lifted: the state a session needs is now its own, so nothing one
//! session hears can reach another.

#![cfg(feature = "streaming")]

mod support;

use std::time::Duration;

use edge_stt_core::{
    AudioSession, Config, EdgeStt, EndpointConfig, Language, Partial, SessionConfig,
};

const SIMULATED_CHUNK: usize = 1_600; // 100ms of 16kHz audio

fn transcriber() -> EdgeStt {
    let config = Config::local(support::model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

fn live() -> SessionConfig {
    SessionConfig::new()
        .with_endpointing(EndpointConfig::new(support::vad_model_path()))
        .with_live_interims()
        .with_interim_min_interval(Duration::from_millis(200))
}

/// Feeds one session to exhaustion and returns the utterances it found.
fn drain(session: &mut AudioSession<'_>, samples: &[i16]) -> Vec<String> {
    let mut texts = Vec::new();
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        let mut show = |_: Partial| {};
        if let Some(transcript) = session.push(chunk, Some(&mut show)).expect("a push") {
            texts.push(transcript.text);
        }
    }
    let mut show = |_: Partial| {};
    if let Some(transcript) = session.close(Some(&mut show)).expect("a close") {
        texts.push(transcript.text);
    }
    texts
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_second_session_no_longer_refuses_to_open() {
    let stt = transcriber();
    let one = stt.open_session(live());
    let two = stt.open_session(live());
    assert!(one.is_ok(), "the first session was refused");
    assert!(two.is_ok(), "the second session was refused");
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn interleaved_sessions_each_produce_what_they_produce_alone() {
    let (samples, expected) = support::spoken_sample();
    let stt = transcriber();

    let mut one = stt.open_session(live()).expect("a session");
    let mut two = stt.open_session(live()).expect("a session");

    // Interleaved rather than one after the other, so any state they
    // shared would show up as one answer polluting the other.
    let mut texts_one = Vec::new();
    let mut texts_two = Vec::new();
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        let mut show = |_: Partial| {};
        if let Some(transcript) = one.push(chunk, Some(&mut show)).expect("a push") {
            texts_one.push(transcript.text);
        }
        let mut show = |_: Partial| {};
        if let Some(transcript) = two.push(chunk, Some(&mut show)).expect("a push") {
            texts_two.push(transcript.text);
        }
    }
    let mut show = |_: Partial| {};
    if let Some(transcript) = one.close(Some(&mut show)).expect("a close") {
        texts_one.push(transcript.text);
    }
    let mut show = |_: Partial| {};
    if let Some(transcript) = two.close(Some(&mut show)).expect("a close") {
        texts_two.push(transcript.text);
    }

    assert_eq!(texts_one, texts_two, "the two sessions disagreed");
    assert_eq!(texts_one.len(), 1, "expected one utterance: {texts_one:?}");
    assert_eq!(texts_one[0].trim(), expected.trim());

    // And the same as one session run with nothing else open.
    let stt = transcriber();
    let mut alone = stt.open_session(live()).expect("a session");
    assert_eq!(drain(&mut alone, &samples), texts_one);
}
