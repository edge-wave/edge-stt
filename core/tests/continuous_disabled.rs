//! Continuous input is opt-in. A build without `streaming`, or a
//! second session on one transcriber, must say so immediately --
//! never fall back to requiring pre-bounded utterances or behave
//! unpredictably.

mod support;

use edge_stt_core::{Config, EdgeStt};

#[test]
#[cfg(not(feature = "streaming"))]
#[ignore = "needs a Whisper model"]
fn a_default_build_carries_no_streaming_capability() {
    use edge_stt_core::{EndpointConfig, Error};

    let stt = EdgeStt::new(Config::local(support::model_spec())).expect("a model");
    let outcome =
        stt.open_session(EndpointConfig::new().with_local_vad_model("does-not-matter.bin"));
    assert!(
        matches!(
            outcome,
            Err(Error::BackendUnavailable {
                backend: "streaming"
            })
        ),
        "a build without the streaming feature must say so, got {outcome:?}",
        outcome = outcome.err()
    );
}

#[test]
#[cfg(feature = "streaming")]
#[ignore = "needs a Whisper model and a VAD model"]
fn opening_a_second_session_is_refused() {
    use edge_stt_core::EndpointConfig;

    let stt = EdgeStt::new(Config::local(support::model_spec())).expect("a model");
    let _first = stt
        .open_session(EndpointConfig::new().with_local_vad_model(support::vad_model_path()))
        .expect("the first session");

    let second =
        stt.open_session(EndpointConfig::new().with_local_vad_model(support::vad_model_path()));
    assert!(
        second.is_err(),
        "a second open session on the same transcriber must be refused"
    );
}

#[test]
#[cfg(feature = "streaming")]
#[ignore = "needs a Whisper model and a VAD model"]
fn closing_a_session_frees_it_for_a_new_one() {
    use edge_stt_core::EndpointConfig;

    let stt = EdgeStt::new(Config::local(support::model_spec())).expect("a model");
    let mut first = stt
        .open_session(EndpointConfig::new().with_local_vad_model(support::vad_model_path()))
        .expect("the first session");
    first.close(None).expect("a clean close");
    drop(first);

    stt.open_session(EndpointConfig::new().with_local_vad_model(support::vad_model_path()))
        .expect("closing the first session frees the slot for a second one");
}
