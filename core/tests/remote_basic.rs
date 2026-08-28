//! The same call, answered by a server instead of the device.

// Nothing here applies to a build that cannot reach a server.
#![cfg(feature = "remote")]

mod support;

use edge_stt_core::{BackendKind, Config, EdgeStt, RemoteConfig, Utterance};
use support::stub_server::{Behaviour, StubServer};

fn behaviour() -> Behaviour {
    Behaviour::Transcribe {
        text: "hello world".to_string(),
        partials: vec![],
    }
}

#[test]
fn a_recording_comes_back_as_a_transcript() {
    let server = StubServer::start(behaviour());
    let stt = EdgeStt::new(Config::remote(RemoteConfig::at(&server.endpoint))).expect("a client");

    let samples = support::silence(1.0);
    let transcript = stt
        .transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");

    assert_eq!(transcript.text, "hello world");
    assert_eq!(transcript.backend, BackendKind::Remote);
    assert_eq!(stt.backend_kind(), BackendKind::Remote);
}

#[test]
fn the_transcript_keeps_the_promises_the_device_makes() {
    let server = StubServer::start(behaviour());
    let stt = EdgeStt::new(Config::remote(RemoteConfig::at(&server.endpoint))).expect("a client");

    let samples = support::silence(1.0);
    let transcript = stt
        .transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");

    let joined: String = transcript
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(
        transcript.text,
        joined.trim(),
        "text must be the segments joined"
    );
    assert!(transcript.audio_duration > std::time::Duration::ZERO);
    assert!(!transcript.language.as_str().is_empty());
    assert!(transcript.real_time_factor() > 0.0);
}

#[test]
fn a_credential_is_presented_on_the_handshake() {
    let server = StubServer::start_with_credential(behaviour(), Some("open-sesame"));
    let remote = RemoteConfig::at(&server.endpoint).with_credential("open-sesame");
    let stt = EdgeStt::new(Config::remote(remote)).expect("a client");

    let samples = support::silence(1.0);
    assert!(stt.transcribe(&Utterance::mono_16k(&samples)).is_ok());
}
