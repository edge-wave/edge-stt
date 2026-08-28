//! A partial is never mistaken for a finished transcript, and nothing
//! follows the end.

// Nothing here applies to a build that cannot reach a server.
#![cfg(feature = "remote")]

mod support;

use edge_stt_core::{CancelToken, Config, EdgeStt, Error, RemoteConfig, Utterance};
use support::stub_server::{Behaviour, StubServer};

#[test]
fn a_failure_after_partials_tells_the_caller_it_will_not_finish() {
    let server = StubServer::start(Behaviour::DropMidRequest);
    let config = Config::remote(RemoteConfig::at(&server.endpoint));
    let stt = EdgeStt::new(config).expect("a client");

    let samples = support::silence(1.0);
    let cancel = CancelToken::new();
    let mut seen = Vec::new();
    let outcome = stt.transcribe_with(&Utterance::mono_16k(&samples), |p| seen.push(p), &cancel);

    assert_eq!(seen.len(), 1, "one partial was delivered");
    assert!(
        matches!(outcome, Err(Error::Network { .. })),
        "got {outcome:?}"
    );
}

#[test]
fn cancelling_before_the_start_delivers_no_partial_at_all() {
    let server = StubServer::start(Behaviour::Transcribe {
        text: "never seen".to_string(),
        partials: vec!["never".to_string()],
    });
    let config = Config::remote(RemoteConfig::at(&server.endpoint));
    let stt = EdgeStt::new(config).expect("a client");

    let cancel = CancelToken::new();
    cancel.cancel();
    let samples = support::silence(1.0);
    let mut seen = 0usize;
    let outcome = stt.transcribe_with(&Utterance::mono_16k(&samples), |_| seen += 1, &cancel);

    assert!(matches!(outcome, Err(Error::Cancelled)));
    assert_eq!(seen, 0);
}
