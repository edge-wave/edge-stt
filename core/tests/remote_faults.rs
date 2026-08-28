//! Each way a server can let you down reaches the caller as its own
//! answer, and none of them hangs.

// Nothing here applies to a build that cannot reach a server.
#![cfg(feature = "remote")]

mod support;

use std::time::{Duration, Instant};

use edge_stt_core::{Config, EdgeStt, Error, RemoteConfig, Utterance};
use support::stub_server::{Behaviour, StubServer};

fn client(endpoint: &str) -> EdgeStt {
    EdgeStt::new(Config::remote(RemoteConfig::at(endpoint))).expect("a client")
}

fn one_second() -> Vec<i16> {
    support::silence(1.0)
}

#[test]
fn a_host_that_is_not_there_is_a_network_failure() {
    let stt = client("ws://127.0.0.1:1/api/v1/transcribe");
    let samples = one_second();
    match stt.transcribe(&Utterance::mono_16k(&samples)) {
        Err(Error::Network { endpoint, .. }) => assert!(endpoint.contains("127.0.0.1:1")),
        other => panic!("expected a network failure, got {other:?}"),
    }
}

#[test]
fn a_wrong_credential_is_told_apart_from_a_dead_host() {
    let text = "unused".to_string();
    let server = StubServer::start_with_credential(
        Behaviour::Transcribe {
            text,
            partials: vec![],
        },
        Some("the-right-one"),
    );
    let remote = RemoteConfig::at(&server.endpoint).with_credential("the-wrong-one");
    let stt = EdgeStt::new(Config::remote(remote)).expect("a client");

    let samples = one_second();
    match stt.transcribe(&Utterance::mono_16k(&samples)) {
        Err(Error::CredentialRejected { .. }) => {}
        other => panic!("expected a refused credential, got {other:?}"),
    }
}

#[test]
fn a_connection_dropped_mid_request_never_looks_like_a_transcript() {
    let server = StubServer::start(Behaviour::DropMidRequest);
    let config = Config::remote(RemoteConfig::at(&server.endpoint));
    let stt = EdgeStt::new(config).expect("a client");

    let samples = one_second();
    let cancel = edge_stt_core::CancelToken::new();
    let mut seen = Vec::new();
    let outcome = stt.transcribe_with(
        &Utterance::mono_16k(&samples),
        |p| seen.push(p.text),
        &cancel,
    );

    assert!(
        matches!(outcome, Err(Error::Network { .. })),
        "got {outcome:?}"
    );
    assert_eq!(
        seen,
        vec!["half a".to_string()],
        "the partial arrived, and stayed a partial"
    );
}

#[test]
fn a_full_server_says_so_and_says_when_to_come_back() {
    let server = StubServer::start(Behaviour::AtCapacity);
    let stt = client(&server.endpoint);
    let samples = one_second();

    match stt.transcribe(&Utterance::mono_16k(&samples)) {
        Err(Error::ServerAtCapacity { retry_after, .. }) => {
            assert_eq!(retry_after, Some(Duration::from_millis(2000)));
        }
        other => panic!("expected a full server, got {other:?}"),
    }
}

#[test]
fn a_broken_server_is_not_a_broken_network() {
    let server = StubServer::start(Behaviour::ServerError);
    let stt = client(&server.endpoint);
    let samples = one_second();

    let outcome = stt.transcribe(&Utterance::mono_16k(&samples));
    assert!(
        matches!(outcome, Err(Error::ServerError { .. })),
        "got {outcome:?}"
    );
}

#[test]
fn a_server_that_says_nothing_runs_out_of_time_rather_than_hanging() {
    let server = StubServer::start(Behaviour::Stall);
    let limit = Duration::from_millis(300);
    let config = Config::remote(RemoteConfig::at(&server.endpoint)).with_timeout(limit);
    let stt = EdgeStt::new(config).expect("a client");

    let samples = one_second();
    let started = Instant::now();
    let outcome = stt.transcribe(&Utterance::mono_16k(&samples));
    let waited = started.elapsed();

    match outcome {
        Err(Error::Timeout { limit: reported }) => assert_eq!(reported, limit),
        other => panic!("expected a timeout, got {other:?}"),
    }
    assert!(waited < Duration::from_secs(5), "it waited {waited:?}");
}
