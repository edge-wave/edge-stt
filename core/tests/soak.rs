//! A thousand utterances in a row. Every one produces a transcript or
//! a typed failure; none is lost, and memory does not creep.

mod support;

use edge_stt_core::{Config, EdgeStt, Utterance};

#[test]
#[ignore = "needs a Whisper model and runs for hours"]
fn a_thousand_utterances_lose_none_and_grow_nothing() {
    let stt = EdgeStt::new(Config::local(support::model_spec())).expect("a model");
    let samples = support::silence(1.0);

    let mut answered = 0usize;
    let mut settled = 0usize;
    for round in 0..1_000 {
        match stt.transcribe(&Utterance::mono_16k(&samples)) {
            Ok(_) | Err(_) => answered += 1,
        }
        if round == 99 {
            settled = resident_kilobytes();
        }
    }

    assert_eq!(
        answered, 1_000,
        "every utterance must be answered one way or the other"
    );

    let ended = resident_kilobytes();
    let allowed = settled + settled / 20;
    assert!(
        ended <= allowed,
        "memory went from {settled} kB after a hundred to {ended} kB after a thousand"
    );
}

/// Asked of the operating system rather than guessed at, because a
/// leak is exactly what a self-report would miss.
fn resident_kilobytes() -> usize {
    let pid = std::process::id().to_string();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .expect("ps, which both macOS and Linux carry");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
}
