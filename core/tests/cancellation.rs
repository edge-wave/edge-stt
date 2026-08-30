//! Cancelling stops the decoder rather than waiting for it, and is not
//! confused with anything having gone wrong.

mod support;

use std::time::Duration;

use edge_stt_core::{CancelToken, Config, EdgeStt, Error, ModelSpec, Utterance};

fn transcriber() -> EdgeStt {
    EdgeStt::new(Config::local(ModelSpec::at(support::model_path()))).expect("a model")
}

#[test]
#[ignore = "needs a Whisper model"]
fn cancelling_before_the_start_returns_at_once() {
    let stt = transcriber();
    let samples = support::silence(30.0);
    let cancel = CancelToken::new();
    cancel.cancel();

    let started = std::time::Instant::now();
    let outcome = stt.transcribe_with(&Utterance::mono_16k(&samples), |_| {}, &cancel);

    assert!(matches!(outcome, Err(Error::Cancelled)));
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "it should not have decoded anything"
    );
}

#[test]
#[ignore = "needs a Whisper model"]
fn cancelling_from_another_thread_stops_the_decoder() {
    let stt = transcriber();
    let (samples, _) = support::spoken_sample();
    // Repeated on purpose: this audio is never decoded to the end,
    // and Whisper is slow on repetition, which is not a problem here.
    let long: Vec<i16> = samples.iter().cycle().take(16_000 * 60).copied().collect();

    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        trigger.cancel();
    });

    let started = std::time::Instant::now();
    let outcome = stt.transcribe_with(&Utterance::mono_16k(&long), |_| {}, &cancel);

    assert!(matches!(outcome, Err(Error::Cancelled)), "got {outcome:?}");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "cancelling should not wait for the end"
    );
}

#[test]
fn a_token_can_be_fired_more_than_once() {
    let cancel = CancelToken::new();
    assert!(!cancel.is_cancelled());
    cancel.cancel();
    cancel.cancel();
    assert!(cancel.is_cancelled());
    assert!(cancel.clone().is_cancelled());
}
