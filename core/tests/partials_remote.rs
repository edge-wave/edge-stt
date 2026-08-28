//! Words arriving as they are decoded, over the wire.

// Nothing here applies to a build that cannot reach a server.
#![cfg(feature = "remote")]

mod support;

use edge_stt_core::{CancelToken, Config, EdgeStt, Partial, PartialKind, RemoteConfig, Utterance};
use support::stub_server::{Behaviour, StubServer};

fn pieces() -> Vec<String> {
    vec![
        "hello".to_string(),
        " brave".to_string(),
        " world".to_string(),
    ]
}

fn run(want_partials: bool) -> (Vec<Partial>, edge_stt_core::Transcript) {
    let server = StubServer::start(Behaviour::Transcribe {
        text: "hello brave world".to_string(),
        partials: pieces(),
    });
    let stt = EdgeStt::new(Config::remote(RemoteConfig::at(&server.endpoint))).expect("a client");

    let samples = support::silence(1.0);
    let cancel = CancelToken::new();
    let mut seen = Vec::new();
    // Handing over a callback is the whole of asking for partials.
    let transcript = if want_partials {
        stt.transcribe_with(&Utterance::mono_16k(&samples), |p| seen.push(p), &cancel)
    } else {
        stt.transcribe(&Utterance::mono_16k(&samples))
    }
    .expect("a transcript");
    (seen, transcript)
}

#[test]
fn partials_arrive_in_order_with_no_gaps() {
    let (seen, _) = run(true);
    assert_eq!(seen.len(), 3);
    for (expected, partial) in seen.iter().enumerate() {
        assert_eq!(
            partial.seq, expected as u32,
            "a gap would be undetectable otherwise"
        );
    }
}

#[test]
fn joining_the_partials_gives_the_final_text() {
    let (seen, transcript) = run(true);
    let joined: String = seen.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(joined, transcript.text);
}

#[test]
fn every_partial_says_it_extends_rather_than_replaces() {
    let (seen, _) = run(true);
    assert!(seen.iter().all(|p| p.kind == PartialKind::Append));
    assert!(
        seen.iter().all(|p| p.segment.is_some()),
        "timing comes with them"
    );
}

#[test]
fn a_caller_that_did_not_ask_is_sent_nothing() {
    let (seen, transcript) = run(false);
    assert!(seen.is_empty(), "partials cost nothing when unasked for");
    assert_eq!(transcript.text, "hello brave world");
}

#[test]
fn nothing_arrives_after_the_final_result() {
    let (seen, transcript) = run(true);
    let last = seen.last().expect("at least one partial");
    assert_eq!(last.seq, 2);
    assert!(
        transcript.text.ends_with(&last.text),
        "the last partial is part of the end"
    );
}
