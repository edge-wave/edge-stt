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
