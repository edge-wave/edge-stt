//! The shortest program that gets a transcript, kept working.

mod support;

use edge_stt_core::{BackendKind, Config, EdgeStt, Language, ModelSpec, Utterance};

#[test]
#[ignore = "needs a Whisper model and a recording"]
fn three_lines_are_enough() {
    let (samples, expected) = support::spoken_sample();

    let config = Config::local(ModelSpec::at(support::model_path()))
        .with_language(Language::new(support::sample_language()));
    let stt = EdgeStt::new(config).expect("a model");
    let transcript = stt
        .transcribe(&Utterance::mono_16k(&samples))
        .expect("a transcript");

    assert!(
        transcript
            .text
            .to_lowercase()
            .contains(&expected.to_lowercase()),
        "expected {expected:?} somewhere in {:?}",
        transcript.text
    );
    assert_eq!(transcript.backend, BackendKind::Local);
    assert!(!transcript.segments.is_empty());
    assert_eq!(
        transcript.text,
        transcript
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>()
            .trim()
    );
    assert!(transcript.audio_duration > std::time::Duration::ZERO);
    assert!(transcript.processing_time > std::time::Duration::ZERO);
}
