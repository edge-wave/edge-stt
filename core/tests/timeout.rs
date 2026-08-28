//! A limit that is reached is its own answer, told apart from being
//! cancelled and never delivered late.

mod support;

use std::time::{Duration, Instant};

use edge_stt_core::{CancelToken, Config, EdgeStt, Error, ModelSpec, Utterance};

#[test]
#[ignore = "needs a Whisper model"]
fn a_limit_shorter_than_the_work_is_reported_as_a_timeout() {
    let limit = Duration::from_millis(150);
    let config = Config::local(ModelSpec::at(support::model_path())).with_timeout(limit);
    let stt = EdgeStt::new(config).expect("a model");

    let (samples, _) = support::spoken_sample();
    let long: Vec<i16> = samples.iter().cycle().take(16_000 * 120).copied().collect();

    let started = Instant::now();
    let outcome = stt.transcribe(&Utterance::mono_16k(&long));
    let waited = started.elapsed();

    match outcome {
        Err(Error::Timeout { limit: reported }) => assert_eq!(reported, limit),
        other => panic!("expected a timeout, got {other:?}"),
    }
    assert!(
        waited < Duration::from_secs(20),
        "it waited {waited:?}, well past the limit"
    );
}

#[test]
#[ignore = "needs a Whisper model"]
fn a_timeout_is_not_a_cancellation() {
    let config = Config::local(ModelSpec::at(support::model_path()))
        .with_timeout(Duration::from_millis(150));
    let stt = EdgeStt::new(config).expect("a model");

    let (samples, _) = support::spoken_sample();
    let long: Vec<i16> = samples.iter().cycle().take(16_000 * 120).copied().collect();
    let cancel = CancelToken::new();

    let outcome = stt.transcribe_with(&Utterance::mono_16k(&long), |_| {}, &cancel);
    assert!(
        matches!(outcome, Err(Error::Timeout { .. })),
        "got {outcome:?}"
    );
    assert!(
        !cancel.is_cancelled(),
        "nothing cancelled this; the limit ran out"
    );
}

#[test]
#[ignore = "needs a Whisper model"]
fn a_generous_limit_lets_the_work_finish() {
    let config =
        Config::local(ModelSpec::at(support::model_path())).with_timeout(Duration::from_secs(300));
    let stt = EdgeStt::new(config).expect("a model");
    let (samples, expected) = support::spoken_sample();

    let transcript = stt
        .transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");
    assert!(
        transcript
            .text
            .to_lowercase()
            .contains(&expected.to_lowercase())
    );
}
