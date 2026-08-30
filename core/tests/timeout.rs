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
    // Repeated on purpose: this audio is never decoded to the end,
    // and Whisper is slow on repetition, which is not a problem here.
    let long: Vec<i16> = samples.iter().cycle().take(16_000 * 120).copied().collect();

    let started = Instant::now();
    let outcome = stt.transcribe(&Utterance::mono_16k(&long));
    let waited = started.elapsed();

    match outcome {
        Err(Error::Timeout { limit: reported }) => assert_eq!(reported, limit),
        other => panic!("expected a timeout, got {other:?}"),
    }
    // The abort is seen between the decoder's own steps, and how long
    // one of those takes belongs to the machine. What must hold is
    // that it gave up rather than transcribing the whole two minutes.
    let one_clip = stt
        .transcribe(&Utterance::mono_16k(&samples))
        .map(|t| t.processing_time)
        .unwrap_or(Duration::from_secs(1));
    let all_of_it = one_clip * 60;
    assert!(waited < all_of_it, "it waited {waited:?}; the whole would take about {all_of_it:?}");
}

#[test]
#[ignore = "needs a Whisper model"]
fn a_timeout_is_not_a_cancellation() {
    let config = Config::local(ModelSpec::at(support::model_path()))
        .with_timeout(Duration::from_millis(150));
    let stt = EdgeStt::new(config).expect("a model");

    let (samples, _) = support::spoken_sample();
    // Repeated on purpose: this audio is never decoded to the end,
    // and Whisper is slow on repetition, which is not a problem here.
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
