//! Words reach a caller while the speaker is still talking, and every
//! one of them says it replaces what came before rather than adding to
//! it -- because a pass re-recognises everything it has heard.

#![cfg(feature = "streaming")]

mod support;

use std::time::Duration;

use edge_stt_core::{
    Config, EdgeStt, EndpointConfig, Language, Partial, PartialKind, SessionConfig,
};

const SIMULATED_CHUNK: usize = 1_600; // 100ms of 16kHz audio

fn transcriber() -> EdgeStt {
    let config = Config::local(support::shared_model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

fn live_session() -> SessionConfig {
    SessionConfig::new()
        .with_endpointing(EndpointConfig::new(support::vad_model_path()))
        .with_live_interims()
        .with_interim_min_interval(Duration::from_millis(200))
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn words_arrive_before_the_speaker_stops() {
    let (samples, _) = support::spoken_sample();
    let stt = transcriber();
    let mut session = stt.open_session(live_session()).expect("a session");

    let total = samples.len() / SIMULATED_CHUNK;
    let mut first_at: Option<usize> = None;
    let mut seen = Vec::new();

    for (index, chunk) in samples.chunks(SIMULATED_CHUNK).enumerate() {
        {
            let mut show = |partial: Partial| {
                if first_at.is_none() {
                    first_at = Some(index);
                }
                assert_eq!(
                    partial.kind,
                    PartialKind::Replace,
                    "a pass re-recognises everything, so it replaces"
                );
                seen.push(partial.text);
            };
            session.push(chunk, Some(&mut show)).expect("a push");
        }
    }

    let first_at = first_at.expect("no words arrived while the speaker was talking");
    assert!(
        first_at < total / 2,
        "the first words waited until chunk {first_at} of {total}"
    );
    assert!(
        seen.len() > 1,
        "the caller was told once and never updated: {seen:?}"
    );
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_session_that_did_not_ask_hears_nothing_early() {
    let (samples, _) = support::spoken_sample();
    let stt = transcriber();
    let mut session = stt
        .open_session(EndpointConfig::new(support::vad_model_path()))
        .expect("a session");

    let mut seen = Vec::new();
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        let mut show = |partial: Partial| seen.push(partial.text);
        session.push(chunk, Some(&mut show)).expect("a push");
    }

    assert!(
        seen.is_empty(),
        "a session opened the way it always was received {seen:?}"
    );
}
