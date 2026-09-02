//! An utterance that never produces a qualifying pause is still
//! closed and delivered once it reaches the same maximum-duration
//! ceiling a pre-bounded utterance is already held to -- never held
//! open indefinitely (FR-009).

#![cfg(feature = "streaming")]

mod support;

use std::time::Duration;

use edge_stt_core::{Config, EdgeStt, EndpointConfig, Language};

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_ceiling_shorter_than_the_speech_forces_a_close_without_calling_close() {
    let (samples, _expected) = support::spoken_sample();

    // Shorter than any real spoken sample, and shorter than the
    // default pause tolerance, so the forced ceiling -- not a natural
    // pause -- is what has to produce the result.
    let config = Config::local(support::model_spec())
        .with_language(Language::new(support::sample_language()))
        .with_max_duration(Duration::from_secs(1));
    let stt = EdgeStt::new(config).expect("a model");
    let mut session = stt
        .open_session(EndpointConfig::new(support::vad_model_path()))
        .expect("a session");

    let mut texts = Vec::new();
    for chunk in samples.chunks(1_600) {
        if let Some(transcript) = session.push(chunk, None).expect("a push") {
            texts.push(transcript.text);
        }
    }

    assert!(
        !texts.is_empty(),
        "a one-second ceiling on a longer recording must force at least one utterance closed \
         from push() alone, before the caller ever calls close()"
    );
}
