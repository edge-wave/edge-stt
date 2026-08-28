//! Nothing said is an empty transcript, never an error and never
//! invented words.

mod support;

use edge_stt_core::{Config, EdgeStt, ModelSpec, Utterance};

fn transcriber() -> EdgeStt {
    EdgeStt::new(Config::local(ModelSpec::at(support::model_path()))).expect("a model")
}

#[test]
#[ignore = "needs a Whisper model"]
fn silence_comes_back_empty() {
    let samples = support::silence(3.0);
    let transcript = transcriber().transcribe(&Utterance::mono_16k(&samples)).expect("a result");
    assert!(transcript.is_empty(), "silence produced {:?}", transcript.text);
    assert_eq!(transcript.audio_duration, std::time::Duration::from_secs(3));
}

#[test]
#[ignore = "needs a Whisper model"]
fn hiss_comes_back_empty() {
    let samples = support::noise(3.0);
    let transcript = transcriber().transcribe(&Utterance::mono_16k(&samples)).expect("a result");
    assert!(transcript.is_empty(), "noise produced {:?}", transcript.text);
}

#[test]
#[ignore = "needs a Whisper model"]
fn a_fifth_of_a_second_is_silence_rather_than_an_error() {
    let samples = support::silence(0.2);
    let transcript = transcriber().transcribe(&Utterance::mono_16k(&samples)).expect("a result");
    assert!(transcript.is_empty());
    assert!(transcript.audio_duration > std::time::Duration::ZERO);
}
