//! `session.close()` mid-utterance finalizes and delivers whatever
//! was captured. A second `close()` call is a no-op, not an
//! error, and does not re-deliver anything.

#![cfg(feature = "streaming")]

mod support;

use edge_stt_core::{Config, EdgeStt, EndpointConfig, Language};

fn transcriber() -> EdgeStt {
    let config = Config::local(support::model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn closing_mid_utterance_finalizes_and_delivers_it() {
    let (samples, expected) = support::spoken_sample();
    let stt = transcriber();
    let mut session = stt
        .open_session(EndpointConfig::new(support::vad_model_path()))
        .expect("a session");

    // Pushed in full, but never long enough to hit a natural pause or
    // the (default, five-minute) forced ceiling -- so nothing should
    // arrive from push() itself.
    for chunk in samples.chunks(1_600) {
        let delivered = session.push(chunk, None).expect("a push");
        assert!(
            delivered.is_none(),
            "the test setup should not have hit a natural boundary before close()"
        );
    }

    let closed = session
        .close(None)
        .expect("a clean close")
        .expect("the in-progress utterance");
    assert_eq!(closed.text.trim(), expected.trim());
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn closing_mid_utterance_still_delivers_partials() {
    let (samples, _expected) = support::spoken_sample();
    let stt = transcriber();
    let mut session = stt
        .open_session(EndpointConfig::new(support::vad_model_path()))
        .expect("a session");
    for chunk in samples.chunks(1_600) {
        session.push(chunk, None).expect("a push");
    }

    let mut seen = Vec::new();
    let mut sink = |p: edge_stt_core::Partial| seen.push(p);
    session
        .close(Some(&mut sink))
        .expect("a clean close")
        .expect("the in-progress utterance");
    assert!(
        !seen.is_empty(),
        "the utterance close() finalizes decodes the same way push() does -- it should carry partials too"
    );
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_second_close_is_a_no_op() {
    let (samples, _expected) = support::spoken_sample();
    let stt = transcriber();
    let mut session = stt
        .open_session(EndpointConfig::new(support::vad_model_path()))
        .expect("a session");
    for chunk in samples.chunks(1_600) {
        session.push(chunk, None).expect("a push");
    }

    session.close(None).expect("the first close");
    let second = session.close(None).expect("a second close must not error");
    assert!(
        second.is_none(),
        "a second close must return nothing, not re-deliver or error"
    );
}
