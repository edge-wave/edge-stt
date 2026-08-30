//! The same promises, decoded here rather than asked for.

mod support;

use edge_stt_core::{CancelToken, Config, EdgeStt, ModelSpec, PartialKind, Utterance};

#[test]
#[ignore = "needs a Whisper model and a recording"]
fn partials_converge_on_the_final_text() {
    let long = support::long_spoken_sample();
    let stt = EdgeStt::new(Config::local(ModelSpec::at(support::model_path()))).expect("a model");

    let cancel = CancelToken::new();
    let mut seen = Vec::new();
    let transcript = stt
        .transcribe_with(&Utterance::mono_16k(&long), |p| seen.push(p), &cancel)
        .expect("a transcript");

    assert!(
        !seen.is_empty(),
        "a twenty second recording should decode in pieces"
    );
    assert!(
        seen.iter().all(|p| p.kind == PartialKind::Append),
        "whisper never revises"
    );
    for (expected, partial) in seen.iter().enumerate() {
        assert_eq!(partial.seq, expected as u32);
    }
    let joined: String = seen.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(joined.trim(), transcript.text);
}

#[test]
#[ignore = "needs a Whisper model and a recording"]
fn nothing_is_held_back_to_be_sent_together() {
    use std::time::Instant;

    let long = support::long_spoken_sample();
    let stt = EdgeStt::new(Config::local(ModelSpec::at(support::model_path()))).expect("a model");

    let cancel = CancelToken::new();
    let started = Instant::now();
    let mut arrivals = Vec::new();
    let transcript = stt
        .transcribe_with(&Utterance::mono_16k(&long), |_| arrivals.push(started.elapsed()), &cancel)
        .expect("a transcript");
    let whole = started.elapsed();

    // How soon the decoder produces its first segment is the model's
    // business and the board's. What is ours: each one goes out as it
    // arrives, and every one of them before the call returns.
    assert!(!arrivals.is_empty());
    assert!(arrivals.windows(2).all(|pair| pair[0] <= pair[1]), "partials arrived out of order");
    assert!(arrivals.iter().all(|at| *at <= whole), "a partial outlived the call");
    assert!(!transcript.text.is_empty());
}

#[test]
#[ignore = "needs a Whisper model"]
fn asking_for_nothing_costs_nothing() {
    let samples = support::silence(2.0);
    let stt = EdgeStt::new(Config::local(ModelSpec::at(support::model_path()))).expect("a model");

    let cancel = CancelToken::new();
    let mut seen = 0usize;
    stt.transcribe_with(&Utterance::mono_16k(&samples), |_| seen += 1, &cancel)
        .expect("a transcript");
    assert_eq!(
        seen, 0,
        "no callback is registered with the decoder unless asked for"
    );
}
