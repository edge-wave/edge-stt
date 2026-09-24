//! Falling back is a thing the caller asks for, never a thing that
//! happens quietly.

// Nothing here applies to a build that cannot reach a server.
#![cfg(feature = "remote")]

mod support;

use edge_stt_core::{Config, EdgeStt, Error, RemoteConfig, Utterance};
use support::stub_server::{Behaviour, StubServer};

const NOWHERE: &str = "ws://127.0.0.1:1/api/v1/transcribe";

#[test]
fn without_fallback_a_remote_failure_reaches_the_caller_untouched() {
    let stt = EdgeStt::new(Config::remote(RemoteConfig::at(NOWHERE))).expect("a client");
    let samples = support::silence(1.0);
    assert!(matches!(
        stt.transcribe(&Utterance::mono_16k(&samples)),
        Err(Error::Network { .. })
    ));
}

#[test]
#[ignore = "needs a Whisper model"]
fn with_fallback_a_dead_server_is_answered_by_the_device() {
    use edge_stt_core::BackendKind;

    let config =
        Config::remote(RemoteConfig::at(NOWHERE)).with_fallback_to_local(support::model_spec());
    let stt = EdgeStt::new(config).expect("a client and a model");

    let samples = support::silence(1.0);
    let transcript = stt
        .transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");
    assert_eq!(
        transcript.backend,
        BackendKind::Local,
        "the result must say where it came from"
    );
}

#[test]
#[ignore = "needs a Whisper model"]
fn nothing_falls_back_once_the_caller_has_already_seen_words() {
    let server = StubServer::start(Behaviour::DropMidRequest);
    let config = Config::remote(RemoteConfig::at(&server.endpoint))
        .with_fallback_to_local(support::model_spec());
    let stt = EdgeStt::new(config).expect("a client and a model");

    let samples = support::silence(1.0);
    let cancel = edge_stt_core::CancelToken::new();
    let mut seen = Vec::new();
    let outcome = stt.transcribe_with(
        &Utterance::mono_16k(&samples),
        |p| seen.push(p.seq),
        &cancel,
    );

    assert_eq!(seen, vec![0], "one partial was delivered");
    assert!(
        matches!(outcome, Err(Error::Network { .. })),
        "restarting the sequence would contradict what the caller was shown, got {outcome:?}"
    );
}

#[test]
#[ignore = "needs a Whisper model"]
#[cfg(feature = "streaming")]
fn a_session_the_server_refuses_opens_on_the_device_and_stays_there() {
    use edge_stt_core::{BackendKind, SessionConfig};
    use support::stub_server::StreamScript;

    let script = StreamScript {
        refuse: Some("at_capacity"),
        ..StreamScript::default()
    };
    let server = StubServer::start(Behaviour::Stream(script.clone()));
    let config = Config::remote(RemoteConfig::at(&server.endpoint))
        .with_fallback_to_local(support::model_spec());
    let stt = EdgeStt::new(config).expect("a client and a model");

    let mut session = stt
        .open_session(SessionConfig::new().with_caller_boundaries())
        .expect("a session on the device");
    session.push(&support::silence(1.0), None).expect("a push");
    let closed = session.close(None).expect("a close");

    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].backend, BackendKind::Local);
    let log = script.log.lock().unwrap();
    assert!(log.opened_with.is_some(), "the server was asked first");
    assert_eq!(log.requests, 0, "the device decoded without asking again");
}

#[test]
#[ignore = "needs a Whisper model"]
#[cfg(feature = "streaming")]
fn a_detector_session_falling_back_needs_an_on_device_model() {
    use edge_stt_core::{EndpointConfig, SessionConfig};

    let config =
        Config::remote(RemoteConfig::at(NOWHERE)).with_fallback_to_local(support::model_spec());
    let stt = EdgeStt::new(config).expect("a client and a model");
    let detector = SessionConfig::new().with_endpointing(EndpointConfig::new());
    match stt.open_session(detector) {
        Err(Error::InvalidValue { setting, .. }) => assert_eq!(setting, "local_vad_model"),
        Err(other) => panic!("expected the missing model to be named, got {other}"),
        Ok(_) => panic!("a device detector opened with no model"),
    }
}
