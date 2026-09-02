//! An integrator can move the boundary earlier or later by
//! configuring `pause_tolerance`, instead of being stuck with one
//! system-wide default.

#![cfg(feature = "streaming")]

mod support;

use std::time::Duration;

use edge_stt_core::{Config, EdgeStt, EndpointConfig, Language};

fn transcriber() -> EdgeStt {
    let config = Config::local(support::model_spec())
        .with_language(Language::new(support::sample_language()));
    EdgeStt::new(config).expect("a model")
}

fn feed_in_chunks(config: EndpointConfig, samples: &[i16]) -> Vec<String> {
    const SIMULATED_CHUNK: usize = 1_600;
    let stt = transcriber();
    let mut session = stt.open_session(config).expect("a session");
    let mut texts = Vec::new();
    for chunk in samples.chunks(SIMULATED_CHUNK) {
        if let Some(transcript) = session.push(chunk, None).expect("a push") {
            texts.push(transcript.text);
        }
    }
    if let Some(transcript) = session.close().expect("a clean close") {
        texts.push(transcript.text);
    }
    texts
}

#[test]
#[ignore = "needs a Whisper model and a VAD model"]
fn a_shorter_configured_tolerance_splits_a_pause_a_longer_one_does_not() {
    // The sample itself has a natural pause of roughly a second and a
    // half -- no inserted silence needed to see the two behaviours.
    let (samples, _expected) = support::spoken_sample();

    let strict = EndpointConfig::new(support::vad_model_path())
        .with_pause_tolerance(Duration::from_millis(1_000));
    let lenient = EndpointConfig::new(support::vad_model_path())
        .with_pause_tolerance(Duration::from_millis(2_000));

    let split = feed_in_chunks(strict, &samples);
    let unsplit = feed_in_chunks(lenient, &samples);

    assert!(
        split.len() > 1,
        "a 1000ms tolerance is under the sample's own pause and must split, got {split:?}"
    );
    assert_eq!(
        unsplit.len(),
        1,
        "a 2000ms tolerance is past the sample's own pause and must not split, got {unsplit:?}"
    );
}
