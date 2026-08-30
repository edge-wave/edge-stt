//! One body of calling code, both backends. This test is what keeps
//! them from drifting apart after everyone has forgotten the promise.

// Nothing here applies to a build that cannot reach a server.
#![cfg(feature = "remote")]

mod support;

use edge_stt_core::{EdgeStt, Result, Transcript, Utterance};
use support::stub_server::{Behaviour, StubServer};

/// The caller. It cannot tell which backend it holds, and that is the
/// property being tested.
fn caller(stt: &EdgeStt, samples: &[i16]) -> Result<Transcript> {
    stt.transcribe(&Utterance::mono_16k(samples))
}

fn shape_of(transcript: &Transcript) -> (bool, bool, bool, bool) {
    let joined: String = transcript
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    (
        transcript.text == joined.trim(),
        !transcript.language.as_str().is_empty(),
        transcript.audio_duration > std::time::Duration::ZERO,
        transcript.processing_time > std::time::Duration::ZERO,
    )
}

#[test]
fn the_remote_backend_answers_the_shared_caller() {
    let server = StubServer::start(Behaviour::Transcribe {
        text: "hello world".to_string(),
        partials: vec![],
    });
    let config = edge_stt_core::Config::remote(edge_stt_core::RemoteConfig::at(&server.endpoint));
    let stt = EdgeStt::new(config).expect("a client");

    let samples = support::silence(1.0);
    let transcript = caller(&stt, &samples).expect("a transcript");
    assert_eq!(shape_of(&transcript), (true, true, true, true));
}

#[test]
#[ignore = "needs a Whisper model and a recording"]
fn both_backends_answer_the_same_caller_the_same_way() {
    let (samples, expected) = support::spoken_sample();

    let local = EdgeStt::new(
        edge_stt_core::Config::local(support::model_spec())
            .with_language(edge_stt_core::Language::new(support::sample_language())),
    )
    .expect("a model");
    let from_device = caller(&local, &samples).expect("a transcript");

    let server = StubServer::start(Behaviour::Transcribe {
        text: from_device.text.clone(),
        partials: vec![],
    });
    let remote = EdgeStt::new(edge_stt_core::Config::remote(
        edge_stt_core::RemoteConfig::at(&server.endpoint),
    ))
    .expect("a client");
    let from_server = caller(&remote, &samples).expect("a transcript");

    assert!(
        from_device
            .text
            .to_lowercase()
            .contains(&expected.to_lowercase())
    );
    assert_eq!(from_device.text, from_server.text);
    assert_eq!(shape_of(&from_device), shape_of(&from_server));
    assert_ne!(
        from_device.backend, from_server.backend,
        "only this may differ"
    );
}
